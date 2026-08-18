//! The brain's side of the hand: launch, hello, reconnect, sync, re-materialise, release.
//!
//! Policy lives here; mechanics live in `hand-lambda`. The invariants this module owns:
//! - the brain mints every transfer URL (I8: the hand holds no platform credential);
//! - a connection loss is not a hand loss (I10): `diagnose` classifies, and only a VM that is
//!   truly gone becomes `hand_lost` -- in-flight calls are then reported `interrupted`, never
//!   replayed;
//! - the 8 h MicroVM wall is survived by syncing before the deadline and re-materialising a
//!   fresh incarnation from the last manifest on the next touch;
//! - `hello` always carries the sealed manifest digest: a hand that cannot serve it fails the
//!   session (`tool_manifest_mismatch`), it does not degrade.

use crate::journal::{HandDoc, HeadDoc, SeedFileDoc};
use crate::{BrainError, Result};
use aex_contracts::abi::{
    HelloRequest, HelloResponse, ProtocolVersion, PutFile, PutRequest, PutSource, RestoreSource,
    RestoreSourcePacksItem, SyncReason, SyncRequest, SyncScope,
};
use aws_sdk_s3::presigning::PresigningConfig;
use hand_client::HandClient;
use hand_lambda::control::Control;
use hand_lambda::launch::{self, Disposition, Keepalive, LaunchedHand};
use std::sync::Arc;
use std::time::Duration;

/// Presigned URL lifetime. Long enough for the largest sync pack on a slow link; short enough
/// that a leaked URL is a bounded exposure.
const PRESIGN_SECS: u64 = 900;

/// How long a fresh launch may take before the session reports `hand_unavailable`.
const HELLO_HEARTBEAT_MS: i64 = 5_000;

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

/// What one sync accomplished, for the journal HEAD.
#[derive(Debug, Clone)]
pub struct SyncDone {
    pub manifest_id: String,
    pub changed: bool,
    pub packs_referenced: u64,
    pub synced_ms: u64,
}

/// The reason a hand became unusable mid-session, surfaced as `hand.lost`.
#[derive(Debug, Clone)]
pub struct LostReport {
    pub reason: String,
}

/// One session's hand, as held by its actor. All state that must survive the actor's discard
/// lives in `HeadDoc.hand`; this struct is only the live connection.
pub struct SessionHand {
    plane: Arc<HandPlane>,
    session_id: String,
    live: Option<Live>,
    keepalive: Option<Keepalive>,
}

struct Live {
    hand: LaunchedHand,
    client: Arc<HandClient>,
}

impl SessionHand {
    pub fn new(plane: Arc<HandPlane>, session_id: impl Into<String>) -> Self {
        Self {
            plane,
            session_id: session_id.into(),
            live: None,
            keepalive: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.live.is_some()
    }

    pub fn client(&self) -> Option<Arc<HandClient>> {
        self.live.as_ref().map(|l| l.client.clone())
    }

    /// Endpoint traffic on a timer while a turn is running, so the idle policy cannot suspend
    /// a hand under an in-flight tool call.
    pub fn hold_up(&mut self) {
        if self.keepalive.is_none()
            && let Some(l) = &self.live
        {
            self.keepalive = Some(Keepalive::spawn(l.hand.clone(), Duration::from_secs(60)));
        }
    }

    /// Turn over: stop the keepalive and let AWS suspend the idle hand (that IS the M0
    /// suspend path -- the brain never suspends explicitly in normal operation).
    pub fn let_idle(&mut self) {
        self.keepalive = None;
    }

    /// Fire-and-forget speculative resume, called at message admission (F-4: hides the
    /// resume entirely behind the model's first round).
    pub fn speculative_resume(&self, head: &HeadDoc) {
        let Some(hand) = launched_from_doc(&head.hand) else {
            return;
        };
        let http = self.plane.http.clone();
        tokio::spawn(async move {
            let _ = launch::resume_via_probe(&http, &hand, Duration::from_secs(60)).await;
        });
    }

    /// Makes the hand ready: reconnects, resumes, launches or re-materialises as the state
    /// demands. Mutates `head.hand`; the caller journals. Returns a `LostReport` when a
    /// previous incarnation died (its in-flight calls are `interrupted`, never replayed).
    pub async fn ensure_ready(&mut self, head: &mut HeadDoc) -> Result<Option<LostReport>> {
        if !head.prefix.hand_enabled {
            return Err(BrainError::HandUnavailable(
                "hand is disabled for this session".into(),
            ));
        }
        if let Some(l) = &self.live {
            if !l.client.is_closed() {
                return Ok(None);
            }
            // The WebSocket died since the last call (suspend, wall, transport). A closed
            // client never recovers: drop it and let the diagnosis below decide.
            self.live = None;
            self.keepalive = None;
        }
        let mut lost: Option<LostReport> = None;

        // A previous incarnation may still be reachable.
        if let Some(vm_id) = head.hand.microvm_id.clone() {
            loop {
                match launch::diagnose(&self.plane.control, &vm_id).await {
                    Disposition::Reconnect | Disposition::ResumeThenReconnect => {
                        match self.reconnect(head).await {
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
                                // Reconnect failed against a VM the control plane says exists:
                                // classify once more; a race with the wall shows up here.
                                tracing::warn!(session = %self.session_id, error = %e, "reconnect failed; re-diagnosing");
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
        self.launch_fresh(head).await?;
        Ok(lost)
    }

    /// Reconnects to a live (running or suspended) incarnation. `Ok(true)`: same generation,
    /// state intact. `Ok(false)`: the incarnation restarted from scratch (fresh generation) --
    /// the caller treats it as a loss and re-materialises.
    async fn reconnect(&mut self, head: &mut HeadDoc) -> Result<bool> {
        let vm_id = head
            .hand
            .microvm_id
            .clone()
            .ok_or_else(|| BrainError::HandUnavailable("no microvm".into()))?;
        let endpoint = head
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
        let hello = self.hello(&client, head, None).await?;
        let expected = head.hand.generation_id.clone();
        let got = (*hello.generation_id).to_string();
        if expected.as_deref() != Some(got.as_str()) {
            tracing::warn!(expected = ?expected, got = %got, "generation changed on reconnect");
            return Ok(false);
        }
        self.live = Some(Live {
            hand,
            client: Arc::new(client),
        });
        Ok(true)
    }

    async fn launch_fresh(&mut self, head: &mut HeadDoc) -> Result<()> {
        let image_arn = self.plane.image_arn().await?;
        let token = crate::mint_id("tok", 32);
        let incarnation = head.hand.incarnations + 1;
        let client_token = format!("{}-{incarnation}", self.session_id);
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
        let restore = self.restore_source(head).await?;
        let restored = restore.is_some();
        let now = crate::wall_ms();
        head.hand = HandDoc {
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
        let hello = self.hello(&client, head, restore).await?;
        head.hand.generation_id = Some((*hello.generation_id).to_string());

        // First boot of a session with seed files and no sync yet: apply them now; the first
        // sync makes them durable.
        if !restored && !head.prefix.seed_files.is_empty() {
            self.apply_seeds(&client, &head.prefix.seed_files).await?;
        }
        self.live = Some(Live {
            hand,
            client: Arc::new(client),
        });
        Ok(())
    }

    async fn hello(
        &self,
        client: &HandClient,
        head: &HeadDoc,
        restore: Option<RestoreSource>,
    ) -> Result<HelloResponse> {
        let token = head
            .hand
            .session_token
            .clone()
            .ok_or_else(|| BrainError::HandUnavailable("no session token for hello".into()))?;
        let req = HelloRequest {
            protocol: ProtocolVersion::CURRENT,
            session_id: self
                .session_id
                .parse()
                .map_err(|_| BrainError::Invalid("session id".into()))?,
            session_token: token,
            expected_generation_id: head
                .hand
                .generation_id
                .as_deref()
                .and_then(|g| g.parse().ok()),
            tool_manifest_digest: Some(
                head.manifest_digest
                    .parse()
                    .map_err(|_| BrainError::Invalid("manifest digest".into()))?,
            ),
            env: head.prefix.env.clone(),
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
    async fn restore_source(&self, head: &HeadDoc) -> Result<Option<RestoreSource>> {
        let Some(manifest_id) = head.sync.manifest_id.clone() else {
            return Ok(None);
        };
        let manifest_key = sync_manifest_key(&self.session_id, &manifest_id);
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
                    .presign_get(&sync_pack_key(&self.session_id, &p.pack_id))
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

    /// One workspace sync. `reason` is advisory to the hand; `full` repacks everything (the
    /// brain asks for it when the pack chain has grown past the configured bound).
    pub async fn sync(
        &mut self,
        head: &mut HeadDoc,
        reason: SyncReason,
    ) -> Result<Option<SyncDone>> {
        let Some(client) = self.client() else {
            return Ok(None);
        };
        let full = head.sync.packs_referenced >= self.plane.cfg.full_sync_after_packs;
        let manifest_id = crate::mint_id("m", 16);
        let pack_id = crate::mint_id("p", 16);
        let req = SyncRequest {
            reason,
            manifest_id: manifest_id
                .parse()
                .map_err(|_| BrainError::Invalid("manifest id".into()))?,
            manifest_put_url: self
                .plane
                .presign_put(&sync_manifest_key(&self.session_id, &manifest_id))
                .await?,
            pack_id: pack_id
                .parse()
                .map_err(|_| BrainError::Invalid("pack id".into()))?,
            pack_put_url: self
                .plane
                .presign_put(&sync_pack_key(&self.session_id, &pack_id))
                .await?,
            full,
        };
        let resp = client
            .sync(req)
            .await
            .map_err(|e| BrainError::HandUnavailable(format!("sync: {e}")))?;
        let done = SyncDone {
            manifest_id: (*resp.manifest_id).to_string(),
            changed: resp.changed,
            packs_referenced: resp.packs_referenced,
            synced_ms: crate::wall_ms(),
        };
        // Even a no-change sync writes a fresh manifest under the new id; track it so restore
        // always follows the newest manifest.
        head.sync.manifest_id = Some(done.manifest_id.clone());
        head.sync.synced_ms = Some(done.synced_ms);
        head.sync.packs_referenced = done.packs_referenced;
        head.sync.bytes_total = resp.bytes_total;
        Ok(Some(done))
    }

    /// True when this incarnation is close enough to the wall that the brain must sync and
    /// release now.
    pub fn wall_due(&self, head: &HeadDoc) -> bool {
        match head.hand.wall_deadline_ms {
            Some(deadline) => {
                crate::wall_ms() + self.plane.cfg.wall_margin_seconds * 1000 >= deadline
            }
            None => false,
        }
    }

    /// Syncs and terminates the incarnation (wall approach, `end`, or delete). The workspace
    /// survives in S3; the next `ensure_ready` re-materialises.
    pub async fn release(&mut self, head: &mut HeadDoc, sync_first: bool) -> Result<()> {
        self.keepalive = None;
        if sync_first && self.live.is_some() {
            // Best effort: a hand at the wall may die under us; the last turn-end sync then
            // remains the restore point.
            if let Err(e) = self.sync(head, SyncReason::BeforeRelease).await {
                tracing::warn!(session = %self.session_id, error = %e, "pre-release sync failed");
            }
        }
        if let Some(vm) = head.hand.microvm_id.clone() {
            match self.plane.control.terminate(&vm).await {
                Ok(()) => {}
                Err(hand_lambda::control::ControlError::Gone(_)) => {}
                Err(e) => {
                    tracing::warn!(session = %self.session_id, error = %e, "terminate failed")
                }
            }
        }
        self.live = None;
        head.hand.state = "released".into();
        head.hand.microvm_id = None;
        head.hand.endpoint = None;
        head.hand.generation_id = None;
        head.hand.session_token = None;
        head.hand.wall_deadline_ms = None;
        Ok(())
    }

    /// Drops the live connection without touching the VM (idle discard: the hand stays up and
    /// AWS suspends it; the journal HEAD keeps everything needed to come back).
    pub fn disconnect(&mut self) {
        self.keepalive = None;
        self.live = None;
    }

    /// Marks the hand lost in the head. The caller journals the `hand_lost` record with the
    /// interrupted call ids (never replayed, per D7).
    pub fn mark_lost(&mut self, head: &mut HeadDoc) {
        self.keepalive = None;
        self.live = None;
        head.hand.state = "lost".into();
        head.hand.microvm_id = None;
        head.hand.endpoint = None;
        head.hand.generation_id = None;
        head.hand.session_token = None;
        head.hand.wall_deadline_ms = None;
    }
}

fn launched_from_doc(doc: &HandDoc) -> Option<LaunchedHand> {
    // The speculative probe can run with an expired JWE? No: it needs a valid one. The probe
    // path mints nothing; it only fires when we still hold a live endpoint + id, and auth is
    // re-minted on the actual reconnect. An unauthenticated probe still reaches the endpoint
    // and triggers the resume, so a stale token here is acceptable for its purpose.
    Some(LaunchedHand {
        microvm_id: doc.microvm_id.clone()?,
        endpoint: doc.endpoint.clone()?,
        auth_token: String::new(),
    })
}

/// The contract-facing `HandInfo` snapshot for `session.updated` and `GET /sessions/{id}`.
pub fn hand_info(head: &HeadDoc) -> aex_contracts::session::HandInfo {
    use aex_contracts::session::{HandShape, HandState};
    let state = match head.hand.state.as_str() {
        "ready" => HandState::Ready,
        "suspended" => HandState::Suspended,
        "released" => HandState::Released,
        "lost" => HandState::Lost,
        _ => HandState::Preparing,
    };
    aex_contracts::session::HandInfo {
        generation: Some(head.hand.incarnations),
        last_sync_at: head.sync.synced_ms.map(crate::events::ts),
        live_jobs: Some(0),
        shape: match head.prefix.shape.as_str() {
            "2gb" => HandShape::X2gb,
            "4gb" => HandShape::X4gb,
            "8gb" => HandShape::X8gb,
            _ => HandShape::X1gb,
        },
        started_at: head.hand.launched_ms.map(crate::events::ts),
        state,
        wall_deadline_at: head.hand.wall_deadline_ms.map(crate::events::ts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s3_keys_are_scoped_per_session() {
        assert_eq!(
            sync_manifest_key("ses_a", "m_1"),
            "sessions/ses_a/sync/m_1.json"
        );
        assert_eq!(
            sync_pack_key("ses_a", "p_1"),
            "sessions/ses_a/sync/p_1.tar.zst"
        );
        assert_eq!(seed_key("ses_a", 3), "sessions/ses_a/seed/0003");
        assert_eq!(
            artifact_key("ses_a", "dist.tar"),
            "sessions/ses_a/artifacts/dist.tar"
        );
    }

    #[test]
    fn hand_info_maps_states() {
        let mut head = crate::journal::HeadDoc {
            state: "idle".into(),
            failure: None,
            turn: None,
            turns: 0,
            created_ms: 0,
            updated_ms: 0,
            last_message_ms: None,
            ended: false,
            prefix: crate::journal::PrefixDoc {
                system_prompt: None,
                provider: "anthropic".into(),
                model: "m".into(),
                base_url: None,
                max_output_tokens: None,
                temperature: None,
                reasoning_effort: None,
                tools: vec![],
                hand_enabled: true,
                shape: "1gb".into(),
                sync_interval_seconds: 600,
                env: Default::default(),
                metadata: Default::default(),
                seed_files: vec![],
            },
            key_b64: String::new(),
            manifest_digest: String::new(),
            hand: HandDoc::default(),
            sync: Default::default(),
            artifacts: vec![],
        };
        head.hand.state = "lost".into();
        assert_eq!(
            hand_info(&head).state,
            aex_contracts::session::HandState::Lost
        );
        head.hand.state = "".into();
        assert_eq!(
            hand_info(&head).state,
            aex_contracts::session::HandState::Preparing
        );
    }
}
