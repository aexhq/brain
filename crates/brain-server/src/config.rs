use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

/// Everything the server reads from its command line and process environment. This is
/// the one place Brain reads either: every limit below is a default the crate that
/// enforces it ships, overridden here and injected once when the server composes.
#[derive(Clone, Parser)]
#[command(name = "brain", version, about = "A standalone Brain session server")]
pub struct ServerConfig {
    /// Address to bind.
    #[arg(long, env = "BRAIN_LISTEN", default_value = "127.0.0.1:8080")]
    pub listen: SocketAddr,
    /// Where journals, admitted Components, and local state live.
    #[arg(long, env = "BRAIN_DATA_DIR", default_value = "brain-data")]
    pub data_dir: PathBuf,
    /// Path to the loop worker executable.
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
    /// Bearer token callers must present. Required when listening beyond loopback.
    #[arg(long, env = "BRAIN_API_TOKEN", hide_env_values = true)]
    pub api_token: Option<String>,
    /// Where an Environment on another machine reaches this server, for the turns it
    /// runs. Defaults to `http://{listen}`.
    #[arg(long, env = "BRAIN_PUBLIC_URL")]
    pub public_url: Option<String>,
    /// Seconds an idle session keeps its task and memory before it is suspended to disk
    /// and rebuilt on its next request. A session may set its own at create; zero means
    /// never.
    #[arg(long, env = "BRAIN_SESSION_IDLE_TTL_SECS")]
    pub session_idle_ttl_secs: Option<u64>,
    #[command(flatten)]
    pub limits: brain::Limits,
    #[command(flatten)]
    pub loop_limits: brain_loophost::LoopLimits,
    #[command(flatten)]
    pub telemetry_limits: brain_telemetry::TelemetryLimits,
    #[command(flatten)]
    pub http_limits: brain_http::HttpLimits,
    #[command(flatten)]
    pub server_limits: crate::ServerLimits,
}
