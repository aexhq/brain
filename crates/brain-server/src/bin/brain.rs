//! Brain's neutral server entry point. Production composes AWS durability and the four component
//! hosts; `local` is explicit and durable.

use brain::journal::Journal;
use brain::keys::{KeyCustody, blob_from_b64};
use brain::session::BrainConfig;
use brain_server::api::{AppState, serve};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let telemetry = brain_observability::install("brain")?;
    let result = tokio::runtime::Runtime::new()?.block_on(run());
    telemetry.shutdown()?;
    result
}

/// The two runtime compositions. Production is the default when omitted.
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
    let data = PathBuf::from(match (mode, std::env::var("BRAIN_DATA_DIR")) {
        (_, Ok(value)) if !value.is_empty() => value,
        (BrainMode::Local, Err(_)) => "./brain-data".into(),
        (BrainMode::Production, Err(_)) => anyhow::bail!("BRAIN_DATA_DIR is not set"),
        (_, Ok(_)) => anyhow::bail!("BRAIN_DATA_DIR cannot be empty"),
    });
    std::fs::create_dir_all(&data)?;
    let token = match mode {
        BrainMode::Production => required("BRAIN_API_TOKEN")?,
        BrainMode::Local => operator_token(&data)?,
    };
    let brain = match mode {
        BrainMode::Production => {
            let cfg = BrainConfig::from_env().map_err(|error| anyhow::anyhow!("{error}"))?;
            let persistence = brain_aws::AwsPersistenceConfig::from_env()
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            let (customer_delivery, customer_transport) =
                aws_customer_environment(&persistence.region).await?;
            brain_server::compose_aws(brain_server::AwsOptions {
                data_dir: data.clone(),
                cfg,
                persistence,
                environment_capabilities: environment_capabilities()?,
                customer_delivery,
                customer_transport,
                loophost: Some(brain_server::LoophostOptions {
                    component_host: component_host_path()?,
                    workers: component_workers()?,
                }),
            })
            .await?
        }
        BrainMode::Local => {
            tracing::warn!(data = %data.display(), "LOCAL MODE: durable SQLite/custody with unsandboxed host Tool execution; network policy is not enforced");
            let cfg = BrainConfig::from_env().map_err(|error| anyhow::anyhow!("{error}"))?;
            let websocket_url = std::env::var("BRAIN_CUSTOMER_ENVIRONMENT_WEBSOCKET_URL").ok();
            let observation_base_url =
                std::env::var("BRAIN_CUSTOMER_ENVIRONMENT_OBSERVATION_BASE_URL").ok();
            let transport_urls = match (websocket_url, observation_base_url) {
                (Some(ws), Some(observe)) => Some((ws, observe)),
                (None, None) => None,
                _ => anyhow::bail!(
                    "set both BRAIN_CUSTOMER_ENVIRONMENT_WEBSOCKET_URL and BRAIN_CUSTOMER_ENVIRONMENT_OBSERVATION_BASE_URL or neither"
                ),
            };
            let loophost = Some(brain_server::LoophostOptions {
                component_host: component_host_path()?,
                workers: component_workers()?,
            });
            let brain = brain_server::compose_local(brain_server::LocalOptions {
                data_dir: data.clone(),
                cfg,
                advertised_address: address.to_string(),
                transport_urls,
                provider_factory: None,
                environment_capabilities: environment_capabilities()?,
                loophost,
            })
            .await?;
            audit_local(&brain.journal, &brain.custody).await?;
            brain
        }
    };
    serve(
        AppState {
            brain,
            token,
            tenancy: match mode {
                BrainMode::Production => brain_server::api::Tenancy::Required,
                BrainMode::Local => brain_server::api::Tenancy::Implicit("local".into()),
            },
        },
        address,
    )
    .await
}

async fn aws_customer_environment(
    region: &str,
) -> anyhow::Result<(
    Option<Arc<dyn brain::customer::CustomerEnvironmentDeliveryPort>>,
    Option<brain::customer::CustomerTransportConfig>,
)> {
    const WEBSOCKET: &str = "BRAIN_CUSTOMER_ENVIRONMENT_WEBSOCKET_URL";
    const OBSERVATION: &str = "BRAIN_CUSTOMER_ENVIRONMENT_OBSERVATION_BASE_URL";
    const CALLBACK: &str = "BRAIN_CUSTOMER_ENVIRONMENT_CALLBACK_URL";
    let websocket = std::env::var(WEBSOCKET).ok();
    let observation = std::env::var(OBSERVATION).ok();
    let callback = std::env::var(CALLBACK).ok();
    match (websocket, observation, callback) {
        (None, None, None) => Ok((None, None)),
        (Some(websocket), Some(observation), Some(callback))
            if !websocket.is_empty() && !observation.is_empty() && !callback.is_empty() =>
        {
            let transport = brain::customer::CustomerTransportConfig::new(websocket, observation)?;
            let delivery =
                brain_aws::gateway::ApiGatewayCustomerDelivery::new(region, &callback).await?;
            Ok((Some(Arc::new(delivery)), Some(transport)))
        }
        _ => anyhow::bail!(
            "set non-empty {WEBSOCKET}, {OBSERVATION}, and {CALLBACK} together or omit all three"
        ),
    }
}

fn required(name: &str) -> anyhow::Result<String> {
    let value = std::env::var(name).map_err(|_| anyhow::anyhow!("{name} is not set"))?;
    if value.is_empty() {
        anyhow::bail!("{name} cannot be empty");
    }
    Ok(value)
}

fn environment_capabilities()
-> anyhow::Result<Option<Arc<dyn brain_component_host::CapabilityHandler>>> {
    let endpoint = std::env::var("BRAIN_ENVIRONMENT_DISPATCH_URL").ok();
    let token = std::env::var("BRAIN_ENVIRONMENT_DISPATCH_TOKEN").ok();
    let timeout = std::env::var("BRAIN_ENVIRONMENT_DISPATCH_TIMEOUT_MS")
        .unwrap_or_else(|_| "30000".into())
        .parse::<u64>()?;
    if !(100..=900_000).contains(&timeout) {
        anyhow::bail!("BRAIN_ENVIRONMENT_DISPATCH_TIMEOUT_MS must be 100 through 900000");
    }
    match endpoint {
        Some(endpoint) if !endpoint.is_empty() => Ok(Some(Arc::new(
            brain_environment_host::HttpEnvironmentCapabilities::new(
                endpoint,
                token,
                std::time::Duration::from_millis(timeout),
            )?,
        ))),
        Some(_) => anyhow::bail!("BRAIN_ENVIRONMENT_DISPATCH_URL cannot be empty"),
        None if token.is_some() => anyhow::bail!(
            "BRAIN_ENVIRONMENT_DISPATCH_TOKEN requires BRAIN_ENVIRONMENT_DISPATCH_URL"
        ),
        None => Ok(None),
    }
}

fn component_host_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("BRAIN_COMPONENT_HOST_BIN") {
        return Ok(path.into());
    }
    let executable = std::env::current_exe()?;
    let name = if cfg!(windows) {
        "brain-component-host.exe"
    } else {
        "brain-component-host"
    };
    Ok(executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Brain executable has no parent directory"))?
        .join(name))
}

fn component_workers() -> anyhow::Result<usize> {
    let workers = std::env::var("BRAIN_COMPONENT_WORKERS")
        .unwrap_or_else(|_| "4".into())
        .parse::<usize>()?;
    if !(1..=64).contains(&workers) {
        anyhow::bail!("BRAIN_COMPONENT_WORKERS must be 1 through 64");
    }
    Ok(workers)
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
        for (label, encoded) in [(
            "Environment environment",
            head.doc.environment_secrets_b64.as_str(),
        )] {
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
