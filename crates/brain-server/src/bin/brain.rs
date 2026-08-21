//! Brain's neutral server entry point. Production is the default and fails closed because hosted
//! adapters are injected by the product composition. `local` is explicit and durable.

use brain::api::{AppState, serve};
use brain::journal::Journal;
use brain::keys::{KeyCustody, blob_from_b64};
use brain::session::{Brain, BrainConfig};
use brain_standalone::durable_local_parts;
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
    let mode = std::env::var("BRAIN_MODE").unwrap_or_else(|_| "production".into());
    let address: std::net::SocketAddr = std::env::var("BRAIN_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3210".into())
        .parse()?;
    let data =
        PathBuf::from(std::env::var("BRAIN_DATA_DIR").unwrap_or_else(|_| "./brain-data".into()));
    std::fs::create_dir_all(&data)?;
    let token = operator_token(&data)?;
    let brain = match mode.as_str() {
        "production" => anyhow::bail!(
            "the neutral brain-server has no production adapters; start the hosted composition or set BRAIN_MODE=local explicitly"
        ),
        "local" => {
            let parts = durable_local_parts(&data).map_err(|error| anyhow::anyhow!("{error}"))?;
            audit_local(&parts.journal, &parts.custody).await?;
            tracing::warn!(data = %data.display(), "LOCAL MODE: durable SQLite/custody with unsandboxed host Tool execution; network policy is not enforced");
            let websocket_url = std::env::var("BRAIN_CUSTOMER_HAND_WEBSOCKET_URL")
                .unwrap_or_else(|_| format!("ws://{address}/v1/customer-hand/socket"));
            let observation_base_url = std::env::var("BRAIN_CUSTOMER_HAND_OBSERVATION_BASE_URL")
                .unwrap_or_else(|_| format!("http://{address}"));
            let customer_transport =
                brain::customer::CustomerTransportConfig::new(websocket_url, observation_base_url)?;
            let local_hand = parts.local_hand.clone();
            let brain = Brain::with_parts_and_services(
                BrainConfig::from_env().map_err(|error| anyhow::anyhow!("{error}"))?,
                parts.journal,
                parts.custody,
                Arc::new(brain::adapter::DisabledToolExecutor),
                brain::session::BrainServices {
                    session_storage: Some(parts.session_storage),
                    bundle_storage: Some(parts.bundle_storage),
                    hand: Some(parts.hand),
                    session_preparation: Some(parts.session_preparation),
                    sandbox_files: Some(parts.sandbox_files),
                    sandbox_control: Some(parts.sandbox_control),
                    customer_delivery: None,
                    customer_transport: Some(customer_transport),
                    compactor: None,
                    agentloop: None,
                },
                None,
            );
            local_hand
                .attach_secret_delivery(brain.clone())
                .map_err(|error| {
                    anyhow::anyhow!("local Hand secret delivery: {}", error.message.as_str())
                })?;
            brain
        }
        other => anyhow::bail!("unsupported BRAIN_MODE={other}; use production or local"),
    };
    serve(AppState { brain, token }, address).await
}

async fn audit_local(journal: &Journal, custody: &Arc<dyn KeyCustody>) -> anyhow::Result<()> {
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
        for (label, encoded) in [("Hand environment", head.doc.hand_secrets_b64.as_str())] {
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
    }
    if !heads.is_empty() {
        tracing::info!(sessions = heads.len(), "local durable-state audit passed");
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
                "created the local operator token; read the protected file to retrieve it"
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
