//! The brain server: one long-lived process, many sessions.
//!
//! Configuration is environment-only (fail fast on anything missing):
//!   AEX_JOURNAL_TABLE       DynamoDB journal table
//!   AEX_KMS_KEY_ID          KMS key (id, arn or alias/...) for BYOK custody
//!   AEX_SESSIONS_BUCKET     S3 bucket for sync packs, seeds and artifacts
//!   AEX_HAND_IMAGE          MicroVM image name (e.g. aex-hands-dev-1gb)
//!   AEX_HAND_IMAGE_VERSION  image version (e.g. 2.0)
//!   AEX_API_TOKEN           dev-plane bearer token (identity proper is slice 4)
//!   AEX_LISTEN              listen address, default 127.0.0.1:8700
//!   AWS_REGION / AWS_PROFILE as usual

use brain::api::{AppState, serve};
use brain::session::{Brain, BrainConfig};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,aws_config=warn,hyper=warn".into()),
        )
        .init();
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let cfg = BrainConfig::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let token =
        std::env::var("AEX_API_TOKEN").map_err(|_| anyhow::anyhow!("AEX_API_TOKEN is not set"))?;
    let addr: std::net::SocketAddr = std::env::var("AEX_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8700".into())
        .parse()?;
    let brain = Brain::new(cfg).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    tracing::info!(reclaim = brain::reclaim::MECHANISM, "brain starting");
    serve(AppState { brain, token }, addr).await
}
