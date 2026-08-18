//! Sessions as spawned tasks (D9): one actor per resident session, hydrate-act-commit-discard.
//!
//! An idle session is nothing but its journal. The actor holds the cached fold (history,
//! head, lease, hand adapter); after `idle_discard` without traffic it releases the lease,
//! drops the adapter and exits -- the next message hydrates from the journal (PD-11 measured
//! the rehydrate at constant ~4 ms). Everything the actor holds is rebuildable; everything
//! durable went through `Journal::commit` first.
//!
//! The brain is COMPOSED, not configured into a cloud: [`Brain::with_parts`] takes a journal
//! store, a key custody, a hand factory and (optionally) a provider factory -- all trait
//! objects (see [`crate::adapter`]). [`Brain::local`] is the zero-setup composition; the AWS
//! one lives in `brain-aws`; yours is whatever you hand in.

use crate::adapter::{HandAdapter, HandFactory, HandSpec, SeedFile};
use crate::compact::DEFAULT_HISTORY_BUDGET_BYTES;
use crate::config::{AgentDef, Dialect, GenOpts, ProviderKey, SessionConfig};
use crate::events::EventHub;
use crate::journal::{ArtifactDoc, FailureDoc, Head, HeadDoc, Journal, Lease, PrefixDoc, Record};
use crate::keys::{KeyCustody, blob_from_b64, blob_to_b64};
use crate::local::LocalFactory;
use crate::message::{ContentBlock, Message};
use crate::provider::Provider;
use crate::tools::TodoState;
use crate::turn::{TurnRun, TurnState};
use crate::{BrainError, Result};
use aex_contracts::session::{
    self, CreateSessionRequest, MessageRequestContent, Provider as ApiProvider,
};
use base64::Engine;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Process configuration: the knobs that are NOT adapters.
#[derive(Debug, Clone)]
pub struct BrainConfig {
    /// Admission: concurrent model rounds across the process.
    pub max_concurrent_model_rounds: usize,
    /// Admission: concurrent active turns across the process.
    pub max_concurrent_turns: usize,
    /// Idle residency before the actor discards its fold and exits.
    pub idle_discard: Duration,
    pub history_budget_bytes: usize,
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            max_concurrent_model_rounds: env_num("AEX_MAX_MODEL_ROUNDS", 64),
            max_concurrent_turns: env_num("AEX_MAX_TURNS", 64),
            idle_discard: Duration::from_secs(env_num("AEX_IDLE_DISCARD_SECONDS", 900) as u64),
            history_budget_bytes: DEFAULT_HISTORY_BUDGET_BYTES,
        }
    }
}

fn env_num(k: &str, default: usize) -> usize {
    std::env::var(k)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Process-wide reclaim policy (PD-13: >=97% of a dropped session's memory returns with an
/// explicit `malloc_trim`; no allocator does it unprompted).
fn reclaim_policy() -> &'static crate::reclaim::ReclaimPolicy {
    static POLICY: std::sync::OnceLock<crate::reclaim::ReclaimPolicy> = std::sync::OnceLock::new();
    POLICY
        .get_or_init(|| crate::reclaim::ReclaimPolicy::new(crate::reclaim::DEFAULT_THRESHOLD_BYTES))
}

/// How turns obtain a provider. Overridable so tests can inject the scripted fake.
pub type ProviderFactory = Arc<dyn Fn(Dialect) -> Arc<dyn Provider> + Send + Sync>;

fn default_provider_factory() -> ProviderFactory {
    Arc::new(|d| Arc::from(crate::provider::for_dialect(d)))
}

/// The supervisor: the composed parts and the resident-session map.
pub struct Brain {
    pub cfg: BrainConfig,
    pub journal: Journal,
    pub custody: Arc<dyn KeyCustody>,
    pub hand_factory: Arc<dyn HandFactory>,
    pub hub: Arc<EventHub>,
    pub model_permits: Arc<Semaphore>,
    provider_factory: ProviderFactory,
    turn_permits: Arc<Semaphore>,
    sessions: Mutex<HashMap<String, mpsc::Sender<Command>>>,
}

enum Command {
    Message {
        content: Vec<ContentBlock>,
        reply: oneshot::Sender<Result<(String, u64)>>, // (turn_id, seq of the user message)
    },
    Cancel {
        reply: oneshot::Sender<Result<HeadDoc>>,
    },
    End {
        reply: oneshot::Sender<Result<HeadDoc>>,
    },
    Delete {
        reply: oneshot::Sender<Result<()>>,
    },
    Persist {
        name: String,
        path: String,
        media_type: Option<String>,
        reply: oneshot::Sender<Result<ArtifactDoc>>,
    },
    Snapshot {
        reply: oneshot::Sender<HeadDoc>,
    },
}

impl Brain {
    /// The general constructor: bring your own backends. This is the whole composition
    /// surface -- a custom substrate needs no core change.
    pub fn with_parts(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        hand_factory: Arc<dyn HandFactory>,
        provider_factory: Option<ProviderFactory>,
    ) -> Arc<Self> {
        Arc::new(Self {
            model_permits: Arc::new(Semaphore::new(cfg.max_concurrent_model_rounds)),
            turn_permits: Arc::new(Semaphore::new(cfg.max_concurrent_turns)),
            journal,
            custody,
            hand_factory,
            provider_factory: provider_factory.unwrap_or_else(default_provider_factory),
            hub: Arc::new(EventHub::new()),
            sessions: Mutex::new(HashMap::new()),
            cfg,
        })
    }

    /// The zero-setup composition: in-memory journal (NOT durable), local subprocess tools
    /// (NOT a sandbox), in-memory custody. Everything under `data_dir`.
    pub fn local(data_dir: impl Into<PathBuf>, cfg: BrainConfig) -> Result<Arc<Self>> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| BrainError::Invalid(format!("data dir: {e}")))?;
        let owner = format!("brain-{}", crate::mint_id("i", 12));
        Ok(Self::with_parts(
            cfg,
            Journal::new_memory(owner),
            Arc::new(crate::keys::PlainCustody),
            Arc::new(LocalFactory::new(data_dir)),
            None,
        ))
    }

    /// A retrievable URL for a persisted artifact, if the substrate can mint one.
    pub async fn artifact_url(&self, session_id: &str, doc: &ArtifactDoc) -> Option<String> {
        self.hand_factory
            .artifact_url(session_id, &doc.location)
            .await
    }

    fn hand_spec(session_id: &str, prefix: &PrefixDoc, manifest_digest: &str) -> HandSpec {
        HandSpec {
            session_id: session_id.to_string(),
            hand_enabled: prefix.hand_enabled,
            shape: prefix.shape.clone(),
            env: prefix.env.clone(),
            manifest_digest: manifest_digest.to_string(),
        }
    }

    async fn open_adapter(&self, session_id: &str, doc: &HeadDoc) -> Result<Arc<dyn HandAdapter>> {
        let spec = Self::hand_spec(session_id, &doc.prefix, &doc.manifest_digest);
        self.hand_factory.open(&spec, doc.hand_state.clone()).await
    }

    // -- create ------------------------------------------------------------------------------

    pub async fn create_session(
        self: &Arc<Self>,
        req: CreateSessionRequest,
    ) -> Result<session::Session> {
        let session_id = crate::mint_id("ses", 24);

        // Validate and resolve the sealed configuration.
        let provider = provider_name(&req.model.provider);
        let base_url = resolve_base_url(&req.model.provider, req.model.base_url.as_deref())?;
        if req.model.api_key.is_empty() {
            return Err(BrainError::Invalid(
                "model.api_key must not be empty".into(),
            ));
        }
        if req.metadata.len() > 16 {
            return Err(BrainError::Invalid("metadata: at most 16 pairs".into()));
        }
        let tools_cfg = req.tools.clone().unwrap_or_default();
        crate::tools::reject_mcp(&tools_cfg.mcp)?;
        let builtins = tools_cfg
            .builtin
            .clone()
            .unwrap_or_else(crate::tools::default_builtins);
        let decls = crate::tools::resolve(&builtins)?;
        let hand_cfg = req.hand.clone().unwrap_or_default();
        let shape = match hand_cfg.shape {
            None => "1gb".to_string(),
            Some(s) => format!("{s}"),
        };

        // Decode seed files; staging is the adapter's business (S3, local disk, yours).
        let mut seed_bytes = Vec::with_capacity(req.files.len());
        for (i, f) in req.files.iter().enumerate() {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&f.content_base64)
                .map_err(|e| BrainError::Invalid(format!("files[{i}].content_base64: {e}")))?;
            if bytes.len() > 1024 * 1024 {
                return Err(BrainError::Invalid(format!("files[{i}] exceeds 1 MiB")));
            }
            bytes_check_path(&f.path)?;
            seed_bytes.push((f.path.clone(), bytes, f.mode));
        }

        // Encrypt the BYOK key; the plaintext never reaches the journal.
        let key = ProviderKey::new(req.model.api_key.to_string());
        let blob = self.custody.encrypt(&session_id, &key).await?;

        let now = crate::wall_ms();
        let prefix = PrefixDoc {
            system_prompt: req.system_prompt.clone(),
            provider: provider.to_string(),
            model: req.model.name.to_string(),
            base_url: Some(base_url),
            max_output_tokens: req.model.max_output_tokens.map(|n| n.get()),
            temperature: req.model.temperature,
            reasoning_effort: req
                .model
                .reasoning_effort
                .as_ref()
                .map(|r| format!("{r:?}").to_lowercase()),
            tools: crate::tools::names(&decls),
            hand_enabled: hand_cfg.enabled,
            shape: shape.clone(),
            sync_interval_seconds: hand_cfg.sync_interval_seconds.max(60) as u64,
            env: hand_cfg.env.clone(),
            metadata: req.metadata.clone(),
        };
        let manifest_digest = crate::tools::manifest_digest();

        // The adapter stages seeds and validates the spec (an unsupported shape is refused
        // HERE, loudly, before anything is journaled).
        let spec = Self::hand_spec(&session_id, &prefix, &manifest_digest);
        let seeds: Vec<SeedFile<'_>> = seed_bytes
            .iter()
            .map(|(path, bytes, mode)| SeedFile {
                path,
                bytes,
                mode: *mode,
            })
            .collect();
        let hand_state = self.hand_factory.create(&spec, &seeds).await?;

        let doc = HeadDoc {
            state: "idle".into(),
            failure: None,
            turn: None,
            turns: 0,
            created_ms: now,
            updated_ms: now,
            last_message_ms: None,
            ended: false,
            prefix,
            key_b64: blob_to_b64(&blob),
            manifest_digest,
            hand_info: HeadDoc::initial_hand_info(&shape),
            hand_state,
            workspace_bytes: 0,
            artifacts: Vec::new(),
        };
        self.journal
            .create(
                &session_id,
                &doc,
                &Record::State {
                    state: "idle".into(),
                    turn: None,
                },
            )
            .await?;

        // Eager hand creation (D16): the actor starts now and readies the substrate without
        // the caller waiting.
        self.spawn_actor(&session_id, true).await;

        Ok(session_doc(&session_id, &doc))
    }

    // -- routing to actors -------------------------------------------------------------------

    async fn sender(&self, session_id: &str) -> Result<mpsc::Sender<Command>> {
        if let Some(tx) = self
            .sessions
            .lock()
            .expect("sessions lock")
            .get(session_id)
            .cloned()
            && !tx.is_closed()
        {
            return Ok(tx);
        }
        Err(BrainError::NoSuchSession(session_id.into()))
    }

    async fn sender_or_spawn(self: &Arc<Self>, session_id: &str) -> Result<mpsc::Sender<Command>> {
        if let Ok(tx) = self.sender(session_id).await {
            return Ok(tx);
        }
        // Not resident: prove the session exists before spawning.
        let head = self.journal.get_head(session_id).await?;
        if head.doc.state == "deleted" {
            return Err(BrainError::SessionDeleted(session_id.into()));
        }
        Ok(self.spawn_actor(session_id, false).await)
    }

    async fn spawn_actor(
        self: &Arc<Self>,
        session_id: &str,
        freshly_created: bool,
    ) -> mpsc::Sender<Command> {
        let (tx, rx) = mpsc::channel(16);
        {
            let mut map = self.sessions.lock().expect("sessions lock");
            map.insert(session_id.to_string(), tx.clone());
        }
        let brain = self.clone();
        let sid = session_id.to_string();
        tokio::spawn(async move {
            actor(brain.clone(), sid.clone(), rx, freshly_created).await;
            let mut map = brain.sessions.lock().expect("sessions lock");
            if let Some(cur) = map.get(&sid)
                && cur.is_closed()
            {
                map.remove(&sid);
            }
        });
        tx
    }

    pub async fn message(
        self: &Arc<Self>,
        session_id: &str,
        content: MessageRequestContent,
    ) -> Result<(String, u64)> {
        let blocks = content_blocks(content)?;
        let tx = self.sender_or_spawn(session_id).await?;
        let (reply, rx) = oneshot::channel();
        tx.send(Command::Message {
            content: blocks,
            reply,
        })
        .await
        .map_err(|_| BrainError::NoSuchSession(session_id.into()))?;
        rx.await
            .map_err(|_| BrainError::Journal("actor dropped the reply".into()))?
    }

    pub async fn cancel(self: &Arc<Self>, session_id: &str) -> Result<session::Session> {
        let tx = self.sender_or_spawn(session_id).await?;
        let (reply, rx) = oneshot::channel();
        tx.send(Command::Cancel { reply })
            .await
            .map_err(|_| BrainError::NoSuchSession(session_id.into()))?;
        let doc = rx
            .await
            .map_err(|_| BrainError::Journal("actor dropped the reply".into()))??;
        Ok(session_doc(session_id, &doc))
    }

    pub async fn end(self: &Arc<Self>, session_id: &str) -> Result<session::Session> {
        let tx = self.sender_or_spawn(session_id).await?;
        let (reply, rx) = oneshot::channel();
        tx.send(Command::End { reply })
            .await
            .map_err(|_| BrainError::NoSuchSession(session_id.into()))?;
        let doc = rx
            .await
            .map_err(|_| BrainError::Journal("actor dropped the reply".into()))??;
        Ok(session_doc(session_id, &doc))
    }

    pub async fn delete(self: &Arc<Self>, session_id: &str) -> Result<()> {
        let tx = self.sender_or_spawn(session_id).await?;
        let (reply, rx) = oneshot::channel();
        tx.send(Command::Delete { reply })
            .await
            .map_err(|_| BrainError::NoSuchSession(session_id.into()))?;
        rx.await
            .map_err(|_| BrainError::Journal("actor dropped the reply".into()))?
    }

    pub async fn persist(
        self: &Arc<Self>,
        session_id: &str,
        name: String,
        path: String,
        media_type: Option<String>,
    ) -> Result<ArtifactDoc> {
        let tx = self.sender_or_spawn(session_id).await?;
        let (reply, rx) = oneshot::channel();
        tx.send(Command::Persist {
            name,
            path,
            media_type,
            reply,
        })
        .await
        .map_err(|_| BrainError::NoSuchSession(session_id.into()))?;
        rx.await
            .map_err(|_| BrainError::Journal("actor dropped the reply".into()))?
    }

    /// GET never hydrates: a resident actor answers from memory, otherwise the head is read
    /// straight from the journal.
    pub async fn get(self: &Arc<Self>, session_id: &str) -> Result<session::Session> {
        if let Ok(tx) = self.sender(session_id).await {
            let (reply, rx) = oneshot::channel();
            if tx.send(Command::Snapshot { reply }).await.is_ok()
                && let Ok(doc) = rx.await
            {
                return Ok(session_doc(session_id, &doc));
            }
        }
        let head = self.journal.get_head(session_id).await?;
        if head.doc.state == "deleted" {
            return Err(BrainError::NoSuchSession(session_id.into()));
        }
        Ok(session_doc(session_id, &head.doc))
    }

    pub async fn list(self: &Arc<Self>, limit: usize) -> Result<Vec<session::Session>> {
        let heads = self.journal.list_sessions(limit).await?;
        Ok(heads
            .iter()
            .filter(|h| h.doc.state != "deleted")
            .map(|h| session_doc(&h.session_id, &h.doc))
            .collect())
    }

    pub async fn head(&self, session_id: &str) -> Result<Head> {
        self.journal.get_head(session_id).await
    }
}

fn bytes_check_path(path: &str) -> Result<()> {
    if path.is_empty() || path.contains("..") {
        return Err(BrainError::Invalid(format!(
            "files path {path:?} is not allowed"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The actor
// ---------------------------------------------------------------------------------------------

struct Resident {
    st: TurnState,
    key: ProviderKey,
}

async fn actor(
    brain: Arc<Brain>,
    session_id: String,
    mut rx: mpsc::Receiver<Command>,
    eager_hand: bool,
) {
    let mut resident: Option<Resident> = None;
    #[allow(clippy::type_complexity)]
    let mut running: Option<(
        tokio::task::JoinHandle<(TurnState, Result<crate::turn::TurnReport>)>,
        CancellationToken,
        String,
        ProviderKey,
    )> = None;

    if eager_hand {
        // Eager hand creation, D16: ready the substrate without any caller waiting.
        match hydrate(&brain, &session_id).await {
            Ok(mut r) => {
                match r.st.hand.ensure_ready().await {
                    Ok(_) => {
                        let seq = r.st.next_seq;
                        r.st.next_seq += 1;
                        let rec = Record::State {
                            state: r.st.head.state.clone(),
                            turn: None,
                        };
                        if commit(&brain, &session_id, &mut r.st, vec![(seq, rec)])
                            .await
                            .is_err()
                        {
                            tracing::warn!(session = %session_id, "eager-hand commit failed");
                        }
                        // Same idle rule as turn end: nothing held open between turns.
                        r.st.hand.idle();
                    }
                    Err(e) => {
                        tracing::warn!(session = %session_id, error = %e, "eager hand launch failed");
                        // Not fatal at create: the first tool call retries and the failure
                        // then lands on the turn.
                    }
                }
                resident = Some(r);
            }
            Err(e) => tracing::warn!(session = %session_id, error = %e, "eager hydrate failed"),
        }
    }

    loop {
        tokio::select! {
            done = async { (&mut running.as_mut().expect("guarded").0).await }, if running.is_some() => {
                let (_, _, turn_id, key) = running.take().expect("running");
                match done {
                    Ok((st, outcome)) => {
                        let mut r = Resident { st, key };
                        finish_turn(&brain, &session_id, &turn_id, &mut r.st, outcome).await;
                        resident = Some(r);
                    }
                    Err(join_err) => {
                        tracing::error!(session = %session_id, error = %join_err, "turn task panicked");
                        // The fold moved into the task and is gone; discard and rehydrate on
                        // the next message. The journal has everything up to the last commit.
                        resident = None;
                    }
                }
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    Command::Snapshot { reply } => {
                        let doc = match &resident {
                            Some(r) => r.st.head.clone(),
                            None => match brain.journal.get_head(&session_id).await {
                                Ok(h) => h.doc,
                                Err(_) => { drop(reply); continue; }
                            },
                        };
                        let _ = reply.send(doc);
                    }
                    Command::Message { content, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let r = match ensure_resident(&brain, &session_id, &mut resident).await {
                            Ok(r) => r,
                            Err(e) => { let _ = reply.send(Err(e)); continue; }
                        };
                        let permit = match brain.turn_permits.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => { let _ = reply.send(Err(BrainError::Overloaded)); continue; }
                        };
                        match admit(&brain, &session_id, r, content).await {
                            Ok((turn_id, seq, cancel)) => {
                                let _ = reply.send(Ok((turn_id.clone(), seq)));
                                // Park the resident state into the turn task; the key rides
                                // the running tuple until the task returns the state.
                                let mut parked = resident.take().expect("resident");
                                let run = match turn_run(&brain, &session_id, &turn_id, &parked, cancel.clone()) {
                                    Ok(run) => run,
                                    Err(e) => {
                                        let _ = fail_turn_now(&brain, &session_id, &turn_id, &mut parked.st, &e).await;
                                        resident = Some(parked);
                                        continue;
                                    }
                                };
                                let key = parked.key.clone();
                                let handle = tokio::spawn(async move {
                                    let _permit = permit; // held for the whole turn (admission)
                                    let mut st = parked.st;
                                    let out = run.run(&mut st).await;
                                    (st, out)
                                });
                                running = Some((handle, cancel, turn_id, key));
                            }
                            Err(e) => { let _ = reply.send(Err(e)); }
                        }
                    }
                    Command::Cancel { reply } => {
                        if let Some((_, cancel, _, _)) = &running {
                            cancel.cancel();
                        }
                        let doc = match &resident {
                            Some(r) => r.st.head.clone(),
                            None => match brain.journal.get_head(&session_id).await {
                                Ok(h) => h.doc,
                                Err(e) => { let _ = reply.send(Err(e)); continue; }
                            },
                        };
                        let _ = reply.send(Ok(doc));
                    }
                    Command::End { reply } => {
                        if let Some((handle, cancel, turn_id, key)) = running.take() {
                            cancel.cancel();
                            if let Ok((st, outcome)) = handle.await {
                                let mut r = Resident { st, key };
                                finish_turn(&brain, &session_id, &turn_id, &mut r.st, outcome).await;
                                resident = Some(r);
                            }
                        }
                        match end_session(&brain, &session_id, &mut resident).await {
                            Ok(doc) => { let _ = reply.send(Ok(doc)); }
                            Err(e) => { let _ = reply.send(Err(e)); }
                        }
                    }
                    Command::Delete { reply } => {
                        if let Some((handle, cancel, _, _)) = running.take() {
                            cancel.cancel();
                            let _ = handle.await;
                            resident = None;
                        }
                        let out = delete_session(&brain, &session_id, &mut resident).await;
                        let _ = reply.send(out);
                        break; // the actor is done; the session is gone
                    }
                    Command::Persist { name, path, media_type, reply } => {
                        if running.is_some() {
                            let _ = reply.send(Err(BrainError::TurnInFlight(session_id.clone())));
                            continue;
                        }
                        let out = do_persist(&brain, &session_id, &mut resident, name, path, media_type).await;
                        let _ = reply.send(out);
                    }
                }
            }
            _ = tokio::time::sleep(brain.cfg.idle_discard), if running.is_none() => {
                // Idle: discard the fold, release the lease, drop the adapter. The substrate
                // does its own idling; the journal holds everything.
                if let Some(r) = resident.take() {
                    r.st.hand.idle();
                    let _ = brain.journal.release(&session_id, &r.st.lease).await;
                    let freed: usize = r.st.history.iter().map(|m| m.heap_bytes()).sum();
                    drop(r);
                    // PD-13: no allocator returns memory on drop without an explicit trim.
                    // The policy batches trims so a burst of discards pays one stall.
                    if reclaim_policy().freed(freed as u64).is_some() {
                        tracing::debug!(freed, "malloc_trim after session drop");
                    }
                }
                break;
            }
        }
    }
    tracing::debug!(session = %session_id, "actor exited");
}

async fn ensure_resident<'a>(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &'a mut Option<Resident>,
) -> Result<&'a mut Resident> {
    if resident.is_none() {
        *resident = Some(hydrate(brain, session_id).await?);
    }
    Ok(resident.as_mut().expect("just set"))
}

/// Rebuilds a resident session from the journal: claim -> read -> fold -> decrypt -> open
/// the adapter from its persisted state.
async fn hydrate(brain: &Arc<Brain>, session_id: &str) -> Result<Resident> {
    let head = brain.journal.claim(session_id).await?;
    if head.doc.state == "deleted" {
        return Err(BrainError::SessionDeleted(session_id.into()));
    }
    let entries = brain.journal.read_records(session_id, 0).await?;
    let fold = crate::journal::fold(&entries);
    let key = brain
        .custody
        .decrypt(session_id, &blob_from_b64(&head.doc.key_b64)?)
        .await?;
    let hand = brain.open_adapter(session_id, &head.doc).await?;
    Ok(Resident {
        st: TurnState {
            history: fold.history,
            hand,
            head: head.doc,
            lease: Lease {
                fence: head.fence,
                last_seq: head.last_seq,
            },
            todo: Arc::new(TodoState::default()),
            next_seq: head.last_seq + 1,
        },
        key,
    })
}

async fn commit(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    records: Vec<(u64, Record)>,
) -> Result<()> {
    st.snapshot_hand();
    st.head.updated_ms = crate::wall_ms();
    let high_water = st.next_seq - 1;
    let mut lease = st.lease.clone();
    brain
        .journal
        .commit(session_id, &mut lease, &records, &st.head, high_water)
        .await?;
    st.lease = lease;
    let now = crate::wall_ms();
    for (seq, record) in &records {
        if let Some(e) = crate::events::derive(session_id, *seq, now, record, &st.head.hand_info) {
            brain.hub.publish(session_id, e);
        }
    }
    Ok(())
}

/// Admits one message: journals the decision, pokes the adapter, hands back the turn
/// identity. 202 semantics: the reply happens after this commit succeeds.
async fn admit(
    brain: &Arc<Brain>,
    session_id: &str,
    r: &mut Resident,
    content: Vec<ContentBlock>,
) -> Result<(String, u64, CancellationToken)> {
    if r.st.head.state == "failed" {
        return Err(BrainError::SessionFailed(
            r.st.head
                .failure
                .as_ref()
                .map(|f| f.message.clone())
                .unwrap_or_default(),
        ));
    }
    // The substrate may demand a release before more work (e.g. a platform lifetime wall):
    // release now; the turn's first call re-materialises through ensure_ready.
    if r.st.hand.must_release() {
        tracing::info!(session = %session_id, "substrate wall: releasing before the turn");
        let _ = r.st.hand.release().await;
        let seq = r.st.next_seq;
        r.st.next_seq += 1;
        let rec = Record::State {
            state: r.st.head.state.clone(),
            turn: None,
        };
        commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;
    }

    let turn_id = crate::mint_id("trn", 24);
    let user_seq = r.st.next_seq;
    r.st.next_seq += 1;
    let started_seq = r.st.next_seq;
    r.st.next_seq += 1;
    r.st.head.state = "active".into();
    r.st.head.turn = Some(turn_id.clone());
    r.st.head.turns += 1;
    r.st.head.last_message_ms = Some(crate::wall_ms());
    let records = vec![
        (
            user_seq,
            Record::UserMessage {
                turn: turn_id.clone(),
                content: content.clone(),
            },
        ),
        (
            started_seq,
            Record::TurnStarted {
                turn: turn_id.clone(),
            },
        ),
    ];
    commit(brain, session_id, &mut r.st, records).await?;
    r.st.history.push(Message {
        role: crate::message::Role::User,
        content,
    });

    // e.g. the speculative resume (F-4): substrate traffic now, hidden behind the model round.
    r.st.hand.on_message_admitted();

    Ok((turn_id, user_seq, CancellationToken::new()))
}

fn turn_run(
    brain: &Arc<Brain>,
    session_id: &str,
    turn_id: &str,
    r: &Resident,
    cancel: CancellationToken,
) -> Result<TurnRun> {
    let (prefix, dialect) = build_prefix(&r.st.head.prefix)?;
    let base_url = r.st.head.prefix.base_url.clone().unwrap_or_default();
    let session = SessionConfig::new(prefix.clone(), r.key.clone(), base_url);
    Ok(TurnRun {
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        prefix,
        session,
        provider: (brain.provider_factory)(dialect),
        provider_name: r.st.head.prefix.provider.clone(),
        journal: brain.journal.clone(),
        hub: brain.hub.clone(),
        cancel,
        model_permits: brain.model_permits.clone(),
        history_budget_bytes: brain.cfg.history_budget_bytes,
    })
}

/// Applies the turn outcome that `TurnRun::run` could not commit itself (failures), then the
/// turn-end checkpoint (the workspace durability point, D7).
async fn finish_turn(
    brain: &Arc<Brain>,
    session_id: &str,
    turn_id: &str,
    st: &mut TurnState,
    outcome: Result<crate::turn::TurnReport>,
) {
    match outcome {
        Ok(report) => {
            tracing::info!(session = %session_id, turn = %turn_id, stop = %report.stop_reason, rounds = report.rounds, "turn done");
        }
        Err(e) => {
            let _ = fail_turn_now(brain, session_id, turn_id, st, &e).await;
        }
    }
    match st.hand.checkpoint().await {
        Ok(()) => {
            if let Err(e) = commit(brain, session_id, st, vec![]).await {
                tracing::warn!(session = %session_id, error = %e, "checkpoint commit failed");
            }
        }
        Err(e) => tracing::warn!(session = %session_id, error = %e, "turn-end checkpoint failed"),
    }
    // Nothing held open between turns: the substrate idles/suspends on its own clock.
    st.hand.idle();
}

async fn fail_turn_now(
    brain: &Arc<Brain>,
    session_id: &str,
    turn_id: &str,
    st: &mut TurnState,
    e: &BrainError,
) -> Result<()> {
    tracing::warn!(session = %session_id, turn = %turn_id, error = %e, "turn failed");
    let (code, session_fatal) = match e {
        BrainError::ProviderStatus { .. } | BrainError::Transport(_) | BrainError::Protocol(_) => {
            ("provider_error", false)
        }
        BrainError::HandUnavailable(_) => ("hand_unavailable", false),
        BrainError::SessionFailed(_) => ("session_failed", true),
        BrainError::Fenced => return Ok(()), // a newer owner exists; nothing to write
        _ => ("internal", false),
    };
    let failed_seq = st.next_seq;
    st.next_seq += 1;
    let state_seq = st.next_seq;
    st.next_seq += 1;
    if session_fatal {
        st.head.state = "failed".into();
        st.head.failure = Some(FailureDoc {
            code: "tool_manifest_mismatch".into(),
            message: e.to_string(),
            at_ms: crate::wall_ms(),
        });
    } else {
        st.head.state = "idle".into();
    }
    st.head.turn = None;
    let records = vec![
        (
            failed_seq,
            Record::TurnFailed {
                turn: turn_id.to_string(),
                code: code.into(),
                message: e.to_string(),
            },
        ),
        (
            state_seq,
            Record::State {
                state: st.head.state.clone(),
                turn: Some(turn_id.to_string()),
            },
        ),
    ];
    commit(brain, session_id, st, records).await
}

async fn end_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<HeadDoc> {
    let r = ensure_resident(brain, session_id, resident).await?;
    if !r.st.head.ended {
        // Checkpoint + release compute; keep the workspace, keep the journal. End is an
        // action, not a state.
        if let Err(e) = r.st.hand.checkpoint().await {
            tracing::warn!(session = %session_id, error = %e, "end checkpoint failed");
        }
        r.st.hand.release().await?;
        r.st.head.ended = true;
        r.st.head.state = "idle".into();
        let seq = r.st.next_seq;
        r.st.next_seq += 1;
        let rec = Record::State {
            state: "idle".into(),
            turn: None,
        };
        commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;
    }
    Ok(r.st.head.clone())
}

async fn delete_session(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
) -> Result<()> {
    let r = ensure_resident(brain, session_id, resident).await?;
    // Release compute without a checkpoint: the workspace is about to be deleted anyway.
    let _ = r.st.hand.release().await;
    r.st.head.state = "deleted".into();
    let seq = r.st.next_seq;
    r.st.next_seq += 1;
    let rec = Record::State {
        state: "deleted".into(),
        turn: None,
    };
    commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;

    // Purge storage (the adapter's), then the journal items. The state=deleted commit above
    // is the irreversible line; purge is cleanup.
    if let Err(e) = brain.hand_factory.purge(session_id).await {
        tracing::warn!(session = %session_id, error = %e, "substrate purge incomplete");
    }
    brain.journal.purge(session_id).await?;
    brain.hub.drop_session(session_id);
    *resident = None;
    Ok(())
}

async fn do_persist(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    name: String,
    path: String,
    media_type: Option<String>,
) -> Result<ArtifactDoc> {
    let r = ensure_resident(brain, session_id, resident).await?;
    r.st.hand.ensure_ready().await?;
    let meta =
        r.st.hand
            .persist(&name, &path, media_type.as_deref())
            .await?;
    let doc = ArtifactDoc {
        name: name.clone(),
        location: meta.location,
        bytes: meta.bytes,
        sha256: meta.sha256,
        media_type: meta.media_type,
        created_ms: crate::wall_ms(),
    };
    r.st.head.artifacts.retain(|a| a.name != name);
    r.st.head.artifacts.push(doc.clone());
    commit(brain, session_id, &mut r.st, vec![]).await?;
    // Same idle rule as turn end: nothing held open while nothing is running.
    r.st.hand.idle();
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// Mapping helpers
// ---------------------------------------------------------------------------------------------

pub fn provider_name(p: &ApiProvider) -> &'static str {
    match p {
        ApiProvider::Openai => "openai",
        ApiProvider::Anthropic => "anthropic",
        ApiProvider::Deepseek => "deepseek",
        ApiProvider::Moonshot => "moonshot",
        ApiProvider::Xai => "xai",
        ApiProvider::OpenaiCompatible => "openai_compatible",
    }
}

/// Certified: openai, anthropic. Available uncertified: the rest (they speak one of the two
/// dialects). `openai_compatible` requires an explicit base_url.
pub fn resolve_base_url(p: &ApiProvider, base_url: Option<&str>) -> Result<String> {
    if let Some(u) = base_url {
        if !u.starts_with("https://") {
            return Err(BrainError::Invalid("model.base_url must be https".into()));
        }
        return Ok(u.trim_end_matches('/').to_string());
    }
    Ok(match p {
        ApiProvider::Openai => "https://api.openai.com".into(),
        ApiProvider::Anthropic => "https://api.anthropic.com".into(),
        ApiProvider::Deepseek => "https://api.deepseek.com".into(),
        ApiProvider::Moonshot => "https://api.moonshot.ai".into(),
        ApiProvider::Xai => "https://api.x.ai".into(),
        ApiProvider::OpenaiCompatible => {
            return Err(BrainError::Invalid(
                "model.base_url is required for provider openai_compatible".into(),
            ));
        }
    })
}

pub fn dialect_of(provider: &str) -> Dialect {
    match provider {
        "anthropic" => Dialect::AnthropicMessages,
        _ => Dialect::OpenAiChat,
    }
}

/// Rebuilds the sealed prefix from the HEAD prefix doc. Deterministic: the same doc always
/// seals to the same digest.
pub fn build_prefix(
    p: &PrefixDoc,
) -> Result<(crate::Shared<crate::config::SealedPrefix>, Dialect)> {
    let dialect = dialect_of(&p.provider);
    let builtins: Vec<_> = p
        .tools
        .iter()
        .filter_map(|n| crate::tools::parse_builtin(n))
        .collect();
    let decls = crate::tools::resolve(&builtins)?;
    let mut def = AgentDef::new(
        p.system_prompt
            .clone()
            .unwrap_or_else(default_system_prompt),
        p.model.clone(),
        dialect,
    );
    for d in decls {
        def = def.tool(d);
    }
    def = def.sampling(GenOpts {
        max_tokens: p.max_output_tokens.unwrap_or(4096) as u32,
        temperature: p.temperature.map(|t| t as f32),
        stop_sequences: Vec::new(),
    });
    Ok((def.seal(), dialect))
}

fn default_system_prompt() -> String {
    "You are an autonomous engineering agent running in an isolated Linux workspace \
     (/workspace, ARM64). Use the tools to inspect and change files and run commands. \
     Software installed system-wide is ephemeral; anything under /workspace and your home \
     directory persists for the session's life."
        .to_string()
}

/// Builds the contract Session document from the head.
pub fn session_doc(session_id: &str, doc: &HeadDoc) -> session::Session {
    session::Session {
        created_at: crate::events::ts(doc.created_ms),
        current_turn: doc.turn.as_deref().and_then(|t| t.parse().ok()),
        failure: doc.failure.as_ref().map(|f| session::SessionFailure {
            at: crate::events::ts(f.at_ms),
            code: match f.code.as_str() {
                "tool_manifest_mismatch" => session::SessionFailureCode::ToolManifestMismatch,
                "provider_unusable" => session::SessionFailureCode::ProviderUnusable,
                "hand_unavailable" => session::SessionFailureCode::HandUnavailable,
                _ => session::SessionFailureCode::Internal,
            },
            message: f.message.clone(),
        }),
        hand: doc.hand_info.clone(),
        id: session_id
            .parse()
            .unwrap_or_else(|_| "ses_00000000000000000000".parse().expect("fallback id")),
        last_message_at: doc.last_message_ms.map(crate::events::ts),
        metadata: doc.prefix.metadata.clone(),
        model: session::ModelInfo {
            base_url: doc.prefix.base_url.clone(),
            name: doc.prefix.model.clone(),
            provider: match doc.prefix.provider.as_str() {
                "openai" => ApiProvider::Openai,
                "anthropic" => ApiProvider::Anthropic,
                "deepseek" => ApiProvider::Deepseek,
                "moonshot" => ApiProvider::Moonshot,
                "xai" => ApiProvider::Xai,
                _ => ApiProvider::OpenaiCompatible,
            },
        },
        object: session::SessionObject::Session,
        state: crate::events::session_state(&doc.state),
        storage: session::StorageInfo {
            artifact_bytes: doc.artifacts.iter().map(|a| a.bytes).sum(),
            // Suspended-memory metering lands with billing (slice 4); until then this is an
            // honest zero: nothing is billed off it.
            suspended_bytes: 0,
            workspace_bytes: doc.workspace_bytes,
        },
        turns: doc.turns,
        updated_at: crate::events::ts(doc.updated_ms),
    }
}

fn content_blocks(content: MessageRequestContent) -> Result<Vec<ContentBlock>> {
    match content {
        MessageRequestContent::String(s) => {
            let s: String = s.to_string();
            if s.is_empty() {
                return Err(BrainError::Invalid("content must not be empty".into()));
            }
            Ok(vec![ContentBlock::text(s)])
        }
        MessageRequestContent::Array(parts) => {
            if parts.is_empty() {
                return Err(BrainError::Invalid("content must not be empty".into()));
            }
            let mut blocks = Vec::with_capacity(parts.len());
            for p in parts {
                match p {
                    session::ContentPart::Text { text } => blocks.push(ContentBlock::text(text)),
                    session::ContentPart::WorkspaceFile { .. } => {
                        return Err(BrainError::Invalid(
                            "workspace_file content parts are not available yet (M1)".into(),
                        ));
                    }
                }
            }
            Ok(blocks)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_urls_resolve_and_compatible_requires_one() {
        assert_eq!(
            resolve_base_url(&ApiProvider::Anthropic, None).unwrap(),
            "https://api.anthropic.com"
        );
        assert_eq!(
            resolve_base_url(&ApiProvider::Deepseek, None).unwrap(),
            "https://api.deepseek.com"
        );
        assert!(resolve_base_url(&ApiProvider::OpenaiCompatible, None).is_err());
        assert!(resolve_base_url(&ApiProvider::Openai, Some("http://insecure")).is_err());
        assert_eq!(
            resolve_base_url(&ApiProvider::Openai, Some("https://proxy.example/")).unwrap(),
            "https://proxy.example"
        );
    }

    #[test]
    fn prefix_rebuild_is_deterministic() {
        let p = PrefixDoc {
            system_prompt: Some("sp".into()),
            provider: "anthropic".into(),
            model: "claude-x".into(),
            base_url: Some("https://api.anthropic.com".into()),
            max_output_tokens: Some(2048),
            temperature: Some(0.5),
            reasoning_effort: None,
            tools: vec!["bash".into(), "todo".into()],
            hand_enabled: true,
            shape: "1gb".into(),
            sync_interval_seconds: 600,
            env: HashMap::new(),
            metadata: HashMap::new(),
        };
        let (a, da) = build_prefix(&p).unwrap();
        let (b, db) = build_prefix(&p).unwrap();
        assert_eq!(a.digest(), b.digest());
        assert_eq!(da, db);
        assert_eq!(a.tools.len(), 2);
    }

    #[test]
    fn dialects_route_by_provider() {
        assert_eq!(dialect_of("anthropic"), Dialect::AnthropicMessages);
        assert_eq!(dialect_of("deepseek"), Dialect::OpenAiChat);
        assert_eq!(dialect_of("openai"), Dialect::OpenAiChat);
    }
}
