//! Sessions as spawned tasks (D9): one actor per resident session, hydrate-act-commit-discard.
//!
//! An idle session is nothing but its journal. The actor holds the cached fold (history,
//! head, lease, hand connection); after `idle_discard` without traffic it releases the lease,
//! drops the connection and exits -- the next message hydrates from the journal (PD-11
//! measured the rehydrate at constant ~4 ms). Everything the actor holds is rebuildable;
//! everything durable went through `Journal::commit` first.

use crate::compact::DEFAULT_HISTORY_BUDGET_BYTES;
use crate::config::{AgentDef, Dialect, GenOpts, ProviderKey, SessionConfig};
use crate::events::EventHub;
use crate::hand::{HandPlane, HandPlaneConfig, HandRuntime, SessionHand, hand_info};
use crate::journal::{
    ArtifactDoc, FailureDoc, HandDoc, Head, HeadDoc, Journal, Lease, PrefixDoc, Record,
    SeedFileDoc, SyncDoc,
};
use crate::keys::{KeyCustody, blob_from_b64, blob_to_b64};
use crate::local::LocalHand;
use crate::message::{ContentBlock, Message};
use crate::provider::Provider;
use crate::tools::TodoState;
use crate::turn::{TurnRun, TurnState};
use crate::{BrainError, Result};
use aex_contracts::abi::SyncReason;
use aex_contracts::session::{
    self, CreateSessionRequest, MessageRequestContent, Provider as ApiProvider,
};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// Which backends this process runs on.
#[derive(Debug, Clone)]
pub enum ModeConfig {
    /// The product path: DynamoDB journal, KMS custody, Lambda MicroVM hands, S3 storage.
    Aws {
        journal_table: String,
        kms_key_id: String,
        hand: HandPlaneConfig,
    },
    /// The zero-setup default: in-memory journal (NOT durable), local tool execution
    /// (process separation, NOT a sandbox), per-session directories under `data_dir`.
    Local { data_dir: PathBuf },
}

/// Process configuration.
#[derive(Debug, Clone)]
pub struct BrainConfig {
    pub mode: ModeConfig,
    /// Admission: concurrent model rounds across the process.
    pub max_concurrent_model_rounds: usize,
    /// Admission: concurrent active turns across the process.
    pub max_concurrent_turns: usize,
    /// Idle residency before the actor discards its fold and exits.
    pub idle_discard: Duration,
    pub history_budget_bytes: usize,
}

impl BrainConfig {
    /// `AEX_MODE=local` (the default) or `AEX_MODE=aws`. Local needs nothing; aws fails fast
    /// on any missing backend variable -- production is configured, never guessed.
    pub fn from_env() -> Result<Self> {
        let get =
            |k: &str| std::env::var(k).map_err(|_| BrainError::Invalid(format!("{k} is not set")));
        let mode = match std::env::var("AEX_MODE").as_deref() {
            Err(_) | Ok("local") => ModeConfig::Local {
                data_dir: PathBuf::from(
                    std::env::var("AEX_DATA_DIR").unwrap_or_else(|_| "./aex-data".into()),
                ),
            },
            Ok("aws") => ModeConfig::Aws {
                journal_table: get("AEX_JOURNAL_TABLE")?,
                kms_key_id: get("AEX_KMS_KEY_ID")?,
                hand: HandPlaneConfig::from_env()?,
            },
            Ok(other) => {
                return Err(BrainError::Invalid(format!(
                    "AEX_MODE must be local or aws, got {other}"
                )));
            }
        };
        Ok(Self {
            mode,
            max_concurrent_model_rounds: env_num("AEX_MAX_MODEL_ROUNDS", 64),
            max_concurrent_turns: env_num("AEX_MAX_TURNS", 64),
            idle_discard: Duration::from_secs(env_num("AEX_IDLE_DISCARD_SECONDS", 900) as u64),
            history_budget_bytes: DEFAULT_HISTORY_BUDGET_BYTES,
        })
    }
}

/// The resolved runtime the process actually holds.
pub enum RuntimeMode {
    Aws { plane: Arc<HandPlane> },
    Local { data_dir: PathBuf },
}

/// How turns obtain a provider. Overridable so tests can inject the scripted fake.
pub type ProviderFactory = Arc<dyn Fn(Dialect) -> Arc<dyn Provider> + Send + Sync>;

fn default_provider_factory() -> ProviderFactory {
    Arc::new(|d| Arc::from(crate::provider::for_dialect(d)))
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

/// The supervisor: owns the shared planes and the resident-session map.
pub struct Brain {
    pub cfg: BrainConfig,
    pub journal: Journal,
    pub custody: Arc<dyn KeyCustody>,
    pub runtime: RuntimeMode,
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
    pub async fn new(cfg: BrainConfig) -> Result<Arc<Self>> {
        let owner = format!("brain-{}", crate::mint_id("i", 12));
        let (journal, custody, runtime): (Journal, Arc<dyn KeyCustody>, RuntimeMode) =
            match &cfg.mode {
                ModeConfig::Aws {
                    journal_table,
                    kms_key_id,
                    hand,
                } => {
                    let aws = aws_config::from_env()
                        .region(aws_config::Region::new(hand.region.clone()))
                        .load()
                        .await;
                    (
                        Journal::new(aws_sdk_dynamodb::Client::new(&aws), journal_table, owner),
                        Arc::new(crate::keys::KmsCustody::new(
                            aws_sdk_kms::Client::new(&aws),
                            kms_key_id,
                        )),
                        RuntimeMode::Aws {
                            plane: Arc::new(HandPlane::from_env(hand.clone()).await),
                        },
                    )
                }
                ModeConfig::Local { data_dir } => {
                    std::fs::create_dir_all(data_dir)
                        .map_err(|e| BrainError::Invalid(format!("AEX_DATA_DIR: {e}")))?;
                    (
                        Journal::new_memory(owner),
                        Arc::new(crate::keys::PlainCustody),
                        RuntimeMode::Local {
                            data_dir: data_dir.clone(),
                        },
                    )
                }
            };
        Ok(Self::assemble(
            cfg,
            journal,
            custody,
            runtime,
            default_provider_factory(),
        ))
    }

    /// For tests: a brain over injected parts.
    pub fn with_parts(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        runtime: RuntimeMode,
        provider_factory: Option<ProviderFactory>,
    ) -> Arc<Self> {
        Self::assemble(
            cfg,
            journal,
            custody,
            runtime,
            provider_factory.unwrap_or_else(default_provider_factory),
        )
    }

    fn assemble(
        cfg: BrainConfig,
        journal: Journal,
        custody: Arc<dyn KeyCustody>,
        runtime: RuntimeMode,
        provider_factory: ProviderFactory,
    ) -> Arc<Self> {
        Arc::new(Self {
            model_permits: Arc::new(Semaphore::new(cfg.max_concurrent_model_rounds)),
            turn_permits: Arc::new(Semaphore::new(cfg.max_concurrent_turns)),
            journal,
            custody,
            runtime,
            provider_factory,
            hub: Arc::new(EventHub::new()),
            sessions: Mutex::new(HashMap::new()),
            cfg,
        })
    }

    /// A presigned (aws) or absent (local) download URL for an artifact.
    pub async fn artifact_url(&self, doc: &ArtifactDoc) -> Option<String> {
        match &self.runtime {
            RuntimeMode::Aws { plane } => plane.presign_get(&doc.s3_key).await.ok(),
            RuntimeMode::Local { .. } => None,
        }
    }

    fn hand_runtime(&self, session_id: &str) -> Result<HandRuntime> {
        Ok(match &self.runtime {
            RuntimeMode::Aws { plane } => {
                HandRuntime::Remote(SessionHand::new(plane.clone(), session_id.to_string()))
            }
            RuntimeMode::Local { data_dir } => {
                HandRuntime::Local(LocalHand::open(data_dir, session_id)?)
            }
        })
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
            None | Some(session::HandShape::X1gb) => "1gb".to_string(),
            Some(other) => {
                return Err(BrainError::Invalid(format!(
                    "hand.shape {other:?} is not offered yet; the dev plane runs 1gb"
                )));
            }
        };

        // Stage seed files. Aws: to S3, applied on the first hello (content never enters the
        // journal). Local: straight onto disk -- the workspace is already its durable form.
        let mut seeds = Vec::with_capacity(req.files.len());
        for (i, f) in req.files.iter().enumerate() {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&f.content_base64)
                .map_err(|e| BrainError::Invalid(format!("files[{i}].content_base64: {e}")))?;
            if bytes.len() > 1024 * 1024 {
                return Err(BrainError::Invalid(format!("files[{i}] exceeds 1 MiB")));
            }
            match &self.runtime {
                RuntimeMode::Aws { plane } => {
                    let key = crate::hand::seed_key(&session_id, i);
                    let sha = hex::encode(Sha256::digest(&bytes));
                    plane
                        .s3
                        .put_object()
                        .bucket(&plane.cfg.bucket)
                        .key(&key)
                        .body(bytes.clone().into())
                        .send()
                        .await
                        .map_err(|e| BrainError::Journal(format!("seed upload: {e}")))?;
                    seeds.push(SeedFileDoc {
                        path: f.path.clone(),
                        s3_key: key,
                        bytes: bytes.len() as u64,
                        sha256: sha,
                        mode: f.mode,
                    });
                }
                RuntimeMode::Local { data_dir } => {
                    LocalHand::open(data_dir, &session_id)?.seed(&f.path, &bytes, f.mode)?;
                }
            }
        }

        // Encrypt the BYOK key; the plaintext never reaches the journal.
        let key = ProviderKey::new(req.model.api_key.to_string());
        let blob = self.custody.encrypt(&session_id, &key).await?;

        let now = crate::wall_ms();
        let doc = HeadDoc {
            state: "idle".into(),
            failure: None,
            turn: None,
            turns: 0,
            created_ms: now,
            updated_ms: now,
            last_message_ms: None,
            ended: false,
            prefix: PrefixDoc {
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
                shape,
                sync_interval_seconds: hand_cfg.sync_interval_seconds.max(60) as u64,
                env: hand_cfg.env.clone(),
                metadata: req.metadata.clone(),
                seed_files: seeds,
            },
            key_b64: blob_to_b64(&blob),
            manifest_digest: crate::tools::manifest_digest(),
            hand: HandDoc {
                state: "preparing".into(),
                ..Default::default()
            },
            sync: SyncDoc::default(),
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

        // Eager hand creation (D16): the actor starts now and launches the hand without the
        // caller waiting.
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
        // Eager hand creation, D16: launch without any caller waiting. Hydration includes it.
        match hydrate(&brain, &session_id).await {
            Ok(mut r) => {
                let lost = r.st.hand.ensure_ready(&mut r.st.head).await;
                match lost {
                    Ok(_) => {
                        r.st.head.hand.state = "ready".into();
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
                                let parked = resident.take().expect("resident");
                                let mut parked = parked;
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
                // Idle: discard the fold, release the lease, drop the connection. The hand
                // stays up (AWS suspends it); the journal holds everything.
                if let Some(mut r) = resident.take() {
                    r.st.hand.disconnect();
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

/// Rebuilds a resident session from the journal: claim -> read -> fold -> decrypt.
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
    Ok(Resident {
        st: TurnState {
            history: fold.history,
            head: head.doc,
            lease: Lease {
                fence: head.fence,
                last_seq: head.last_seq,
            },
            hand: brain.hand_runtime(session_id)?,
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
    st.head.updated_ms = crate::wall_ms();
    let high_water = st.next_seq - 1;
    let mut lease = st.lease.clone();
    brain
        .journal
        .commit(session_id, &mut lease, &records, &st.head, high_water)
        .await?;
    st.lease = lease;
    let info = hand_info(&st.head);
    let now = crate::wall_ms();
    for (seq, record) in &records {
        if let Some(e) = crate::events::derive(session_id, *seq, now, record, &info) {
            brain.hub.publish(session_id, e);
        }
    }
    Ok(())
}

/// Admits one message: journals the decision, fires the speculative resume, hands back the
/// turn identity. 202 semantics: the reply happens after this commit succeeds.
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
    // The wall: an incarnation close to its 8 h limit is synced and released now; the turn
    // then re-materialises a fresh one on the first tool call.
    if r.st.hand.wall_due(&r.st.head) {
        tracing::info!(session = %session_id, "wall approaching: sync + release before the turn");
        let _ = r.st.hand.ensure_ready(&mut r.st.head).await;
        let _ = r.st.hand.release(&mut r.st.head, true).await;
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

    // Speculative resume (F-4): endpoint traffic now, so a suspended hand is running again
    // by the time the model asks for a tool.
    r.st.hand.speculative_resume(&r.st.head);

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
/// turn-end sync.
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
    // Turn-end sync: the durability point of the workspace (D7).
    if st.hand.is_connected() {
        match st.hand.sync(&mut st.head, SyncReason::TurnEnd).await {
            Ok(_) => {
                if let Err(e) = commit(brain, session_id, st, vec![]).await {
                    tracing::warn!(session = %session_id, error = %e, "sync head commit failed");
                }
            }
            Err(e) => tracing::warn!(session = %session_id, error = %e, "turn-end sync failed"),
        }
    }
    // Disconnect between turns: an open ABI WebSocket carries the guest's heartbeat through
    // the endpoint every few seconds, which counts as traffic and defeats the 180 s idle
    // suspend forever. Connection loss is not hand loss (I10): the VM stays up, AWS suspends
    // it when truly idle, and the next message reconnects through the speculative resume.
    st.hand.disconnect();
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
        // Sync + release the hand; keep the workspace, keep the journal. End is an action,
        // not a state. A hand that is already gone keeps its last sync as the restore point.
        let have_vm = r.st.head.hand.microvm_id.is_some();
        if have_vm {
            let _ = r.st.hand.ensure_ready(&mut r.st.head).await;
        }
        r.st.hand.release(&mut r.st.head, have_vm).await?;
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
    // Release the hand without a sync: the workspace is about to be deleted anyway.
    let _ = r.st.hand.release(&mut r.st.head, false).await;
    r.st.head.state = "deleted".into();
    let seq = r.st.next_seq;
    r.st.next_seq += 1;
    let rec = Record::State {
        state: "deleted".into(),
        turn: None,
    };
    commit(brain, session_id, &mut r.st, vec![(seq, rec)]).await?;

    // Purge storage, then the journal items. The state=deleted commit above is the
    // irreversible line; purge is cleanup.
    match &brain.runtime {
        RuntimeMode::Aws { plane } => {
            let prefix = format!("sessions/{session_id}/");
            if let Err(e) = purge_s3_prefix(plane, &prefix).await {
                tracing::warn!(session = %session_id, error = %e, "s3 purge incomplete");
            }
        }
        RuntimeMode::Local { data_dir } => LocalHand::purge(data_dir, session_id),
    }
    brain.journal.purge(session_id).await?;
    brain.hub.drop_session(session_id);
    *resident = None;
    Ok(())
}

async fn purge_s3_prefix(plane: &HandPlane, prefix: &str) -> Result<()> {
    let mut token = None;
    loop {
        let out = plane
            .s3
            .list_objects_v2()
            .bucket(&plane.cfg.bucket)
            .prefix(prefix)
            .set_continuation_token(token)
            .send()
            .await
            .map_err(|e| BrainError::Journal(format!("s3 list: {e}")))?;
        for obj in out.contents() {
            if let Some(key) = obj.key() {
                let _ = plane
                    .s3
                    .delete_object()
                    .bucket(&plane.cfg.bucket)
                    .key(key)
                    .send()
                    .await;
            }
        }
        token = out.next_continuation_token().map(str::to_owned);
        if token.is_none() {
            return Ok(());
        }
    }
}

async fn do_persist(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    name: String,
    path: String,
    media_type: Option<String>,
) -> Result<ArtifactDoc> {
    use aex_contracts::abi::{PersistItem, PersistRequest as AbiPersist, PersistSource};
    let r = ensure_resident(brain, session_id, resident).await?;
    r.st.hand.ensure_ready(&mut r.st.head).await?;

    // Local: copy into the session's artifacts directory, no URLs anywhere.
    if let Some(local) = r.st.hand.local().cloned() {
        let (bytes, sha256, target) = local.persist(&name, &path)?;
        let doc = ArtifactDoc {
            name: name.clone(),
            s3_key: target.to_string_lossy().to_string(),
            bytes,
            sha256,
            media_type: media_type.unwrap_or_else(|| "application/octet-stream".into()),
            created_ms: crate::wall_ms(),
        };
        r.st.head.artifacts.retain(|a| a.name != name);
        r.st.head.artifacts.push(doc.clone());
        commit(brain, session_id, &mut r.st, vec![]).await?;
        return Ok(doc);
    }

    let RuntimeMode::Aws { plane } = &brain.runtime else {
        return Err(BrainError::HandUnavailable("no hand runtime".into()));
    };
    let client =
        r.st.hand
            .client()
            .ok_or_else(|| BrainError::HandUnavailable("no hand".into()))?;
    let key = crate::hand::artifact_key(session_id, &name);
    let put_url = plane.presign_put(&key).await?;
    let resp = client
        .persist(AbiPersist {
            items: vec![PersistItem {
                name: name
                    .parse()
                    .map_err(|_| BrainError::Invalid("artifact name".into()))?,
                put_url,
                media_type: media_type.clone(),
                source: PersistSource::Path { path },
            }],
        })
        .await
        .map_err(|e| BrainError::Hand(format!("persist: {e}")))?;
    let item = resp
        .persisted
        .first()
        .ok_or_else(|| BrainError::Hand("persist returned no items".into()))?;
    let doc = ArtifactDoc {
        name: name.clone(),
        s3_key: key,
        bytes: item.bytes,
        sha256: (*item.sha256).to_string(),
        media_type: if item.media_type.is_empty() {
            "application/octet-stream".into()
        } else {
            item.media_type.clone()
        },
        created_ms: crate::wall_ms(),
    };
    r.st.head.artifacts.retain(|a| a.name != name);
    r.st.head.artifacts.push(doc.clone());
    commit(brain, session_id, &mut r.st, vec![]).await?;
    // Same idle rule as turn end: no open connection while nothing is running.
    r.st.hand.disconnect();
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
        hand: hand_info(doc),
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
            workspace_bytes: doc.sync.bytes_total,
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
            seed_files: vec![],
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
