//! Brain's neutral server entry point. Production is the default and fails closed because hosted
//! adapters are injected by the product composition. `local` is explicit and durable.

use brain::api::{AppState, serve};
use brain::journal::Journal;
use brain::keys::{KeyCustody, blob_from_b64};
use brain::session::BrainConfig;
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

/// The two runtime compositions. Production is the default when omitted and fails closed
/// because hosted adapters are injected by the product composition, never here.
#[derive(Debug, Clone, Copy)]
enum BrainMode {
    Production,
    Local,
}

fn brain_mode() -> anyhow::Result<BrainMode> {
    match std::env::var("BRAIN_MODE") {
        Err(_) => Ok(BrainMode::Production),
        Ok(value) if value == "production" => Ok(BrainMode::Production),
        Ok(value) if value == "local" => Ok(BrainMode::Local),
        Ok(other) => anyhow::bail!("unsupported BRAIN_MODE={other}; use production or local"),
    }
}

async fn run() -> anyhow::Result<()> {
    let mode = brain_mode()?;
    let address: std::net::SocketAddr = std::env::var("BRAIN_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3210".into())
        .parse()?;
    let data =
        PathBuf::from(std::env::var("BRAIN_DATA_DIR").unwrap_or_else(|_| "./brain-data".into()));
    std::fs::create_dir_all(&data)?;
    let token = operator_token(&data)?;
    let brain = match mode {
        BrainMode::Production => anyhow::bail!(
            "the neutral brain-server has no production adapters; start the hosted composition or set BRAIN_MODE=local explicitly"
        ),
        BrainMode::Local => {
            tracing::warn!(data = %data.display(), "LOCAL MODE: durable SQLite/custody with unsandboxed host Tool execution; network policy is not enforced");
            let cfg = BrainConfig::from_env().map_err(|error| anyhow::anyhow!("{error}"))?;
            let websocket_url = std::env::var("BRAIN_CUSTOMER_HAND_WEBSOCKET_URL").ok();
            let observation_base_url =
                std::env::var("BRAIN_CUSTOMER_HAND_OBSERVATION_BASE_URL").ok();
            let transport_urls = match (websocket_url, observation_base_url) {
                (Some(ws), Some(observe)) => Some((ws, observe)),
                (None, None) => None,
                _ => anyhow::bail!(
                    "set both BRAIN_CUSTOMER_HAND_WEBSOCKET_URL and BRAIN_CUSTOMER_HAND_OBSERVATION_BASE_URL or neither"
                ),
            };
            let loophost = match std::env::var("BRAIN_LOOPHOST_AEX_COMPONENT") {
                Err(_) => None,
                Ok(component) => Some(brain_server::LoophostOptions {
                    aex_component: component.into(),
                    toolchain_dir: std::env::var("BRAIN_LOOPHOST_TOOLCHAIN_DIR")
                        .map_err(|_| anyhow::anyhow!(
                            "BRAIN_LOOPHOST_TOOLCHAIN_DIR is required with BRAIN_LOOPHOST_AEX_COMPONENT"
                        ))?
                        .into(),
                }),
            };
            let brain = brain_server::compose_local(brain_server::LocalOptions {
                data_dir: data.clone(),
                cfg,
                advertised_address: address.to_string(),
                transport_urls,
                provider_factory: None,
                loophost,
            })?;
            audit_local(&brain.journal, &brain.custody).await?;
            brain
        }
    };
    serve(
        AppState {
            brain,
            token,
            tenancy: brain::api::Tenancy::Implicit("local".into()),
        },
        address,
    )
    .await
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
