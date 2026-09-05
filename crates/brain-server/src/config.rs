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
    /// HTTP origins or authorities that sessions may grant Brain Wasm Components.
    #[arg(long, env = "BRAIN_WASM_NETWORK_ALLOW", value_delimiter = ',')]
    pub wasm_network_allow: Vec<String>,
    /// Process environment variable names that sessions may grant Brain Wasm Components.
    #[arg(long, env = "BRAIN_WASM_SECRET_ALLOW", value_delimiter = ',')]
    pub wasm_secret_allow: Vec<String>,
    /// Writable filesystem roots that sessions may grant: `scratch` or `workspace`.
    #[arg(long, env = "BRAIN_WASM_FILESYSTEM_ALLOW", value_delimiter = ',')]
    pub wasm_filesystem_allow: Vec<String>,
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
    /// JSON map from driver name to { endpoint, api_key? }. Credentials stay in the server.
    #[arg(long, env = "BRAIN_ENVIRONMENT_ROUTES_FILE")]
    pub environment_routes_file: Option<PathBuf>,
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
