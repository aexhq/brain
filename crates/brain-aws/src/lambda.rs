//! The AWS Lambda MicroVM hand: the production implementation of
//! [`brain::adapter::HandAdapter`] / [`brain::adapter::HandFactory`].
//!
//! Policy lives here; mechanics live in `hand-lambda`. The invariants this module owns:
//! - the brain mints every transfer URL (I8: the hand holds no platform credential);
//! - a connection loss is not a hand loss (I10): `diagnose` classifies, and only a VM that is
//!   truly gone becomes `hand_lost` -- in-flight calls are then `interrupted`, never replayed;
//! - the 8 h MicroVM wall is survived by syncing before the deadline (`must_release`) and
//!   re-materialising a fresh incarnation from the last manifest on the next `ensure_ready`;
//! - `hello` always carries the sealed manifest digest: a hand that cannot serve it fails the
//!   session (`tool_manifest_mismatch`), it does not degrade;
//! - NO connection is held between turns (`idle`): the guest heartbeat through the endpoint
//!   counts as traffic and would defeat the 180 s idle suspend forever;
//! - a lane serializes its operations, so parallel calls each fork their OWN ephemeral lane.
//!
//! Locking discipline: `state` is a std mutex (sync snapshots for `hand_info`/`state`/
//! `must_release`; NEVER held across await); `conn` is a tokio mutex serializing connection
//! lifecycle (held across awaits, one lifecycle operation at a time).

use aex_contracts::abi::{
    CancelRequest, Cursor, HelloRequest, HelloResponse, LaneMode, LaneRef, OperationStatus,
    PollRequest, ProtocolVersion, PutFile, PutRequest, PutSource, ReleaseRequest, RestoreSource,
    RestoreSourcePacksItem, Stream as AbiStream, SyncEntry, SyncManifest, SyncReason, SyncRequest,
    SyncScope,
};
use aws_sdk_s3::presigning::PresigningConfig;
use base64::Engine;
use brain::adapter::{
    ArtifactMeta, CallOutcome, CallRequest, HandAdapter, HandFactory, HandSpec, LostReport,
    OutputSink, SeedFile, WorkspaceFile, WorkspaceListing,
};
use brain::journal::MAX_RECORD_CONTENT_BYTES;
use brain::{BrainError, Result};
use hand_client::HandClient;
use hand_lambda::control::Control;
use hand_lambda::launch::{self, Disposition, Keepalive, LaunchedHand};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Presigned URL lifetime. Long enough for the largest sync pack on a slow link; short enough
/// that a leaked URL is a bounded exposure.
const PRESIGN_SECS: u64 = 900;
const HELLO_HEARTBEAT_MS: i64 = 5_000;

/// Long-poll windows against the hand. `start` waits briefly (most calls are short: one round
/// trip); `poll` waits long (the hand answers early on state change).
const START_WAIT_MS: u64 = 10_000;
const POLL_WAIT_MS: u64 = 30_000;
const START_MAX_BYTES: u64 = 64 * 1024;
const POLL_MAX_BYTES: u64 = 256 * 1024;
/// SIGTERM grace before SIGKILL on cancel.
const CANCEL_GRACE_MS: u64 = 2_000;

// ---------------------------------------------------------------------------------------------
// Configuration and the shared plane
// ---------------------------------------------------------------------------------------------

/// Process-wide configuration for the hand plane, from the environment.
#[derive(Debug, Clone)]
pub struct HandPlaneConfig {
    pub region: String,
    pub image: String,
    pub image_version: String,
    pub bucket: String,
    /// The platform wall for one incarnation (running + suspended). AWS enforces 8 h; tests
    /// shrink it to exercise the re-materialise path quickly.
    pub wall_seconds: u64,
    /// How long before the wall the brain syncs and releases.
    pub wall_margin_seconds: u64,
    /// Ask for a full sync when the pack chain reaches this length.
    pub full_sync_after_packs: u64,
}

impl HandPlaneConfig {
    pub fn from_env() -> Result<Self> {
        let get =
            |k: &str| std::env::var(k).map_err(|_| BrainError::Invalid(format!("{k} is not set")));
        Ok(Self {
            region: std::env::var("AWS_REGION").unwrap_or_else(|_| hand_lambda::REGION.into()),
            image: get("AEX_HAND_IMAGE")?,
            image_version: get("AEX_HAND_IMAGE_VERSION")?,
            bucket: get("AEX_SESSIONS_BUCKET")?,
            wall_seconds: std::env::var("AEX_WALL_SECONDS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(hand_lambda::MAX_DURATION_SECONDS),
            wall_margin_seconds: std::env::var("AEX_WALL_MARGIN_SECONDS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(600),
            full_sync_after_packs: 16,
        })
    }
}

/// Shared clients for the hand plane. One per process (pooled TLS is a named reason the
/// architecture keeps one client, not one per session).
pub struct HandPlane {
    pub control: Control,
    pub s3: aws_sdk_s3::Client,
    pub http: reqwest::Client,
    pub cfg: HandPlaneConfig,
    image_arn: tokio::sync::OnceCell<String>,
}

impl HandPlane {
    pub async fn from_env(cfg: HandPlaneConfig) -> Self {
        let aws = aws_config::from_env()
            .region(aws_config::Region::new(cfg.region.clone()))
            .load()
            .await;
        Self {
            control: Control::from_env(&cfg.region).await,
            s3: aws_sdk_s3::Client::new(&aws),
            http: reqwest::Client::new(),
            cfg,
            image_arn: tokio::sync::OnceCell::new(),
        }
    }

    pub async fn image_arn(&self) -> Result<String> {
        self.image_arn
            .get_or_try_init(|| async {
                hand_lambda::image::find_image_arn(&self.control, &self.cfg.image)
                    .await
                    .map_err(|e| BrainError::HandUnavailable(format!("image lookup: {e}")))?
                    .ok_or_else(|| {
                        BrainError::HandUnavailable(format!(
                            "no MicroVM image named {}",
                            self.cfg.image
                        ))
                    })
            })
            .await
            .cloned()
    }

    pub async fn presign_put(&self, key: &str) -> Result<String> {
        let cfg = PresigningConfig::expires_in(Duration::from_secs(PRESIGN_SECS))
            .map_err(|e| BrainError::HandUnavailable(format!("presign config: {e}")))?;
        Ok(self
            .s3
            .put_object()
            .bucket(&self.cfg.bucket)
            .key(key)
            .presigned(cfg)
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("presign put: {e}")))?
            .uri()
            .to_string())
    }

    pub async fn presign_get(&self, key: &str) -> Result<String> {
        let cfg = PresigningConfig::expires_in(Duration::from_secs(PRESIGN_SECS))
            .map_err(|e| BrainError::HandUnavailable(format!("presign config: {e}")))?;
        Ok(self
            .s3
            .get_object()
            .bucket(&self.cfg.bucket)
            .key(key)
            .presigned(cfg)
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("presign get: {e}")))?
            .uri()
            .to_string())
    }
}

pub fn sync_manifest_key(session_id: &str, manifest_id: &str) -> String {
    format!("sessions/{session_id}/sync/{manifest_id}.json")
}
pub fn sync_pack_key(session_id: &str, pack_id: &str) -> String {
    format!("sessions/{session_id}/sync/{pack_id}.tar.zst")
}
pub fn seed_key(session_id: &str, index: usize) -> String {
    format!("sessions/{session_id}/seed/{index:04}")
}
pub fn artifact_key(session_id: &str, name: &str) -> String {
    format!("sessions/{session_id}/artifacts/{name}")
}
pub fn transfer_key(session_id: &str) -> String {
    format!(
        "sessions/{session_id}/transfer/{}",
        brain::mint_id("xfer", 20)
    )
}

// ---------------------------------------------------------------------------------------------
// Adapter state (persisted opaquely in the journal head)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LambdaState {
    pub hand: HandDoc,
    pub sync: SyncDoc,
    pub seeds: Vec<SeedFileDoc>,
    /// True once the seeds have been applied to some incarnation (the first sync then makes
    /// them durable).
    pub seeded: bool,
}

/// The MicroVM incarnation as last known.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HandDoc {
    pub state: String,
    pub microvm_id: Option<String>,
    pub endpoint: Option<String>,
    /// Incarnation count across the session's life (HandInfo.generation).
    pub incarnations: u64,
    pub generation_id: Option<String>,
    pub session_token: Option<String>,
    pub launched_ms: Option<u64>,
    pub wall_deadline_ms: Option<u64>,
    pub image: Option<String>,
    pub image_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncDoc {
    pub manifest_id: Option<String>,
    pub synced_ms: Option<u64>,
    /// Packs referenced by the last manifest, as the hand reported. Drives the "ask for a
    /// full sync when the chain grows" policy.
    pub packs_referenced: u64,
    /// Total workspace bytes as of the last sync. Storage info, not billing authority (I9).
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedFileDoc {
    pub path: String,
    pub s3_key: String,
    pub bytes: u64,
    pub sha256: String,
    pub mode: Option<i64>,
}

// ---------------------------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------------------------

pub struct LambdaFactory {
    plane: Arc<HandPlane>,
}

impl LambdaFactory {
    pub fn new(plane: Arc<HandPlane>) -> Self {
        Self { plane }
    }

    pub async fn from_env() -> Result<Self> {
        Ok(Self::new(Arc::new(
            HandPlane::from_env(HandPlaneConfig::from_env()?).await,
        )))
    }
}

#[async_trait::async_trait]
impl HandFactory for LambdaFactory {
    async fn create(&self, spec: &HandSpec, seeds: &[SeedFile<'_>]) -> Result<serde_json::Value> {
        if spec.shape != "1gb" {
            return Err(BrainError::Invalid(format!(
                "hand.shape {} is not offered yet; this plane runs 1gb",
                spec.shape
            )));
        }
        let mut docs = Vec::with_capacity(seeds.len());
        for (i, s) in seeds.iter().enumerate() {
            let key = seed_key(&spec.session_id, i);
            let sha = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(s.bytes));
            self.plane
                .s3
                .put_object()
                .bucket(&self.plane.cfg.bucket)
                .key(&key)
                .body(s.bytes.to_vec().into())
                .send()
                .await
                .map_err(|e| BrainError::Journal(format!("seed upload: {e}")))?;
            docs.push(SeedFileDoc {
                path: s.path.to_string(),
                s3_key: key,
                bytes: s.bytes.len() as u64,
                sha256: sha,
                mode: s.mode,
            });
        }
        let st = LambdaState {
            hand: HandDoc {
                state: "preparing".into(),
                ..Default::default()
            },
            sync: SyncDoc::default(),
            seeds: docs,
            seeded: false,
        };
        serde_json::to_value(&st).map_err(|e| BrainError::Journal(format!("state: {e}")))
    }

    async fn open(
        &self,
        spec: &HandSpec,
        state: serde_json::Value,
    ) -> Result<Arc<dyn HandAdapter>> {
        let st: LambdaState = if state.is_null() {
            LambdaState::default()
        } else {
            serde_json::from_value(state)
                .map_err(|e| BrainError::Journal(format!("lambda state does not parse: {e}")))?
        };
        Ok(Arc::new(LambdaHand {
            plane: self.plane.clone(),
            spec: spec.clone(),
            state: Mutex::new(st),
            conn: tokio::sync::Mutex::new(Conn::default()),
        }))
    }

    async fn purge(&self, session_id: &str) -> Result<()> {
        let prefix = format!("sessions/{session_id}/");
        let mut token = None;
        loop {
            let out = self
                .plane
                .s3
                .list_objects_v2()
                .bucket(&self.plane.cfg.bucket)
                .prefix(&prefix)
                .set_continuation_token(token)
                .send()
                .await
                .map_err(|e| BrainError::Journal(format!("s3 list: {e}")))?;
            for obj in out.contents() {
                if let Some(key) = obj.key() {
                    let _ = self
                        .plane
                        .s3
                        .delete_object()
                        .bucket(&self.plane.cfg.bucket)
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

    async fn artifact_url(&self, _session_id: &str, location: &str) -> Option<String> {
        self.plane.presign_get(location).await.ok()
    }
}

// ---------------------------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Conn {
    live: Option<Live>,
    keepalive: Option<Keepalive>,
}

struct Live {
    hand: LaunchedHand,
    client: Arc<HandClient>,
}

pub struct LambdaHand {
    plane: Arc<HandPlane>,
    spec: HandSpec,
    state: Mutex<LambdaState>,
    conn: tokio::sync::Mutex<Conn>,
}

impl LambdaHand {
    fn snap(&self) -> LambdaState {
        self.state.lock().expect("lambda state").clone()
    }

    fn merge<T>(&self, f: impl FnOnce(&mut LambdaState) -> T) -> T {
        f(&mut self.state.lock().expect("lambda state"))
    }

    /// Makes a client available, reconnecting / resuming / re-materialising as the VM state
    /// demands. Runs under the `conn` lock.
    async fn ensure_ready_inner(&self, conn: &mut Conn) -> Result<Option<LostReport>> {
        if !self.spec.hand_enabled {
            return Err(BrainError::HandUnavailable(
                "hand is disabled for this session".into(),
            ));
        }
        if let Some(l) = &conn.live {
            if !l.client.is_closed() {
                return Ok(None);
            }
            // The WebSocket died since the last call (suspend, wall, transport). A closed
            // client never recovers: drop it and let the diagnosis below decide.
            conn.live = None;
            conn.keepalive = None;
        }
        let mut lost: Option<LostReport> = None;

        // A previous incarnation may still be reachable.
        if let Some(vm_id) = self.snap().hand.microvm_id {
            loop {
                match launch::diagnose(&self.plane.control, &vm_id).await {
                    Disposition::Reconnect | Disposition::ResumeThenReconnect => {
                        match self.reconnect(conn).await {
                            Ok(true) => return Ok(None),
                            Ok(false) => {
                                // The incarnation restarted from scratch: its state is gone.
                                let _ = self.plane.control.terminate(&vm_id).await;
                                lost = Some(LostReport {
                                    reason: "hand generation changed (in-VM restart)".into(),
                                });
                                break;
                            }
                            Err(e) => {
                                tracing::warn!(session = %self.spec.session_id, error = %e, "reconnect failed; re-diagnosing");
                                match launch::diagnose(&self.plane.control, &vm_id).await {
                                    Disposition::Lost(reason) => {
                                        lost = Some(LostReport { reason });
                                        break;
                                    }
                                    _ => return Err(e),
                                }
                            }
                        }
                    }
                    Disposition::Wait => {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                    Disposition::Lost(reason) => {
                        lost = Some(LostReport { reason });
                        break;
                    }
                }
            }
        }

        // Fresh incarnation: launch, hello with restore, seed if never synced.
        self.launch_fresh(conn).await?;
        Ok(lost)
    }

    /// Reconnects to a live (running or suspended) incarnation. `Ok(true)`: same generation,
    /// state intact. `Ok(false)`: fresh generation -- treated as a loss upstream.
    async fn reconnect(&self, conn: &mut Conn) -> Result<bool> {
        let st = self.snap();
        let vm_id = st
            .hand
            .microvm_id
            .clone()
            .ok_or_else(|| BrainError::HandUnavailable("no microvm".into()))?;
        let endpoint = st
            .hand
            .endpoint
            .clone()
            .ok_or_else(|| BrainError::HandUnavailable("no endpoint".into()))?;
        // The JWE is short-lived (1 h): mint a fresh one per reconnect.
        let auth_token = self
            .plane
            .control
            .auth_token(&vm_id)
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("auth token: {e}")))?;
        let hand = LaunchedHand {
            microvm_id: vm_id,
            endpoint,
            auth_token,
        };
        launch::resume_via_probe(&self.plane.http, &hand, Duration::from_secs(90))
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("resume: {e}")))?;
        let client = launch::connect(&hand, 1)
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("connect: {e}")))?;
        let hello = self.hello(&client, &st, None).await?;
        let got = (*hello.generation_id).to_string();
        if st.hand.generation_id.as_deref() != Some(got.as_str()) {
            tracing::warn!(expected = ?st.hand.generation_id, got = %got, "generation changed on reconnect");
            return Ok(false);
        }
        conn.live = Some(Live {
            hand,
            client: Arc::new(client),
        });
        conn.keepalive = Some(Keepalive::spawn(
            conn.live.as_ref().expect("just set").hand.clone(),
            Duration::from_secs(60),
        ));
        Ok(true)
    }

    async fn launch_fresh(&self, conn: &mut Conn) -> Result<()> {
        let image_arn = self.plane.image_arn().await?;
        let token = brain::mint_id("tok", 32);
        let incarnation = self.snap().hand.incarnations + 1;
        let client_token = format!("{}-{incarnation}", self.spec.session_id);
        let hand = launch::launch(
            &self.plane.control,
            &image_arn,
            &self.plane.cfg.image_version,
            &token,
            &client_token,
        )
        .await
        .map_err(|e| BrainError::HandUnavailable(format!("launch: {e}")))?;
        let client = launch::connect(&hand, 1)
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("connect: {e}")))?;

        // Re-materialise from the last sync, if there is one.
        let restore = self.restore_source().await?;
        let restored = restore.is_some();
        let now = brain::wall_ms();
        let st_for_hello = self.merge(|st| {
            st.hand = HandDoc {
                state: "ready".into(),
                microvm_id: Some(hand.microvm_id.clone()),
                endpoint: Some(hand.endpoint.clone()),
                incarnations: incarnation,
                generation_id: None,
                session_token: Some(token.clone()),
                launched_ms: Some(now),
                wall_deadline_ms: Some(now + self.plane.cfg.wall_seconds * 1000),
                image: Some(self.plane.cfg.image.clone()),
                image_version: Some(self.plane.cfg.image_version.clone()),
            };
            st.clone()
        });
        let hello = self.hello(&client, &st_for_hello, restore).await?;
        self.merge(|st| st.hand.generation_id = Some((*hello.generation_id).to_string()));

        // First boot of a session with seed files and no sync yet: apply them now; the first
        // sync makes them durable.
        let st = self.snap();
        if !restored && !st.seeded && !st.seeds.is_empty() {
            self.apply_seeds(&client, &st.seeds).await?;
            self.merge(|s| s.seeded = true);
        }
        conn.live = Some(Live {
            hand,
            client: Arc::new(client),
        });
        conn.keepalive = Some(Keepalive::spawn(
            conn.live.as_ref().expect("just set").hand.clone(),
            Duration::from_secs(60),
        ));
        Ok(())
    }

    async fn hello(
        &self,
        client: &HandClient,
        st: &LambdaState,
        restore: Option<RestoreSource>,
    ) -> Result<HelloResponse> {
        let token = st
            .hand
            .session_token
            .clone()
            .ok_or_else(|| BrainError::HandUnavailable("no session token for hello".into()))?;
        let req = HelloRequest {
            protocol: ProtocolVersion::CURRENT,
            session_id: self
                .spec
                .session_id
                .parse()
                .map_err(|_| BrainError::Invalid("session id".into()))?,
            session_token: token,
            expected_generation_id: st
                .hand
                .generation_id
                .as_deref()
                .and_then(|g| g.parse().ok()),
            tool_manifest_digest: Some(
                self.spec
                    .manifest_digest
                    .parse()
                    .map_err(|_| BrainError::Invalid("manifest digest".into()))?,
            ),
            env: self.spec.env.clone(),
            sync: SyncScope {
                roots: vec!["/workspace".into(), "/home/agent".into()],
                exclude: vec![],
            },
            restore,
            heartbeat_ms: HELLO_HEARTBEAT_MS,
        };
        let hello = client.hello(req).await.map_err(|e| {
            let s = e.to_string();
            if s.contains("tool_manifest_mismatch") {
                BrainError::SessionFailed(format!("tool_manifest_mismatch: {s}"))
            } else {
                BrainError::HandUnavailable(format!("hello: {s}"))
            }
        })?;
        Ok(hello)
    }

    /// Builds the restore source from the last sync: fetch the manifest (brain-side, with our
    /// credentials), presign a GET per referenced pack (the hand gets URLs, never creds).
    async fn restore_source(&self) -> Result<Option<RestoreSource>> {
        let Some(manifest_id) = self.snap().sync.manifest_id else {
            return Ok(None);
        };
        let manifest_key = sync_manifest_key(&self.spec.session_id, &manifest_id);
        let bytes = self
            .plane
            .s3
            .get_object()
            .bucket(&self.plane.cfg.bucket)
            .key(&manifest_key)
            .send()
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("manifest read: {e}")))?
            .body
            .collect()
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("manifest body: {e}")))?
            .into_bytes();
        let manifest: aex_contracts::abi::SyncManifest = serde_json::from_slice(&bytes)
            .map_err(|e| BrainError::HandUnavailable(format!("manifest parse: {e}")))?;
        let mut packs = Vec::with_capacity(manifest.packs.len());
        for p in &manifest.packs {
            packs.push(RestoreSourcePacksItem {
                pack_id: p.pack_id.clone(),
                get_url: self
                    .plane
                    .presign_get(&sync_pack_key(&self.spec.session_id, &p.pack_id))
                    .await?,
            });
        }
        Ok(Some(RestoreSource {
            manifest_id: manifest_id
                .parse()
                .map_err(|_| BrainError::Invalid("manifest id".into()))?,
            manifest_get_url: self.plane.presign_get(&manifest_key).await?,
            packs,
        }))
    }

    async fn apply_seeds(&self, client: &HandClient, seeds: &[SeedFileDoc]) -> Result<()> {
        let mut files = Vec::with_capacity(seeds.len());
        for s in seeds {
            files.push(PutFile {
                path: s.path.clone(),
                mode: s.mode,
                source: PutSource::Url {
                    get_url: self.plane.presign_get(&s.s3_key).await?,
                    bytes: s.bytes,
                    sha256: s
                        .sha256
                        .parse()
                        .map_err(|_| BrainError::Invalid("seed sha256".into()))?,
                },
            });
        }
        client
            .put(PutRequest { files })
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("seed put: {e}")))?;
        Ok(())
    }

    /// One workspace sync (the durability point). Runs under the `conn` lock.
    async fn sync_inner(&self, conn: &mut Conn, reason: SyncReason) -> Result<()> {
        let Some(live) = &conn.live else {
            return Ok(());
        };
        let full = self.snap().sync.packs_referenced >= self.plane.cfg.full_sync_after_packs;
        let manifest_id = brain::mint_id("m", 16);
        let pack_id = brain::mint_id("p", 16);
        let req = SyncRequest {
            reason,
            manifest_id: manifest_id
                .parse()
                .map_err(|_| BrainError::Invalid("manifest id".into()))?,
            manifest_put_url: self
                .plane
                .presign_put(&sync_manifest_key(&self.spec.session_id, &manifest_id))
                .await?,
            pack_id: pack_id
                .parse()
                .map_err(|_| BrainError::Invalid("pack id".into()))?,
            pack_put_url: self
                .plane
                .presign_put(&sync_pack_key(&self.spec.session_id, &pack_id))
                .await?,
            full,
        };
        let resp = live
            .client
            .sync(req)
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("sync: {e}")))?;
        // The hand echoes the manifest id it actually stands behind: on a no-change sync that
        // is the PREVIOUS manifest, which still describes the tree. Store the echo.
        self.merge(|st| {
            st.sync.manifest_id = Some((*resp.manifest_id).to_string());
            st.sync.synced_ms = Some(brain::wall_ms());
            st.sync.packs_referenced = resp.packs_referenced;
            st.sync.bytes_total = resp.bytes_total;
        });
        Ok(())
    }

    async fn client(&self) -> Option<Arc<HandClient>> {
        let conn = self.conn.lock().await;
        conn.live.as_ref().map(|l| l.client.clone())
    }

    async fn manifest_listing(
        &self,
        path: &str,
        recursive: bool,
        source: aex_contracts::session::FileListSource,
    ) -> Result<WorkspaceListing> {
        use aex_contracts::session::{FileEntry, FileEntryKind, Timestamp};
        let snap = self.snap();
        let Some(manifest_id) = snap.sync.manifest_id.as_deref() else {
            if path != "/workspace" {
                return Err(BrainError::FileNotFound(path.into()));
            }
            return Ok(WorkspaceListing {
                entries: Vec::new(),
                source,
                synced_ms: snap.sync.synced_ms,
            });
        };
        let object = self
            .plane
            .s3
            .get_object()
            .bucket(&self.plane.cfg.bucket)
            .key(sync_manifest_key(&self.spec.session_id, manifest_id))
            .send()
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("manifest download: {e}")))?;
        const MAX_MANIFEST_BYTES: i64 = 16 * 1024 * 1024;
        if object.content_length().unwrap_or_default() > MAX_MANIFEST_BYTES {
            return Err(BrainError::Hand(
                "sync manifest exceeds the 16 MiB control limit".into(),
            ));
        }
        let raw = object
            .body
            .collect()
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("manifest body: {e}")))?
            .into_bytes();
        if raw.len() > MAX_MANIFEST_BYTES as usize {
            return Err(BrainError::Hand(
                "sync manifest exceeds the 16 MiB control limit".into(),
            ));
        }
        let manifest: SyncManifest = serde_json::from_slice(&raw)
            .map_err(|e| BrainError::Hand(format!("sync manifest is invalid: {e}")))?;
        let child_prefix = format!("{}/", path.trim_end_matches('/'));
        let selected = |candidate: &str| {
            candidate == path
                || candidate
                    .strip_prefix(&child_prefix)
                    .is_some_and(|rest| !rest.is_empty() && (recursive || !rest.contains('/')))
        };
        let mut entries = Vec::new();
        for entry in manifest.entries {
            let mapped = match entry {
                SyncEntry::File {
                    mtime_ns,
                    path,
                    sha256,
                    size,
                    ..
                } if selected(&path) => {
                    let secs = i64::try_from(mtime_ns / 1_000_000_000).ok();
                    let nanos = (mtime_ns % 1_000_000_000) as u32;
                    Some(FileEntry {
                        kind: FileEntryKind::File,
                        modified_at: secs
                            .and_then(|s| chrono::DateTime::<chrono::Utc>::from_timestamp(s, nanos))
                            .map(Timestamp),
                        path,
                        sha256: (*sha256).to_string().parse().ok(),
                        size: Some(size),
                    })
                }
                SyncEntry::Dir { path, .. } if selected(&path) => Some(FileEntry {
                    kind: FileEntryKind::Dir,
                    modified_at: None,
                    path,
                    sha256: None,
                    size: None,
                }),
                SyncEntry::Symlink { path, .. } if selected(&path) => Some(FileEntry {
                    kind: FileEntryKind::Symlink,
                    modified_at: None,
                    path,
                    sha256: None,
                    size: None,
                }),
                _ => None,
            };
            if let Some(entry) = mapped {
                entries.push(entry);
            }
        }
        entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        if entries.is_empty() && path != "/workspace" {
            return Err(BrainError::FileNotFound(path.into()));
        }
        Ok(WorkspaceListing {
            entries,
            source,
            synced_ms: snap.sync.synced_ms,
        })
    }

    async fn delete_transfer(&self, key: &str) {
        if let Err(error) = self
            .plane
            .s3
            .delete_object()
            .bucket(&self.plane.cfg.bucket)
            .key(key)
            .send()
            .await
        {
            tracing::warn!(session = %self.spec.session_id, %error, "temporary file transfer cleanup failed");
        }
    }
}

#[async_trait::async_trait]
impl HandAdapter for LambdaHand {
    async fn ensure_ready(&self) -> Result<Option<LostReport>> {
        let mut conn = self.conn.lock().await;
        self.ensure_ready_inner(&mut conn).await
    }

    async fn call(
        &self,
        req: CallRequest,
        cancel: CancellationToken,
        sink: OutputSink,
    ) -> CallOutcome {
        let Some(client) = self.client().await else {
            return CallOutcome {
                outcome: "interrupted".into(),
                content: "hand not connected".into(),
                is_error: true,
                exit_code: None,
                duration_ms: 0,
                truncated: false,
            };
        };
        hand_call(&client, &req, &cancel, &sink).await
    }

    fn on_message_admitted(&self) {
        // Speculative resume (F-4): endpoint traffic now, so a suspended hand is running
        // again by the time the model asks for a tool. An unauthenticated probe still reaches
        // the endpoint and triggers the resume, so no fresh JWE is needed here.
        let st = self.snap();
        let (Some(microvm_id), Some(endpoint)) = (st.hand.microvm_id, st.hand.endpoint) else {
            return;
        };
        let hand = LaunchedHand {
            microvm_id,
            endpoint,
            auth_token: String::new(),
        };
        let http = self.plane.http.clone();
        tokio::spawn(async move {
            let _ = launch::resume_via_probe(&http, &hand, Duration::from_secs(60)).await;
        });
    }

    fn idle(&self) {
        // Disconnect between turns: an open ABI WebSocket carries the guest heartbeat through
        // the endpoint, which counts as traffic and defeats the 180 s idle suspend forever.
        // Connection loss is not hand loss (I10): the VM stays up, AWS suspends it when truly
        // idle, and the next message reconnects through the speculative resume.
        if let Ok(mut conn) = self.conn.try_lock() {
            conn.live = None;
            conn.keepalive = None;
        }
    }

    async fn checkpoint(&self) -> Result<()> {
        let mut conn = self.conn.lock().await;
        self.sync_inner(&mut conn, SyncReason::TurnEnd).await
    }

    fn must_release(&self) -> bool {
        let st = self.snap();
        match st.hand.wall_deadline_ms {
            Some(deadline) => {
                brain::wall_ms() + self.plane.cfg.wall_margin_seconds * 1000 >= deadline
            }
            None => false,
        }
    }

    async fn release(&self) -> Result<()> {
        let mut conn = self.conn.lock().await;
        conn.keepalive = None;
        if conn.live.is_none() {
            // Wake a suspended incarnation for a final sync only if one exists at all; a hand
            // already gone keeps its last sync as the restore point.
            if self.snap().hand.microvm_id.is_some() {
                let _ = self.ensure_ready_inner(&mut conn).await;
            }
        }
        if conn.live.is_some()
            && let Err(e) = self.sync_inner(&mut conn, SyncReason::BeforeRelease).await
        {
            tracing::warn!(session = %self.spec.session_id, error = %e, "pre-release sync failed");
        }
        if let Some(vm) = self.snap().hand.microvm_id {
            match self.plane.control.terminate(&vm).await {
                Ok(()) => {}
                Err(hand_lambda::control::ControlError::Gone(_)) => {}
                Err(e) => {
                    tracing::warn!(session = %self.spec.session_id, error = %e, "terminate failed");
                }
            }
        }
        conn.live = None;
        self.merge(|st| {
            st.hand.state = "released".into();
            st.hand.microvm_id = None;
            st.hand.endpoint = None;
            st.hand.generation_id = None;
            st.hand.session_token = None;
            st.hand.wall_deadline_ms = None;
        });
        Ok(())
    }

    async fn acknowledge(&self, call_ids: &[String]) {
        // The results are committed; the hand may delete spill files and forget the ops.
        if let Some(client) = self.client().await {
            let ids = call_ids.iter().filter_map(|c| c.parse().ok()).collect();
            let _ = client.release(ReleaseRequest { operation_ids: ids }).await;
        }
    }

    fn workspace_bytes(&self) -> u64 {
        self.snap().sync.bytes_total
    }

    async fn list_files(&self, path: &str, recursive: bool) -> Result<WorkspaceListing> {
        let released = self.snap().hand.state == "released";
        if released {
            return self
                .manifest_listing(
                    path,
                    recursive,
                    aex_contracts::session::FileListSource::Manifest,
                )
                .await;
        }
        // Capture a complete, metadata-bearing live view with the existing sync mechanism;
        // this avoids parsing a lossy tool preview and makes the listing a durability point.
        let mut conn = self.conn.lock().await;
        self.sync_inner(&mut conn, SyncReason::Explicit).await?;
        drop(conn);
        self.manifest_listing(
            path,
            recursive,
            aex_contracts::session::FileListSource::Hand,
        )
        .await
    }

    async fn read_file(&self, path: &str, max_bytes: usize) -> Result<WorkspaceFile> {
        use aex_contracts::abi::{PersistItem, PersistRequest as AbiPersist, PersistSource};
        use aex_contracts::session::{FileEntry, FileEntryKind};
        let client = self
            .client()
            .await
            .ok_or_else(|| BrainError::HandUnavailable("no hand".into()))?;
        let key = transfer_key(&self.spec.session_id);
        let result = async {
            let put_url = self.plane.presign_put(&key).await?;
            let response = client
                .persist(AbiPersist {
                    items: vec![PersistItem {
                        name: "download.bin"
                            .parse()
                            .map_err(|_| BrainError::Invalid("transfer name".into()))?,
                        put_url,
                        media_type: Some("application/octet-stream".into()),
                        source: PersistSource::Path {
                            path: path.to_string(),
                        },
                    }],
                })
                .await
                .map_err(|e| BrainError::Hand(format!("file download stage: {e}")))?;
            let item = response
                .persisted
                .first()
                .ok_or_else(|| BrainError::Hand("file download returned no item".into()))?;
            if item.bytes > max_bytes as u64 {
                return Err(BrainError::FileTooLarge { limit: max_bytes });
            }
            let object = self
                .plane
                .s3
                .get_object()
                .bucket(&self.plane.cfg.bucket)
                .key(&key)
                .send()
                .await
                .map_err(|e| BrainError::HandUnavailable(format!("file download: {e}")))?;
            let bytes = object
                .body
                .collect()
                .await
                .map_err(|e| BrainError::HandUnavailable(format!("file body: {e}")))?
                .into_bytes()
                .to_vec();
            if bytes.len() > max_bytes {
                return Err(BrainError::FileTooLarge { limit: max_bytes });
            }
            let got = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&bytes));
            if got != *item.sha256 {
                return Err(BrainError::Hand("file download digest mismatch".into()));
            }
            Ok(WorkspaceFile {
                entry: FileEntry {
                    kind: FileEntryKind::File,
                    modified_at: None,
                    path: path.to_string(),
                    sha256: got.parse().ok(),
                    size: Some(bytes.len() as u64),
                },
                bytes,
            })
        }
        .await;
        self.delete_transfer(&key).await;
        result
    }

    async fn write_file(
        &self,
        path: &str,
        bytes: &[u8],
    ) -> Result<aex_contracts::session::FileEntry> {
        use aex_contracts::session::{FileEntry, FileEntryKind};
        let client = self
            .client()
            .await
            .ok_or_else(|| BrainError::HandUnavailable("no hand".into()))?;
        let key = transfer_key(&self.spec.session_id);
        let result = async {
            let digest = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(bytes));
            self.plane
                .s3
                .put_object()
                .bucket(&self.plane.cfg.bucket)
                .key(&key)
                .body(bytes.to_vec().into())
                .send()
                .await
                .map_err(|e| BrainError::HandUnavailable(format!("file upload stage: {e}")))?;
            let response = client
                .put(PutRequest {
                    files: vec![PutFile {
                        path: path.to_string(),
                        mode: None,
                        source: PutSource::Url {
                            get_url: self.plane.presign_get(&key).await?,
                            bytes: bytes.len() as u64,
                            sha256: digest
                                .clone()
                                .parse()
                                .map_err(|_| BrainError::Hand("sha256 conversion".into()))?,
                        },
                    }],
                })
                .await
                .map_err(|e| BrainError::Hand(format!("file upload: {e}")))?;
            let written = response
                .written
                .first()
                .ok_or_else(|| BrainError::Hand("file upload returned no item".into()))?;
            if written.bytes != bytes.len() as u64 || *written.sha256 != digest {
                return Err(BrainError::Hand("file upload verification mismatch".into()));
            }
            Ok(FileEntry {
                kind: FileEntryKind::File,
                modified_at: None,
                path: path.to_string(),
                sha256: digest.parse().ok(),
                size: Some(bytes.len() as u64),
            })
        }
        .await;
        self.delete_transfer(&key).await;
        result
    }

    async fn persist(
        &self,
        name: &str,
        path: &str,
        media_type: Option<&str>,
    ) -> Result<ArtifactMeta> {
        use aex_contracts::abi::{PersistItem, PersistRequest as AbiPersist, PersistSource};
        let client = self
            .client()
            .await
            .ok_or_else(|| BrainError::HandUnavailable("no hand".into()))?;
        let key = artifact_key(&self.spec.session_id, name);
        let put_url = self.plane.presign_put(&key).await?;
        let resp = client
            .persist(AbiPersist {
                items: vec![PersistItem {
                    name: name
                        .parse()
                        .map_err(|_| BrainError::Invalid("artifact name".into()))?,
                    put_url,
                    media_type: media_type.map(str::to_owned),
                    source: PersistSource::Path {
                        path: path.to_string(),
                    },
                }],
            })
            .await
            .map_err(|e| BrainError::Hand(format!("persist: {e}")))?;
        let item = resp
            .persisted
            .first()
            .ok_or_else(|| BrainError::Hand("persist returned no items".into()))?;
        Ok(ArtifactMeta {
            bytes: item.bytes,
            sha256: (*item.sha256).to_string(),
            media_type: if item.media_type.is_empty() {
                "application/octet-stream".into()
            } else {
                item.media_type.clone()
            },
            location: key,
        })
    }

    fn hand_info(&self) -> aex_contracts::session::HandInfo {
        use aex_contracts::session::{HandInfo, HandShape, HandState};
        let st = self.snap();
        let state = match st.hand.state.as_str() {
            "ready" => HandState::Ready,
            "suspended" => HandState::Suspended,
            "released" => HandState::Released,
            "lost" => HandState::Lost,
            _ => HandState::Preparing,
        };
        HandInfo {
            generation: Some(st.hand.incarnations),
            last_sync_at: st.sync.synced_ms.map(brain::events::ts),
            live_jobs: Some(0),
            shape: match self.spec.shape.as_str() {
                "2gb" => HandShape::X2gb,
                "4gb" => HandShape::X4gb,
                "8gb" => HandShape::X8gb,
                _ => HandShape::X1gb,
            },
            started_at: st.hand.launched_ms.map(brain::events::ts),
            state,
            wall_deadline_at: st.hand.wall_deadline_ms.map(brain::events::ts),
        }
    }

    fn state(&self) -> serde_json::Value {
        serde_json::to_value(self.snap()).unwrap_or(serde_json::Value::Null)
    }
}

// ---------------------------------------------------------------------------------------------
// One remote tool call
// ---------------------------------------------------------------------------------------------

fn decode_slices(
    slices: &[aex_contracts::abi::OutputSlice],
    stdout: &mut String,
    stderr: &mut String,
    sink: &OutputSink,
) {
    for s in slices {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&s.data_base64)
            .unwrap_or_default();
        if bytes.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        match s.stream {
            AbiStream::Stdout => {
                sink("stdout", s.offset, text.to_string());
                stdout.push_str(&text);
            }
            AbiStream::Stderr => {
                sink("stderr", s.offset, text.to_string());
                stderr.push_str(&text);
            }
        }
    }
}

/// One hand tool call to terminal state: start (short wait), poll loop (long wait), output
/// streamed as slices arrive, cancel honoured with grace. A lane serializes its operations,
/// so a parallel call forks its OWN ephemeral lane off the root.
async fn hand_call(
    client: &HandClient,
    req: &CallRequest,
    cancel: &CancellationToken,
    sink: &OutputSink,
) -> CallOutcome {
    let t0 = Instant::now();
    let fail = |content: String, outcome: &str, t0: Instant| CallOutcome {
        outcome: outcome.into(),
        content,
        is_error: outcome != "completed",
        exit_code: None,
        duration_ms: t0.elapsed().as_millis() as u64,
        truncated: false,
    };

    let lane = if req.parallel {
        LaneRef {
            id: match brain::mint_id("lane", 12).parse() {
                Ok(id) => id,
                Err(_) => return fail("lane id".into(), "failed", t0),
            },
            mode: LaneMode::Ephemeral,
            parent: Some("0".parse().expect("root lane id")),
        }
    } else {
        hand_client::root_lane()
    };

    let started = match client
        .start(hand_client::start_request(
            &req.call_id,
            &req.tool,
            req.input.clone(),
            lane,
            None,
            false,
            START_WAIT_MS,
            START_MAX_BYTES,
        ))
        .await
    {
        Ok(s) => s,
        Err(e) => {
            return fail(
                format!("hand start failed: {e}"),
                interrupted_or_failed(&e),
                t0,
            );
        }
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut view = started.view;
    decode_slices(&started.slices, &mut stdout, &mut stderr, sink);

    let mut cancelled = false;
    while view.status != OperationStatus::Terminal {
        if cancel.is_cancelled() && !cancelled {
            cancelled = true;
            let _ = client
                .cancel(CancelRequest {
                    operation_id: match req.call_id.parse() {
                        Ok(id) => id,
                        Err(_) => return fail("operation id".into(), "failed", t0),
                    },
                    grace_ms: Some(CANCEL_GRACE_MS),
                })
                .await;
        }
        let poll = client
            .poll(PollRequest {
                operation_id: match req.call_id.parse() {
                    Ok(id) => id,
                    Err(_) => return fail("operation id".into(), "failed", t0),
                },
                cursors: vec![
                    Cursor {
                        stream: AbiStream::Stdout,
                        offset: stdout.len() as u64,
                    },
                    Cursor {
                        stream: AbiStream::Stderr,
                        offset: stderr.len() as u64,
                    },
                ],
                wait_ms: POLL_WAIT_MS,
                max_bytes: POLL_MAX_BYTES,
            })
            .await;
        match poll {
            Ok(p) => {
                decode_slices(&p.slices, &mut stdout, &mut stderr, sink);
                view = p.view;
            }
            Err(e) => {
                // The connection died under the call. I10: the loss is classified by the
                // session layer; here the call is interrupted, never replayed.
                return fail(
                    format!("hand connection lost mid-call: {e}"),
                    "interrupted",
                    t0,
                );
            }
        }
    }

    let terminal = view.terminal.as_ref();
    let outcome = terminal
        .map(|t| match t.outcome {
            aex_contracts::abi::Outcome::Completed => "completed",
            aex_contracts::abi::Outcome::Failed => "failed",
            aex_contracts::abi::Outcome::Cancelled => "cancelled",
            aex_contracts::abi::Outcome::DeadlineExceeded => "deadline_exceeded",
            aex_contracts::abi::Outcome::Interrupted => "interrupted",
        })
        .unwrap_or("failed")
        .to_string();
    let exit_code = terminal.and_then(|t| t.exit_code);

    let mut content = stdout;
    if !stderr.is_empty() {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str("[stderr]\n");
        content.push_str(&stderr);
    }
    if let Some(t) = terminal {
        if let Some(err) = &t.error {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&format!("[error] {}: {}", err.code, err.message));
        }
        if let Some(out) = &t.output {
            let is_bash = req.tool == "bash";
            let timed_out = out
                .get("timed_out")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_bash && timed_out {
                content = format!("[command timed out]\n{content}");
            }
            if !is_bash {
                if !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(&format!("[meta] {out}"));
            }
        }
    }
    let mut truncated = false;
    if content.len() > MAX_RECORD_CONTENT_BYTES {
        // Tail-retained: the end of the output is where compilers and tests put the verdict.
        let keep_from = content.len() - MAX_RECORD_CONTENT_BYTES;
        let mut start = keep_from;
        while !content.is_char_boundary(start) {
            start += 1;
        }
        content = format!(
            "[output truncated: first {start} bytes elided]\n{}",
            &content[start..]
        );
        truncated = true;
    }

    let is_error = outcome != "completed";
    CallOutcome {
        outcome,
        content,
        is_error,
        exit_code,
        duration_ms: t0.elapsed().as_millis() as u64,
        truncated,
    }
}

fn interrupted_or_failed(e: &hand_client::ClientError) -> &'static str {
    let s = e.to_string();
    if s.contains("connection") || s.contains("closed") || s.contains("timed out") {
        "interrupted"
    } else {
        "failed"
    }
}
