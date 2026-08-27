use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

#[derive(Clone, Parser)]
#[command(name = "brain", version, about = "A standalone Brain session server")]
pub struct ServerConfig {
    #[arg(long, env = "BRAIN_LISTEN", default_value = "127.0.0.1:8080")]
    pub listen: SocketAddr,
    #[arg(long, env = "BRAIN_DATA_DIR", default_value = "brain-data")]
    pub data_dir: PathBuf,
    #[arg(long, env = "BRAIN_LOOP_WORKER", default_value = "brain-loop-worker")]
    pub loop_worker: PathBuf,
    #[arg(
        long,
        env = "BRAIN_MODEL_BASE_URL",
        default_value = "https://ai-gateway.vercel.sh/v1"
    )]
    pub model_base_url: String,
    #[arg(long, env = "BRAIN_MODEL_API_KEY", hide_env_values = true)]
    pub model_api_key: String,
    #[arg(long, env = "BRAIN_API_TOKEN", hide_env_values = true)]
    pub api_token: Option<String>,
    #[arg(long, env = "BRAIN_ENVIRONMENT_BASE_URL", default_value = "")]
    pub environment_base_url: String,
    #[arg(long, env = "BRAIN_ENVIRONMENT_API_KEY", hide_env_values = true)]
    pub environment_api_key: Option<String>,
    #[arg(long, env = "BRAIN_MAX_DECISIONS", default_value_t = 128)]
    pub max_decisions_per_turn: usize,
}
