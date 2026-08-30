use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use brain::{Kernel, KernelConfig};
use brain_loophost::{LoopLimits, WorkerPool};
use brain_server::{
    EnvironmentRegistry, HttpEnvironmentAdapter, InMemoryEnvironmentDirectory,
    LocalModelBindingStore, ServerApi, ServerConfig, ServerModelExecutor, ServerResources,
    ServerToolExecutor, WorkerLoopExecutor,
};
use brain_telemetry::{TelemetryRecord, TelemetrySink, telemetry_channel};
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "brain=info,brain_server=info".into()),
        )
        .init();
    let config = ServerConfig::parse();
    validate(&config)?;
    let api = compose(&config).await?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    tracing::info!(listen = %config.listen, "Brain is ready");
    let router = match config.api_token {
        Some(token) => brain_http::router_with_bearer(api, token),
        None => brain_http::router(api),
    };
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn compose(config: &ServerConfig) -> anyhow::Result<ServerApi> {
    let (telemetry, worker) = telemetry_channel();
    tokio::spawn(worker.run(Arc::new(LogSink)));
    let loops = Arc::new(WorkerPool::new(
        &config.loop_worker,
        config.data_dir.join("run"),
        config.data_dir.join("agentloops"),
        LoopLimits::default(),
    ));
    loops
        .ready()
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    // The server's one-off record of each session: the journal it was given, and the
    // credential it calls a model with. Appended, never fsynced.
    let metadata = Arc::new(brain_server::metadata::ServerMetadata::open(
        &brain_server::metadata::metadata_directory(&config.data_dir),
    )?);
    let models = Arc::new(LocalModelBindingStore::new(Arc::clone(&metadata)));
    let mut base_url_overrides = vec![(
        "vercel-ai-gateway".to_owned(),
        config.model_base_url.clone(),
    )];
    if let Some(url) = &config.openai_base_url {
        base_url_overrides.push(("openai".to_owned(), url.clone()));
    }
    if let Some(url) = &config.anthropic_base_url {
        base_url_overrides.push(("anthropic".to_owned(), url.clone()));
    }
    let model = Arc::new(ServerModelExecutor::new(
        models.clone(),
        &base_url_overrides,
        Duration::from_secs(120),
    )?);
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(120))
        .build()?;
    let environments = Arc::new(EnvironmentRegistry::new(
        Arc::new(InMemoryEnvironmentDirectory::new(
            &config.environment_base_url,
        )),
        Arc::new(HttpEnvironmentAdapter::new(
            http,
            config.environment_api_key.clone(),
        )),
    ));
    let kernel = Kernel::open(
        KernelConfig {
            data_dir: config.data_dir.join("journal"),
            max_decisions_per_turn: config.max_decisions_per_turn,
            loop_executor: Arc::new(WorkerLoopExecutor(loops.clone())),
            model_executor: model,
            tool_executor: Arc::new(ServerToolExecutor::new(environments.clone())),
        },
        telemetry,
    )?;
    // Restored sessions get back the journal ids this server minted for them. Best effort:
    // one the metadata lost stays readable and refuses further turns.
    kernel.adopt_journal_ids(&metadata.journals()?)?;
    Ok(ServerApi::new(ServerResources {
        kernel,
        loops,
        environments,
        models,
        metadata,
    }))
}

fn validate(config: &ServerConfig) -> anyhow::Result<()> {
    if config
        .api_token
        .as_deref()
        .is_some_and(|token| token.trim().is_empty())
    {
        anyhow::bail!("BRAIN_API_TOKEN cannot be empty when set");
    }
    if !config.listen.ip().is_loopback() && config.api_token.is_none() {
        anyhow::bail!("BRAIN_API_TOKEN is required when Brain listens beyond loopback");
    }
    if config
        .environment_api_key
        .as_deref()
        .is_some_and(|token| token.trim().is_empty())
    {
        anyhow::bail!("BRAIN_ENVIRONMENT_API_KEY cannot be empty when set");
    }
    if !config.environment_base_url.is_empty() {
        let url = reqwest::Url::parse(&config.environment_base_url)?;
        let loopback_http = url.scheme() == "http"
            && url
                .host_str()
                .and_then(|host| {
                    host.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .ok()
                })
                .is_some_and(|ip| ip.is_loopback());
        if !(url.scheme() == "https" || loopback_http)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            anyhow::bail!(
                "BRAIN_ENVIRONMENT_BASE_URL must use HTTPS or literal loopback HTTP and cannot contain credentials, query, or fragment"
            );
        }
    }
    if config.max_decisions_per_turn == 0 || config.max_decisions_per_turn > 1_024 {
        anyhow::bail!("BRAIN_MAX_DECISIONS must be in 1..=1024");
    }
    Ok(())
}

async fn shutdown() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
    tracing::info!("Brain is shutting down");
}

struct LogSink;

#[async_trait]
impl TelemetrySink for LogSink {
    async fn publish(
        &self,
        record: &TelemetryRecord,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!(
            telemetry_kind = ?record.kind,
            telemetry_name = %record.name,
            session_id = record.session_id.as_ref().map(ToString::to_string),
            journal_id = record.journal_id.as_ref().map(ToString::to_string),
            event_id = record.event_id.as_ref().map(ToString::to_string),
            operation_id = record.operation_id.as_ref().map(ToString::to_string),
            payload_bytes = record.payload.len(),
            "Brain telemetry"
        );
        Ok(())
    }
}
