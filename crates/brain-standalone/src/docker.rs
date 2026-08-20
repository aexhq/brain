//! Docker Hand adapter for the standalone server.
//!
//! The implementation speaks only Brain's public Hand protocol. The configured image may be the
//! official Hands image or any compatible image; this crate has no dependency on Hands code.

use async_trait::async_trait;
use base64::Engine as _;
use brain::adapter::{
    ArtifactMeta, CallOutcome, CallRequest, HandAdapter, HandFactory, HandSpec, LostReport,
    OutputSink, SeedFile, ToolBundleFile, WorkspaceFile, WorkspaceListing,
};
use brain::journal::MAX_RECORD_CONTENT_BYTES;
use brain::{BrainError, Result};
use brain_hand_client::{ClientError, HandClient};
use brain_protocol::abi::{
    CancelRequest, Cursor, HelloRequest, LaneMode, LaneRef, OperationStatus, PollRequest,
    ProtocolVersion, ReleaseRequest, Stream as AbiStream, SyncScope, ToolExecutableSource,
};
use brain_protocol::session::{
    FileEntry, FileEntryKind, FileListSource, HandInfo, HandShape, HandState,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

const HAND_PORT: &str = "8080/tcp";
const START_WAIT_MS: u64 = 250;
const START_MAX_BYTES: u64 = 256 * 1024;
const POLL_WAIT_MS: u64 = 20_000;
const POLL_MAX_BYTES: u64 = 256 * 1024;
const CANCEL_GRACE_MS: u64 = 2_000;

#[derive(Debug, Clone)]
pub struct DockerConfig {
    pub executable: PathBuf,
    pub image: String,
    pub data_dir: PathBuf,
    pub startup_timeout: Duration,
    /// Optional pre-existing Docker network. When set, Brain connects to each Hand by container
    /// name and no host port is published (the Compose path uses this).
    pub network: Option<String>,
}

impl DockerConfig {
    pub fn new(data_dir: impl Into<PathBuf>, image: impl Into<String>) -> Self {
        Self {
            executable: PathBuf::from("docker"),
            image: image.into(),
            data_dir: data_dir.into(),
            startup_timeout: Duration::from_secs(45),
            network: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DockerState {
    v: u8,
    container_name: String,
    image_id: String,
    status: String,
    incarnations: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started_ms: Option<u64>,
}

impl DockerState {
    fn initial(session_id: &str, image_id: String) -> Self {
        let digest = hex::encode(Sha256::digest(session_id.as_bytes()));
        Self {
            v: 1,
            container_name: format!("brain-hand-{}", &digest[..20]),
            image_id,
            status: "prepared".into(),
            incarnations: 0,
            generation_id: None,
            started_ms: None,
        }
    }
}

#[derive(Clone)]
pub struct DockerHandFactory {
    config: DockerConfig,
    resolved_image_id: Arc<OnceLock<String>>,
}

impl DockerHandFactory {
    pub fn new(config: DockerConfig) -> Self {
        Self {
            config,
            resolved_image_id: Arc::new(OnceLock::new()),
        }
    }

    /// Fails closed at server startup when Docker or the configured immutable image is absent.
    pub async fn verify(&self) -> Result<()> {
        let version = self
            .run([
                OsString::from("version"),
                OsString::from("--format"),
                OsString::from("{{.Server.Version}}"),
            ])
            .await?;
        require_success("query Docker server", &version)?;
        let image = self
            .run([
                OsString::from("image"),
                OsString::from("inspect"),
                OsString::from("--format"),
                OsString::from("{{.Id}}"),
                OsString::from(&self.config.image),
            ])
            .await?;
        require_success("inspect configured Hand image", &image)?;
        let image_id = String::from_utf8_lossy(&image.stdout).trim().to_string();
        validate_image_id(&image_id)?;
        if let Some(existing) = self.resolved_image_id.get() {
            if existing != &image_id {
                return Err(BrainError::HandUnavailable(
                    "configured Hand image identity changed while Brain was running".into(),
                ));
            }
        } else {
            let _ = self.resolved_image_id.set(image_id);
        }
        Ok(())
    }

    fn image_id(&self) -> Result<String> {
        self.resolved_image_id.get().cloned().ok_or_else(|| {
            BrainError::HandUnavailable(
                "Docker Hand factory was used before its image identity was verified".into(),
            )
        })
    }

    /// Audit every durable byte needed to reopen a session before the server accepts traffic.
    /// A missing bundle or changed image is a startup error, never a lazy fallback.
    pub fn verify_session_state(
        &self,
        session_id: &str,
        doc: &brain::journal::HeadDoc,
    ) -> Result<()> {
        let root = self.session_dir(session_id)?;
        if !root.is_dir() {
            return Err(BrainError::HandUnavailable(format!(
                "durable Hand directory is missing for {session_id}"
            )));
        }
        read_token(&root.join("hand.env"))?;
        let state: DockerState = serde_json::from_value(doc.hand_state.clone())
            .map_err(|error| BrainError::Hand(format!("Docker Hand state: {error}")))?;
        let expected = DockerState::initial(session_id, self.image_id()?);
        if state.v != expected.v
            || state.container_name != expected.container_name
            || state.image_id != expected.image_id
        {
            return Err(BrainError::HandUnavailable(format!(
                "durable Hand image or identity changed for {session_id}"
            )));
        }
        if brain::tools::manifest_digest(&doc.hand_manifest) != doc.manifest_digest {
            return Err(BrainError::Hand(format!(
                "stored Hand manifest digest is invalid for {session_id}"
            )));
        }
        for tool in &doc.hand_manifest.tools {
            if tool.executable.source != ToolExecutableSource::Bundle {
                continue;
            }
            let checksum = tool.executable.checksum.to_string();
            let path = root.join("incoming").join(format!("{checksum}.mjs"));
            let bytes = std::fs::read(&path).map_err(|error| {
                BrainError::HandUnavailable(format!(
                    "stored Tool bundle {checksum} is unavailable for {session_id}: {error}"
                ))
            })?;
            if hex::encode(Sha256::digest(&bytes)) != checksum {
                return Err(BrainError::Hand(format!(
                    "stored Tool bundle {checksum} is corrupt for {session_id}"
                )));
            }
        }
        for artifact in &doc.artifacts {
            let name = safe_artifact_name(&artifact.location)?;
            let path = root.join("artifacts").join(name);
            let bytes = std::fs::read(&path).map_err(|error| {
                BrainError::HandUnavailable(format!(
                    "artifact {} is unavailable for {session_id}: {error}",
                    artifact.name
                ))
            })?;
            if bytes.len() as u64 != artifact.bytes
                || hex::encode(Sha256::digest(&bytes)) != artifact.sha256
            {
                return Err(BrainError::Hand(format!(
                    "artifact {} is corrupt for {session_id}",
                    artifact.name
                )));
            }
        }
        Ok(())
    }

    async fn run<I>(&self, args: I) -> Result<Output>
    where
        I: IntoIterator<Item = OsString>,
    {
        tokio::process::Command::new(&self.config.executable)
            .args(args)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| BrainError::HandUnavailable(format!("run Docker CLI: {error}")))
    }

    fn sessions_dir(&self) -> PathBuf {
        self.config.data_dir.join("sessions")
    }

    fn session_dir(&self, session_id: &str) -> Result<PathBuf> {
        validate_session_id(session_id)?;
        Ok(self.sessions_dir().join(session_id))
    }
}

#[async_trait]
impl HandFactory for DockerHandFactory {
    async fn create(
        &self,
        spec: &HandSpec,
        seeds: &[SeedFile<'_>],
        bundles: &[ToolBundleFile<'_>],
    ) -> Result<serde_json::Value> {
        let sessions = self.sessions_dir();
        std::fs::create_dir_all(&sessions)
            .map_err(|error| BrainError::Hand(format!("create sessions directory: {error}")))?;
        secure_parent(&sessions)?;
        let root = self.session_dir(&spec.session_id)?;
        std::fs::create_dir(&root).map_err(|error| {
            BrainError::Invalid(format!(
                "standalone Hand state already exists or cannot be created for {}: {error}",
                spec.session_id
            ))
        })?;
        for name in ["workspace", "home", "ops", "tools", "incoming", "artifacts"] {
            let path = root.join(name);
            std::fs::create_dir(&path)
                .map_err(|error| BrainError::Hand(format!("create Hand {name}: {error}")))?;
            container_writable(&path)?;
        }

        let result = (|| -> Result<()> {
            let workspace = root.join("workspace");
            for seed in seeds {
                let path = resolve_for_write(&workspace, seed.path)?;
                atomic_write(&path, seed.bytes, seed.mode.map(|mode| mode as u32))?;
            }
            let incoming = root.join("incoming");
            let mut seen = HashSet::new();
            for bundle in bundles {
                if !seen.insert(bundle.checksum) {
                    return Err(BrainError::Invalid(format!(
                        "duplicate staged bundle {}",
                        bundle.checksum
                    )));
                }
                let actual = hex::encode(Sha256::digest(bundle.bytes));
                if actual != bundle.checksum {
                    return Err(BrainError::Invalid(format!(
                        "staged bundle {} failed checksum verification",
                        bundle.checksum
                    )));
                }
                atomic_write(
                    &incoming.join(format!("{}.mjs", bundle.checksum)),
                    bundle.bytes,
                    Some(0o444),
                )?;
            }
            write_token_env(&root.join("hand.env"))?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_dir_all(&root);
            return Err(error);
        }
        serde_json::to_value(DockerState::initial(&spec.session_id, self.image_id()?))
            .map_err(BrainError::from)
    }

    async fn open(
        &self,
        spec: &HandSpec,
        state: serde_json::Value,
    ) -> Result<Arc<dyn HandAdapter>> {
        let root = self.session_dir(&spec.session_id)?;
        if !root.is_dir() || !root.join("hand.env").is_file() {
            return Err(BrainError::HandUnavailable(format!(
                "durable Hand state is missing for {}",
                spec.session_id
            )));
        }
        let state: DockerState = serde_json::from_value(state)
            .map_err(|error| BrainError::Hand(format!("Docker Hand state: {error}")))?;
        let expected = DockerState::initial(&spec.session_id, self.image_id()?);
        if state.v != 1
            || state.container_name != expected.container_name
            || state.image_id != expected.image_id
        {
            return Err(BrainError::Hand(
                "invalid Docker Hand state identity".into(),
            ));
        }
        Ok(Arc::new(DockerHand {
            factory: self.clone(),
            spec: spec.clone(),
            root,
            state: Mutex::new(state),
            client: AsyncMutex::new(None),
            lifecycle: AsyncMutex::new(()),
        }))
    }

    async fn purge(&self, session_id: &str) -> Result<()> {
        let root = self.session_dir(session_id)?;
        let container_name = DockerState::initial(session_id, String::new()).container_name;
        let output = self
            .run([
                OsString::from("rm"),
                OsString::from("--force"),
                OsString::from(container_name),
            ])
            .await?;
        if !output.status.success() && !stderr_text(&output).contains("No such container") {
            require_success("remove Docker Hand", &output)?;
        }
        remove_exact_session_dir(&self.sessions_dir(), &root)?;
        Ok(())
    }

    async fn artifact_url(&self, session_id: &str, location: &str) -> Option<String> {
        let root = self.session_dir(session_id).ok()?;
        let name = safe_artifact_name(location).ok()?;
        let path = root.join("artifacts").join(name);
        path.is_file().then(|| file_url(&path))
    }
}

struct DockerHand {
    factory: DockerHandFactory,
    spec: HandSpec,
    root: PathBuf,
    state: Mutex<DockerState>,
    client: AsyncMutex<Option<Arc<HandClient>>>,
    lifecycle: AsyncMutex<()>,
}

impl DockerHand {
    fn snapshot(&self) -> DockerState {
        self.state.lock().expect("Docker Hand state").clone()
    }

    fn merge(&self, update: impl FnOnce(&mut DockerState)) {
        update(&mut self.state.lock().expect("Docker Hand state"));
    }

    async fn inspect_running(&self) -> Result<Option<bool>> {
        let name = self.snapshot().container_name;
        let output = self
            .factory
            .run([
                OsString::from("inspect"),
                OsString::from("--format"),
                OsString::from("{{.State.Running}}"),
                OsString::from(name),
            ])
            .await?;
        if output.status.success() {
            return Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim() == "true",
            ));
        }
        if stderr_text(&output).contains("No such") {
            Ok(None)
        } else {
            require_success("inspect Docker Hand", &output)?;
            unreachable!()
        }
    }

    async fn launch(&self) -> Result<()> {
        let state = self.snapshot();
        let mounts = [
            (self.root.join("workspace"), "/workspace", false),
            (self.root.join("home"), "/home/agent", false),
            (self.root.join("ops"), "/var/hand/ops", false),
            (self.root.join("tools"), "/var/hand/tools", false),
            (self.root.join("incoming"), "/var/hand/incoming", true),
        ];
        let mut args = vec![
            OsString::from("run"),
            OsString::from("--detach"),
            OsString::from("--name"),
            OsString::from(&state.container_name),
            OsString::from("--label"),
            OsString::from(format!("dev.brain.hand.session={}", self.spec.session_id)),
            OsString::from("--init"),
            OsString::from("--cap-drop"),
            OsString::from("ALL"),
            OsString::from("--security-opt"),
            OsString::from("no-new-privileges"),
            OsString::from("--pids-limit"),
            OsString::from("512"),
            OsString::from("--memory"),
            OsString::from(memory_limit(&self.spec.shape)?),
            OsString::from("--env-file"),
            self.root.join("hand.env").into_os_string(),
        ];
        if let Some(network) = &self.factory.config.network {
            if network.trim().is_empty() {
                return Err(BrainError::Invalid("Docker network cannot be empty".into()));
            }
            args.push(OsString::from("--network"));
            args.push(OsString::from(network));
        } else {
            args.push(OsString::from("--publish"));
            args.push(OsString::from("127.0.0.1::8080"));
        }
        for (source, target, readonly) in mounts {
            let source = source
                .canonicalize()
                .map_err(|error| BrainError::Hand(format!("canonicalize Hand mount: {error}")))?;
            let source = source.to_string_lossy();
            if source.contains(',') {
                return Err(BrainError::Hand(
                    "standalone data path cannot contain a comma (Docker mount syntax)".into(),
                ));
            }
            args.push(OsString::from("--mount"));
            args.push(OsString::from(format!(
                "type=bind,source={source},target={target}{}",
                if readonly { ",readonly" } else { "" }
            )));
        }
        args.push(OsString::from(&state.image_id));
        let output = self.factory.run(args).await?;
        require_success("launch Docker Hand", &output)?;
        Ok(())
    }

    async fn start_existing(&self) -> Result<()> {
        let output = self
            .factory
            .run([
                OsString::from("start"),
                OsString::from(self.snapshot().container_name),
            ])
            .await?;
        require_success("start Docker Hand", &output)
    }

    async fn mapped_port(&self) -> Result<u16> {
        let output = self
            .factory
            .run([
                OsString::from("port"),
                OsString::from(self.snapshot().container_name),
                OsString::from(HAND_PORT),
            ])
            .await?;
        require_success("query Docker Hand port", &output)?;
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines()
            .find_map(|line| line.trim().rsplit_once(':').map(|(_, port)| port))
            .and_then(|port| port.parse::<u16>().ok())
            .ok_or_else(|| BrainError::Hand("Docker did not report the Hand port".into()))
    }

    async fn connect_and_hello(&self) -> Result<(Arc<HandClient>, bool)> {
        let deadline = Instant::now() + self.factory.config.startup_timeout;
        let url = if self.factory.config.network.is_some() {
            format!("ws://{}:8080/", self.snapshot().container_name)
        } else {
            let port = loop {
                match self.mapped_port().await {
                    Ok(port) => break port,
                    Err(error) if Instant::now() < deadline => {
                        tracing::debug!(session = %self.spec.session_id, %error, "waiting for Docker Hand port");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(error) => return Err(error),
                }
            };
            format!("ws://127.0.0.1:{port}/")
        };
        let client = loop {
            match HandClient::connect(&url, 1).await {
                Ok(client) => break Arc::new(client),
                Err(error) if Instant::now() < deadline => {
                    tracing::debug!(session = %self.spec.session_id, %error, "waiting for Docker Hand protocol");
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
                Err(error) => {
                    return Err(BrainError::HandUnavailable(format!(
                        "Docker Hand did not become ready: {error}"
                    )));
                }
            }
        };
        let token = read_token(&self.root.join("hand.env"))?;
        let previous = self.snapshot().generation_id;
        let mut manifest = self.spec.tool_manifest.clone();
        for tool in &mut manifest.tools {
            if tool.executable.source == ToolExecutableSource::Bundle {
                tool.executable.get_url = Some(format!(
                    "file:///var/hand/incoming/{}.mjs",
                    *tool.executable.checksum
                ));
            }
        }
        let expected_generation_id = previous.as_deref().and_then(|value| value.parse().ok());
        let response = client
            .hello(HelloRequest {
                env: self.spec.env.clone(),
                expected_generation_id,
                heartbeat_ms: 10_000,
                protocol: ProtocolVersion::CURRENT,
                restore: None,
                session_id: self
                    .spec
                    .session_id
                    .parse()
                    .map_err(|error| BrainError::Hand(format!("session id: {error}")))?,
                session_token: token,
                sync: SyncScope {
                    roots: vec!["/workspace".into(), "/home/agent".into()],
                    exclude: vec![],
                },
                tool_manifest: manifest,
                tool_manifest_digest: self
                    .spec
                    .manifest_digest
                    .clone()
                    .try_into()
                    .map_err(|error| BrainError::Hand(format!("manifest digest: {error}")))?,
            })
            .await
            .map_err(|error| BrainError::Hand(format!("Docker Hand hello: {error}")))?;
        if response.tool_manifest_digest.to_string() != self.spec.manifest_digest
            || response.tools != self.spec.tool_manifest.tools
        {
            return Err(BrainError::SessionFailed(
                "Docker Hand acknowledged a different execution seal".into(),
            ));
        }
        let generation = response.generation_id.to_string();
        let lost = previous.as_ref().is_some_and(|prior| prior != &generation);
        self.merge(|state| {
            if state.generation_id.as_deref() != Some(&generation) {
                state.incarnations = state.incarnations.saturating_add(1);
            }
            state.generation_id = Some(generation);
            state.status = "ready".into();
            state.started_ms.get_or_insert_with(brain::wall_ms);
        });
        Ok((client, lost))
    }

    async fn live_client(&self) -> Option<Arc<HandClient>> {
        self.client
            .lock()
            .await
            .as_ref()
            .filter(|client| !client.is_closed())
            .cloned()
    }
}

#[async_trait]
impl HandAdapter for DockerHand {
    async fn ensure_ready(&self) -> Result<Option<LostReport>> {
        if !self.spec.hand_enabled {
            return Ok(None);
        }
        if self.live_client().await.is_some() {
            return Ok(None);
        }
        let _guard = self.lifecycle.lock().await;
        if self.live_client().await.is_some() {
            return Ok(None);
        }
        let had_generation = self.snapshot().generation_id.is_some();
        let missing = match self.inspect_running().await? {
            Some(true) => false,
            Some(false) => {
                self.start_existing().await?;
                false
            }
            None => {
                self.launch().await?;
                true
            }
        };
        let (client, generation_changed) = self.connect_and_hello().await?;
        *self.client.lock().await = Some(client);
        if had_generation && (missing || generation_changed) {
            Ok(Some(LostReport {
                reason: "Docker Hand generation was recreated; unfinished calls were interrupted"
                    .into(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn call(
        &self,
        req: CallRequest,
        cancel: CancellationToken,
        sink: OutputSink,
    ) -> CallOutcome {
        let Some(client) = self.live_client().await else {
            return interrupted("Docker Hand is not connected");
        };
        hand_call(&client, &req, &cancel, &sink).await
    }

    fn idle(&self) {
        if let Ok(mut client) = self.client.try_lock() {
            *client = None;
        }
    }

    async fn checkpoint(&self) -> Result<()> {
        Ok(())
    }

    async fn release(&self) -> Result<()> {
        *self.client.lock().await = None;
        if self.inspect_running().await? == Some(true) {
            let output = self
                .factory
                .run([
                    OsString::from("stop"),
                    OsString::from("--time"),
                    OsString::from("5"),
                    OsString::from(self.snapshot().container_name),
                ])
                .await?;
            require_success("stop Docker Hand", &output)?;
        }
        self.merge(|state| state.status = "released".into());
        Ok(())
    }

    async fn acknowledge(&self, call_ids: &[String]) {
        let Some(client) = self.live_client().await else {
            return;
        };
        let operation_ids = call_ids.iter().filter_map(|id| id.parse().ok()).collect();
        let _ = client.release(ReleaseRequest { operation_ids }).await;
    }

    fn workspace_bytes(&self) -> u64 {
        walkdir::WalkDir::new(self.root.join("workspace"))
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| entry.metadata().ok())
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .sum()
    }

    async fn list_files(&self, path: &str, recursive: bool) -> Result<WorkspaceListing> {
        let workspace = self.root.join("workspace");
        let base = resolve_existing(&workspace, path)?;
        if !base.is_dir() {
            return Err(BrainError::FileNotFound(path.into()));
        }
        let depth = if recursive { usize::MAX } else { 1 };
        let mut entries = Vec::new();
        for entry in walkdir::WalkDir::new(&base)
            .min_depth(1)
            .max_depth(depth)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
        {
            let entry =
                entry.map_err(|error| BrainError::Hand(format!("list workspace: {error}")))?;
            entries.push(file_entry(&workspace, entry.path())?);
        }
        Ok(WorkspaceListing {
            entries,
            source: FileListSource::Hand,
            synced_ms: Some(brain::wall_ms()),
        })
    }

    async fn read_file(&self, path: &str, max_bytes: usize) -> Result<WorkspaceFile> {
        let workspace = self.root.join("workspace");
        let resolved = resolve_existing(&workspace, path)?;
        let metadata =
            std::fs::metadata(&resolved).map_err(|_| BrainError::FileNotFound(path.into()))?;
        if !metadata.is_file() {
            return Err(BrainError::FileNotFound(path.into()));
        }
        if metadata.len() > max_bytes as u64 {
            return Err(BrainError::FileTooLarge { limit: max_bytes });
        }
        let bytes = std::fs::read(&resolved)
            .map_err(|error| BrainError::Hand(format!("read workspace file: {error}")))?;
        if bytes.len() > max_bytes {
            return Err(BrainError::FileTooLarge { limit: max_bytes });
        }
        Ok(WorkspaceFile {
            entry: file_entry(&workspace, &resolved)?,
            bytes,
        })
    }

    async fn write_file(&self, path: &str, bytes: &[u8]) -> Result<FileEntry> {
        let workspace = self.root.join("workspace");
        let resolved = resolve_for_write(&workspace, path)?;
        atomic_write(&resolved, bytes, None)?;
        file_entry(&workspace, &resolved)
    }

    async fn persist(
        &self,
        name: &str,
        path: &str,
        media_type: Option<&str>,
    ) -> Result<ArtifactMeta> {
        let name = safe_artifact_name(name)?;
        let workspace = self.root.join("workspace");
        let source = resolve_existing(&workspace, path)?;
        if !source.is_file() {
            return Err(BrainError::FileNotFound(path.into()));
        }
        let bytes = std::fs::read(&source)
            .map_err(|error| BrainError::Hand(format!("read artifact source: {error}")))?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        atomic_write(&self.root.join("artifacts").join(name), &bytes, Some(0o600))?;
        Ok(ArtifactMeta {
            bytes: bytes.len() as u64,
            sha256,
            media_type: media_type.unwrap_or("application/octet-stream").into(),
            location: name.into(),
        })
    }

    fn hand_info(&self) -> HandInfo {
        let state = self.snapshot();
        HandInfo {
            generation: Some(state.incarnations),
            last_sync_at: None,
            live_jobs: Some(0),
            shape: match self.spec.shape.as_str() {
                "2gb" => HandShape::X2gb,
                "4gb" => HandShape::X4gb,
                "8gb" => HandShape::X8gb,
                _ => HandShape::X1gb,
            },
            started_at: state.started_ms.map(brain::events::ts),
            state: match state.status.as_str() {
                "ready" => HandState::Ready,
                "released" => HandState::Released,
                "lost" => HandState::Lost,
                _ => HandState::Preparing,
            },
            wall_deadline_at: None,
        }
    }

    fn state(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or(serde_json::Value::Null)
    }
}

fn decode_slices(
    slices: &[brain_protocol::abi::OutputSlice],
    stdout: &mut String,
    stderr: &mut String,
    sink: &OutputSink,
) {
    for slice in slices {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&slice.data_base64)
            .unwrap_or_default();
        if bytes.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        match slice.stream {
            AbiStream::Stdout => {
                sink("stdout", slice.offset, text.clone());
                stdout.push_str(&text);
            }
            AbiStream::Stderr => {
                sink("stderr", slice.offset, text.clone());
                stderr.push_str(&text);
            }
        }
    }
}

async fn hand_call(
    client: &HandClient,
    req: &CallRequest,
    cancel: &CancellationToken,
    sink: &OutputSink,
) -> CallOutcome {
    let started_at = Instant::now();
    let lane = if req.parallel {
        LaneRef {
            id: match brain::mint_id("lane", 12).parse() {
                Ok(id) => id,
                Err(_) => return CallOutcome::failed("invalid lane id"),
            },
            mode: LaneMode::Ephemeral,
            parent: Some("0".parse().expect("root lane id")),
        }
    } else {
        brain_hand_client::root_lane()
    };
    let started = match client
        .start(brain_hand_client::start_request(
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
        Ok(started) => started,
        Err(error) => {
            return terminal_transport(
                format!("Hand start failed: {error}"),
                classify_client_error(&error),
                started_at,
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
            if let Ok(operation_id) = req.call_id.parse() {
                let _ = client
                    .cancel(CancelRequest {
                        operation_id,
                        grace_ms: Some(CANCEL_GRACE_MS),
                    })
                    .await;
            }
        }
        let operation_id = match req.call_id.parse() {
            Ok(id) => id,
            Err(_) => return CallOutcome::failed("invalid operation id"),
        };
        match client
            .poll(PollRequest {
                operation_id,
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
            .await
        {
            Ok(poll) => {
                decode_slices(&poll.slices, &mut stdout, &mut stderr, sink);
                view = poll.view;
            }
            Err(error) => {
                return terminal_transport(
                    format!("Hand connection lost during call: {error}"),
                    "interrupted",
                    started_at,
                );
            }
        }
    }
    let terminal = view.terminal.as_ref();
    let outcome = terminal
        .map(|terminal| terminal.outcome.to_string())
        .unwrap_or_else(|| "failed".into());
    let mut content = String::new();
    if let Some(output) = terminal.and_then(|terminal| terminal.output.as_ref()) {
        content.push_str(&output.to_string());
    }
    if !stdout.is_empty() {
        if !content.is_empty() {
            content.push_str("\n[stdout]\n");
        }
        content.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !content.is_empty() {
            content.push_str("\n[stderr]\n");
        }
        content.push_str(&stderr);
    }
    if let Some(error) = terminal.and_then(|terminal| terminal.error.as_ref()) {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&format!("[error] {}: {}", error.code, error.message));
    }
    if content.is_empty() {
        content = format!("Hand call ended with outcome {outcome}");
    }
    let mut truncated = false;
    if content.len() > MAX_RECORD_CONTENT_BYTES {
        let mut start = content.len() - MAX_RECORD_CONTENT_BYTES;
        while !content.is_char_boundary(start) {
            start += 1;
        }
        content = format!(
            "[output truncated: first {start} bytes elided]\n{}",
            &content[start..]
        );
        truncated = true;
    }
    CallOutcome {
        is_error: outcome != "completed",
        value: terminal.and_then(|terminal| terminal.output.clone()),
        outcome,
        content,
        exit_code: terminal.and_then(|terminal| terminal.exit_code),
        duration_ms: started_at.elapsed().as_millis() as u64,
        truncated,
        terminal: None,
    }
}

fn classify_client_error(error: &ClientError) -> &'static str {
    match error {
        ClientError::Closed | ClientError::Timeout(_) | ClientError::Transport(_) => "interrupted",
        ClientError::Abi(_) | ClientError::WrongReply { .. } => "failed",
    }
}

fn terminal_transport(content: String, outcome: &str, started_at: Instant) -> CallOutcome {
    CallOutcome {
        outcome: outcome.into(),
        value: None,
        content,
        is_error: true,
        exit_code: None,
        duration_ms: started_at.elapsed().as_millis() as u64,
        truncated: false,
        terminal: None,
    }
}

fn interrupted(message: &str) -> CallOutcome {
    terminal_transport(message.into(), "interrupted", Instant::now())
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.len() > 64
        || session_id.is_empty()
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(BrainError::Invalid(
            "invalid session id for local storage".into(),
        ));
    }
    Ok(())
}

fn validate_image_id(image_id: &str) -> Result<()> {
    let Some(digest) = image_id.strip_prefix("sha256:") else {
        return Err(BrainError::HandUnavailable(
            "Docker returned a non-SHA256 Hand image identity".into(),
        ));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(BrainError::HandUnavailable(
            "Docker returned an invalid Hand image digest".into(),
        ));
    }
    Ok(())
}

fn memory_limit(shape: &str) -> Result<&'static str> {
    match shape {
        "1gb" => Ok("1g"),
        "2gb" => Ok("2g"),
        "4gb" => Ok("4g"),
        "8gb" => Ok("8g"),
        _ => Err(BrainError::Invalid(format!(
            "unsupported standalone Hand shape {shape}"
        ))),
    }
}

fn safe_relative(path: &str) -> Result<PathBuf> {
    let path = path.strip_prefix("/workspace/").unwrap_or(path);
    if path.is_empty() {
        return Ok(PathBuf::new());
    }
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(BrainError::Invalid(format!(
            "workspace path {path:?} is not allowed"
        )));
    }
    Ok(candidate.to_path_buf())
}

fn resolve_existing(workspace: &Path, path: &str) -> Result<PathBuf> {
    let workspace = workspace
        .canonicalize()
        .map_err(|error| BrainError::Hand(format!("canonicalize workspace: {error}")))?;
    let candidate = workspace.join(safe_relative(path)?);
    let resolved = candidate
        .canonicalize()
        .map_err(|_| BrainError::FileNotFound(path.into()))?;
    if !resolved.starts_with(&workspace) {
        return Err(BrainError::Invalid(
            "workspace path escapes through a symlink".into(),
        ));
    }
    Ok(resolved)
}

fn resolve_for_write(workspace: &Path, path: &str) -> Result<PathBuf> {
    let workspace = workspace
        .canonicalize()
        .map_err(|error| BrainError::Hand(format!("canonicalize workspace: {error}")))?;
    let candidate = workspace.join(safe_relative(path)?);
    let parent = candidate
        .parent()
        .ok_or_else(|| BrainError::Invalid("workspace file has no parent".into()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| BrainError::Hand(format!("create workspace directory: {error}")))?;
    let parent = parent
        .canonicalize()
        .map_err(|error| BrainError::Hand(format!("canonicalize workspace parent: {error}")))?;
    if !parent.starts_with(&workspace) {
        return Err(BrainError::Invalid(
            "workspace path escapes through a symlink".into(),
        ));
    }
    Ok(parent.join(
        candidate
            .file_name()
            .ok_or_else(|| BrainError::Invalid("workspace path must name a file".into()))?,
    ))
}

fn file_entry(workspace: &Path, path: &Path) -> Result<FileEntry> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| BrainError::Hand(format!("workspace metadata: {error}")))?;
    let relative = path
        .strip_prefix(workspace)
        .map_err(|_| BrainError::Invalid("workspace entry escaped its root".into()))?;
    let public = format!(
        "/workspace/{}",
        relative.to_string_lossy().replace('\\', "/")
    );
    let (kind, size, sha256) = if metadata.file_type().is_symlink() {
        (FileEntryKind::Symlink, None, None)
    } else if metadata.is_dir() {
        (FileEntryKind::Dir, None, None)
    } else {
        let bytes = std::fs::read(path)
            .map_err(|error| BrainError::Hand(format!("hash workspace file: {error}")))?;
        (
            FileEntryKind::File,
            Some(bytes.len() as u64),
            hex::encode(Sha256::digest(&bytes)).parse().ok(),
        )
    };
    Ok(FileEntry {
        kind,
        modified_at: None,
        path: public,
        sha256,
        size,
    })
}

fn atomic_write(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| BrainError::Invalid("file path has no parent".into()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| BrainError::Hand(format!("create file directory: {error}")))?;
    let tmp = parent.join(format!(".brain-write-{}", brain::mint_id("tmp", 12)));
    std::fs::write(&tmp, bytes)
        .map_err(|error| BrainError::Hand(format!("write staged file: {error}")))?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))
            .map_err(|error| BrainError::Hand(format!("set file mode: {error}")))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    std::fs::rename(&tmp, path)
        .map_err(|error| BrainError::Hand(format!("commit staged file: {error}")))
}

fn write_token_env(path: &Path) -> Result<()> {
    let token = brain::mint_id("hand", 40);
    atomic_write(
        path,
        format!("HAND_TOKEN={token}\n").as_bytes(),
        Some(0o600),
    )
}

fn read_token(path: &Path) -> Result<String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| BrainError::Hand(format!("read Hand token: {error}")))?;
    text.lines()
        .find_map(|line| line.strip_prefix("HAND_TOKEN="))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BrainError::Hand("Hand token file is invalid".into()))
}

fn safe_artifact_name(name: &str) -> Result<&str> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
    {
        return Err(BrainError::Invalid("invalid artifact name".into()));
    }
    Ok(name)
}

fn file_url(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if value.starts_with('/') {
        format!("file://{value}")
    } else {
        format!("file:///{value}")
    }
}

fn secure_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| BrainError::Hand(format!("secure sessions directory: {error}")))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn container_writable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The parent is operator-only. This lets the unprivileged uid-1000 Hand write bind mounts
        // regardless of the host operator uid without running the container as root.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777))
            .map_err(|error| BrainError::Hand(format!("prepare container mount: {error}")))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn remove_exact_session_dir(base: &Path, target: &Path) -> Result<()> {
    if !target.exists() {
        return Ok(());
    }
    let base = base
        .canonicalize()
        .map_err(|error| BrainError::Hand(format!("canonicalize sessions directory: {error}")))?;
    let target = target
        .canonicalize()
        .map_err(|error| BrainError::Hand(format!("canonicalize session directory: {error}")))?;
    if target.parent() != Some(base.as_path()) {
        return Err(BrainError::Hand(
            "refusing to remove a path outside the sessions directory".into(),
        ));
    }
    std::fs::remove_dir_all(&target)
        .map_err(|error| BrainError::Hand(format!("remove session directory: {error}")))
}

fn stderr_text(output: &Output) -> String {
    let bytes = if output.stderr.len() > 8192 {
        &output.stderr[output.stderr.len() - 8192..]
    } else {
        &output.stderr
    };
    String::from_utf8_lossy(bytes).into_owned()
}

fn require_success(context: &str, output: &Output) -> Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(BrainError::HandUnavailable(format!(
            "{context} failed: {}",
            stderr_text(output).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(brain::mint_id("brain-docker-test", 12));
            std::fs::create_dir_all(path.join("workspace")).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn workspace_paths_never_escape_through_components_or_symlinks() {
        let dir = TestDir::new();
        let workspace = dir.0.join("workspace");
        assert!(resolve_for_write(&workspace, "src/main.ts").is_ok());
        assert!(resolve_for_write(&workspace, "../outside").is_err());
        assert!(resolve_for_write(&workspace, "/etc/passwd").is_err());
    }

    #[test]
    fn session_and_artifact_names_are_storage_safe() {
        assert!(validate_session_id("ses_abc-123").is_ok());
        assert!(validate_session_id("../escape").is_err());
        assert!(safe_artifact_name("report.json").is_ok());
        assert!(safe_artifact_name("../report.json").is_err());
    }

    #[test]
    fn docker_image_identity_is_an_exact_content_digest() {
        assert!(validate_image_id(&format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(validate_image_id("hands:latest").is_err());
        assert!(validate_image_id(&format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(validate_image_id("sha256:abcd").is_err());
    }
}
