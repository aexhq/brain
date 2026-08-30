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
    /// Endpoint for the `vercel-ai-gateway` provider. Kept under its historic
    /// name because deploys and tests already set it.
    #[arg(
        long,
        env = "BRAIN_MODEL_BASE_URL",
        default_value = "https://ai-gateway.vercel.sh/v1"
    )]
    pub model_base_url: String,
    /// Endpoint override for the `openai` provider; also the hook for pointing
    /// that provider at any OpenAI-compatible server (Ollama, vLLM, a proxy).
    #[arg(long, env = "BRAIN_OPENAI_BASE_URL")]
    pub openai_base_url: Option<String>,
    /// Endpoint override for the `anthropic` provider.
    #[arg(long, env = "BRAIN_ANTHROPIC_BASE_URL")]
    pub anthropic_base_url: Option<String>,
    /// A JSON file of custom provider definitions merged over the built-in
    /// catalog: `{"providers": [{"name", "dialect", "base_url", ...}]}`, each
    /// entry in the same shape as a registry `ProviderDef`. A definition here
    /// supersedes a catalog provider of the same name.
    #[arg(long, env = "BRAIN_PROVIDERS_FILE")]
    pub providers_file: Option<PathBuf>,
    #[arg(long, env = "BRAIN_API_TOKEN", hide_env_values = true)]
    pub api_token: Option<String>,
    #[arg(long, env = "BRAIN_ENVIRONMENT_BASE_URL", default_value = "")]
    pub environment_base_url: String,
    #[arg(long, env = "BRAIN_ENVIRONMENT_API_KEY", hide_env_values = true)]
    pub environment_api_key: Option<String>,
    #[arg(long, env = "BRAIN_MAX_DECISIONS", default_value_t = 128)]
    pub max_decisions_per_turn: usize,
}
