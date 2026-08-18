//! The brain server: one long-lived process, many sessions.
//!
//! Configuration is environment-only. Two modes:
//!
//! `AEX_MODE=local` (the DEFAULT -- `cargo run --bin brain` just works):
//!   in-memory journal (sessions do NOT survive a restart), tools run locally in
//!   subprocesses against per-session directories under AEX_DATA_DIR (./aex-data).
//!   Process separation is NOT a sandbox: run only your own prompts this way.
//!   AEX_API_TOKEN optional (a token is generated and printed if unset).
//!
//! `AEX_MODE=aws` (production -- fail fast on anything missing):
//!   AEX_JOURNAL_TABLE       DynamoDB journal table
//!   AEX_KMS_KEY_ID          KMS key (id, arn or alias/...) for BYOK custody
//!   AEX_SESSIONS_BUCKET     S3 bucket for sync packs, seeds and artifacts
//!   AEX_HAND_IMAGE          MicroVM image name (e.g. aex-hands-dev-1gb)
//!   AEX_HAND_IMAGE_VERSION  image version (e.g. 2.0)
//!   AEX_API_TOKEN           required
//!   AWS_REGION / AWS_PROFILE as usual
//!
//! Both: AEX_LISTEN (default 127.0.0.1:8700).

use brain::api::{AppState, serve};
use brain::session::{Brain, BrainConfig};

fn is_local() -> bool {
    matches!(std::env::var("AEX_MODE").as_deref(), Err(_) | Ok("local"))
}

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
    let cfg = BrainConfig::default();
    let local = is_local();
    match std::env::var("AEX_MODE").as_deref() {
        Err(_) | Ok("local") | Ok("aws") => {}
        Ok(other) => anyhow::bail!("AEX_MODE must be local or aws, got {other}"),
    }
    let token = match std::env::var("AEX_API_TOKEN") {
        Ok(t) => t,
        Err(_) if local => {
            let t = brain::mint_id("tok", 24);
            tracing::warn!("AEX_API_TOKEN not set; generated one for this run: {t}");
            t
        }
        Err(_) => anyhow::bail!("AEX_API_TOKEN is not set"),
    };
    let addr: std::net::SocketAddr = std::env::var("AEX_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8700".into())
        .parse()?;
    let brain = if local {
        let data_dir = std::env::var("AEX_DATA_DIR").unwrap_or_else(|_| "./aex-data".into());
        Brain::local(data_dir, cfg).map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        brain_aws::brain_from_env(cfg)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
    };
    if local {
        tracing::warn!(
            "LOCAL MODE: in-memory journal (sessions do not survive restarts); tools run in              subprocesses on THIS machine -- process separation, not a sandbox. Use only with              prompts you trust; production isolation is AEX_MODE=aws (MicroVMs)."
        );
    }
    tracing::info!(
        mode = if local { "local" } else { "aws" },
        reclaim = brain::reclaim::MECHANISM,
        "brain starting"
    );
    serve(AppState { brain, token }, addr).await
}
