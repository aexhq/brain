use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use brain::{Feed, SessionRuntime, Writer};
use brain_loophost::{LoopLimits, WorkerPool};
use brain_server::{
    BrainEnvironment, EnvironmentLoopExecutor, EnvironmentRegistry, HostEnvironment,
    HttpEnvironmentAdapter, IdempotencyStore, NativePolicy, ServerApi, ServerConfig,
    ServerModelExecutor, ServerResources, ServerToolExecutor,
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
    let _data_lock = brain_server::data_layout::lock(&config.data_dir)?;
    let api = compose(&config).await?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    tracing::info!(listen = %config.listen, "Brain is ready");
    let router = match config.api_token {
        Some(token) => brain_http::router_with_bearer(api, token),
        None => {
            tracing::warn!(
                "BRAIN_API_TOKEN is not set: the API is reachable without authentication"
            );
            brain_http::router(api)
        }
    };
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn compose(config: &ServerConfig) -> anyhow::Result<ServerApi> {
    let sessions_dir = brain_server::data_layout::prepare(&config.data_dir)?;
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
    // Credentials are durable before the session's creation record is committed.
    let credentials = Arc::new(brain_server::metadata::ServerMetadata::open(
        &brain_server::metadata::metadata_directory(&config.data_dir),
    )?);
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
    let custom_providers = match &config.providers_file {
        Some(path) => brain_server::load_providers_file(path)?,
        None => Vec::new(),
    };
    let providers = Arc::new(brain::model::ProviderRegistry::compose(
        custom_providers,
        &base_url_overrides,
    )?);
    let model = Arc::new(ServerModelExecutor::new(
        credentials.clone(),
        &providers,
        Duration::from_secs(120),
    )?);
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(120))
        .build()?;
    let environments = Arc::new(EnvironmentRegistry::new(
        Arc::new(BrainEnvironment::new(
            loops.clone(),
            NativePolicy {
                network: config.env_network_allow.iter().cloned().collect(),
                secrets: config.env_secret_allow.iter().cloned().collect(),
                filesystem: config.env_filesystem_allow.iter().cloned().collect(),
            },
            config.data_dir.join("native-workspaces"),
        )),
        HostEnvironment::open(&config.data_dir.join("hosts").join("hosts.log"))?,
        Arc::new(HttpEnvironmentAdapter::new(http, credentials.clone())),
    ));
    // Every session's directory, rebuilt from disk. A session that was mid-turn when the
    // last process stopped is failed with code `interrupted` before anything is served.
    let writer = Writer::spawn();
    let feed = Arc::new(Feed::new(telemetry.clone()));
    let session_runtime = Arc::new(SessionRuntime {
        max_model_calls_per_turn: config.max_model_calls_per_turn,
        max_turn_ms: config.max_turn_secs.saturating_mul(1_000),
        tool_deadline_ms: brain::DEFAULT_TOOL_DEADLINE_MS,
        loop_executor: Arc::new(EnvironmentLoopExecutor(environments.clone())),
        model_executor: model,
        tool_executor: Arc::new(ServerToolExecutor::new(environments.clone())),
        live: feed.clone(),
        telemetry: telemetry.clone(),
    });
    let api = ServerApi::new(ServerResources {
        sessions_dir,
        writer,
        feed,
        session_runtime,
        session_idle_ttl: config.session_idle_ttl_secs.map(Duration::from_secs),
        idempotency: IdempotencyStore::open(
            &config.data_dir.join("requests").join("requests.log"),
            brain_server::idempotency::DEFAULT_RETENTION,
        )?,
        loops,
        environments,
        credentials,
        providers,
    })?;
    api.spawn_idle_sweeper();
    Ok(api)
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
    if config.max_model_calls_per_turn == 0 || config.max_model_calls_per_turn > 1_024 {
        anyhow::bail!("BRAIN_MAX_MODEL_CALLS must be in 1..=1024");
    }
    if config
        .env_filesystem_allow
        .iter()
        .any(|name| name != "scratch" && name != "workspace")
    {
        anyhow::bail!("BRAIN_ENV_FILESYSTEM_ALLOW accepts only scratch and workspace");
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
    async fn publish_batch(
        &self,
        records: &[TelemetryRecord],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for record in records {
            tracing::info!(
                telemetry_kind = ?record.kind,
                telemetry_name = %record.name,
                session_id = record.session_id.as_ref().map(ToString::to_string),
                sequence = record.sequence,
                payload_bytes = record.payload.len(),
                "Brain telemetry"
            );
        }
        Ok(())
    }
}
