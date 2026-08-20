//! Brain's neutral server entry point. The durable standalone composition is the default; the
//! explicitly named development mode retains in-memory/local-subprocess adapters for tests.

use brain::api::{AppState, serve};
use brain::journal::Journal;
use brain::keys::{KeyCustody, blob_from_b64};
use brain::session::{Brain, BrainConfig};
use brain_standalone::{DockerConfig, DockerHandFactory, LocalKeyCustody, SqliteStore};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,hyper=warn".into()),
        )
        .init();
    tokio::runtime::Runtime::new()?.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let mode = std::env::var("BRAIN_MODE").unwrap_or_else(|_| "standalone".into());
    let address: std::net::SocketAddr = std::env::var("BRAIN_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3210".into())
        .parse()?;
    let data =
        PathBuf::from(std::env::var("BRAIN_DATA_DIR").unwrap_or_else(|_| "./brain-data".into()));
    std::fs::create_dir_all(&data)?;
    let token = operator_token(&data)?;
    let brain = match mode.as_str() {
        "standalone" => {
            let image = std::env::var("BRAIN_HAND_IMAGE").map_err(|_| {
                anyhow::anyhow!(
                    "BRAIN_HAND_IMAGE is required in standalone mode and must name a compatible immutable Hand image"
                )
            })?;
            if image.trim().is_empty() {
                anyhow::bail!("BRAIN_HAND_IMAGE cannot be empty");
            }
            let mut docker = DockerConfig::new(&data, image);
            if let Ok(executable) = std::env::var("BRAIN_DOCKER_BIN") {
                if executable.trim().is_empty() {
                    anyhow::bail!("BRAIN_DOCKER_BIN cannot be empty");
                }
                docker.executable = executable.into();
            }
            if let Ok(network) = std::env::var("BRAIN_DOCKER_NETWORK") {
                if network.trim().is_empty() {
                    anyhow::bail!("BRAIN_DOCKER_NETWORK cannot be empty");
                }
                docker.network = Some(network);
            }
            let hands = Arc::new(DockerHandFactory::new(docker));
            hands
                .verify()
                .await
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            let store = Arc::new(
                SqliteStore::open(data.join("journal.sqlite3"))
                    .map_err(|error| anyhow::anyhow!("{error}"))?,
            );
            let custody = Arc::new(
                LocalKeyCustody::open(data.join("master.key"))
                    .map_err(|error| anyhow::anyhow!("{error}"))?,
            );
            let owner = format!("brain-{}", brain::mint_id("node", 16));
            let journal = Journal::new(store, owner);
            audit_standalone(&journal, &custody, &hands).await?;
            tracing::info!(data = %data.display(), "standalone mode: SQLite journal and Docker Hands");
            Brain::with_parts(BrainConfig::default(), journal, custody, hands, None)
        }
        "development" => {
            tracing::warn!(
                "DEVELOPMENT MODE: in-memory journal and host subprocess tools; sessions do not survive restart and tools are not sandboxed"
            );
            Brain::local(&data, BrainConfig::default())
                .map_err(|error| anyhow::anyhow!("{error}"))?
        }
        other => anyhow::bail!("unsupported BRAIN_MODE={other}; use standalone or development"),
    };
    serve(AppState { brain, token }, address).await
}

async fn audit_standalone(
    journal: &Journal,
    custody: &Arc<LocalKeyCustody>,
    hands: &Arc<DockerHandFactory>,
) -> anyhow::Result<()> {
    let heads = journal.list_sessions(1_000_000).await?;
    for head in &heads {
        custody
            .decrypt(&head.session_id, &blob_from_b64(&head.doc.key_b64)?)
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "cannot decrypt provider custody for {}: {error}",
                    head.session_id
                )
            })?;
        for (label, encoded) in [
            ("MCP", head.doc.mcp_secrets_b64.as_str()),
            ("Hand environment", head.doc.hand_secrets_b64.as_str()),
        ] {
            if encoded.is_empty() {
                continue;
            }
            custody
                .decrypt(&head.session_id, &blob_from_b64(encoded)?)
                .await
                .map_err(|error| {
                    anyhow::anyhow!(
                        "cannot decrypt {label} custody for {}: {error}",
                        head.session_id
                    )
                })?;
        }
        hands
            .verify_session_state(&head.session_id, &head.doc)
            .map_err(|error| {
                anyhow::anyhow!(
                    "cannot reopen durable state for {}: {error}",
                    head.session_id
                )
            })?;
    }
    if !heads.is_empty() {
        tracing::info!(
            sessions = heads.len(),
            "standalone durable-state audit passed"
        );
    }
    Ok(())
}

fn operator_token(data: &Path) -> anyhow::Result<String> {
    match std::env::var("BRAIN_API_TOKEN") {
        Ok(value) if !value.is_empty() => return Ok(value),
        Ok(_) => anyhow::bail!("BRAIN_API_TOKEN cannot be empty"),
        Err(_) => {}
    }
    let path = data.join("operator.token");
    if path.exists() {
        let mut value = String::new();
        secure_read(&path)?.read_to_string(&mut value)?;
        let value = value.trim().to_string();
        if value.is_empty() {
            anyhow::bail!("{} is empty", path.display());
        }
        return Ok(value);
    }
    let value = brain::mint_id("tok", 40);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(value.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            tracing::warn!(
                path = %path.display(),
                "created the standalone operator token; read the protected file to retrieve it"
            );
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let mut value = String::new();
            secure_read(&path)?.read_to_string(&mut value)?;
            Ok(value.trim().to_string())
        }
        Err(error) => Err(error.into()),
    }
}

fn secure_read(path: &Path) -> anyhow::Result<std::fs::File> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        anyhow::bail!("{} must be a regular non-symlink file", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            anyhow::bail!("{} must have 0600 permissions", path.display());
        }
        Ok(std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?)
    }
    #[cfg(not(unix))]
    Ok(std::fs::File::open(path)?)
}
