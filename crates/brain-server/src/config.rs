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
    /// Origins the brain env may grant a Component that needs them: exact, or
    /// `https://*.example.com` for a family of hosts.
    #[arg(long, env = "BRAIN_ENV_NETWORK_ALLOW", value_delimiter = ',')]
    pub env_network_allow: Vec<String>,
    /// Process environment variable names the brain env may mount under `/secrets`.
    #[arg(long, env = "BRAIN_ENV_SECRET_ALLOW", value_delimiter = ',')]
    pub env_secret_allow: Vec<String>,
    /// Filesystem roots the brain env may grant: `scratch` or `workspace`.
    #[arg(long, env = "BRAIN_ENV_FILESYSTEM_ALLOW", value_delimiter = ',')]
    pub env_filesystem_allow: Vec<String>,
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
    /// Where an Environment on another machine reaches this server, for the turns it
    /// runs. Defaults to `http://{listen}`.
    #[arg(long, env = "BRAIN_PUBLIC_URL")]
    pub public_url: Option<String>,
    /// Model calls one turn may make before Brain refuses the next.
    #[arg(long, env = "BRAIN_MAX_MODEL_CALLS", default_value_t = 128)]
    pub max_model_calls_per_turn: usize,
    /// Seconds one turn may run before Brain cancels it. Zero means no bound.
    #[arg(long, env = "BRAIN_MAX_TURN_SECS", default_value_t = 1800)]
    pub max_turn_secs: u64,
    /// Seconds an idle session keeps its task and memory before it is suspended to disk
    /// and rebuilt on its next request. A session may set its own at create; zero means
    /// never.
    #[arg(long, env = "BRAIN_SESSION_IDLE_TTL_SECS")]
    pub session_idle_ttl_secs: Option<u64>,
}
