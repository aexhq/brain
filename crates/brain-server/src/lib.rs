//! The neutral Brain server library: one local composition shared by the shipped binary and
//! the canonical e2e, so the wiring under test is the wiring that runs.

use std::path::PathBuf;
use std::sync::Arc;

pub mod api;

use brain::session::{Brain, BrainConfig, BrainServices, ProviderFactory};
use brain_standalone::durable_local_parts;

/// Optional custom-agentloop wiring: the official aex loop as a wasm component plus a
/// content-addressed store for customer loops. Absent, the in-process built-in loop runs —
/// identical policy, no wasm isolation boundary.
#[cfg(feature = "loophost")]
pub struct LoophostOptions {
    /// Path to the componentized official aex loop.
    pub aex_component: PathBuf,
    /// Directory holding `componentize-one.mjs` with the pinned componentizer installed.
    pub toolchain_dir: PathBuf,
}

pub struct LocalOptions {
    pub data_dir: PathBuf,
    pub cfg: BrainConfig,
    /// Address the customer-hand transport advertises (`ws://{address}/v1/customer-hand/socket`
    /// and `http://{address}`), or explicit URL overrides via [`LocalOptions::transport_urls`].
    pub advertised_address: String,
    /// Explicit customer-hand transport URLs winning over the derived defaults.
    pub transport_urls: Option<(String, String)>,
    /// `None` composes the real providers.
    pub provider_factory: Option<ProviderFactory>,
    #[cfg(feature = "loophost")]
    pub loophost: Option<LoophostOptions>,
}

/// The explicit durable local composition: SQLite WAL journal, persistent local custody and
/// session-object storage under `data_dir`, Tool execution through the host node runtime, and
/// the customer-hand transport served from the same listener. This is the whole wiring — the
/// binary adds only env parsing, the operator token and the startup audit.
pub fn compose_local(options: LocalOptions) -> anyhow::Result<Arc<Brain>> {
    let allow_private = options.cfg.outbound_allow_private;
    let parts =
        durable_local_parts(&options.data_dir).map_err(|error| anyhow::anyhow!("{error}"))?;
    let external_executor = brain_providers::external_executor_from_cfg(&options.cfg)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let (websocket_url, observation_base_url) = options.transport_urls.unwrap_or_else(|| {
        (
            format!(
                "ws://{}/v1/customer-hand/socket",
                options.advertised_address
            ),
            format!("http://{}", options.advertised_address),
        )
    });
    let customer_transport =
        brain::customer::CustomerTransportConfig::new(websocket_url, observation_base_url)?;
    #[cfg(feature = "loophost")]
    let loop_services = match &options.loophost {
        Some(loophost) => brain_loophost::registry::services_with_loop_store(
            &loophost.aex_component,
            &options.data_dir.join("loops"),
            &loophost.toolchain_dir,
        )?,
        None => BrainServices::default(),
    };
    #[cfg(not(feature = "loophost"))]
    let loop_services = BrainServices::default();
    let local_hand = parts.local_hand.clone();
    let brain = Brain::with_parts_and_services(
        options.cfg,
        parts.journal,
        parts.custody,
        external_executor,
        BrainServices {
            session_storage: Some(parts.session_storage),
            bundle_storage: Some(parts.bundle_storage),
            hand: Some(parts.hand),
            session_preparation: Some(parts.session_preparation),
            sandbox_files: Some(parts.sandbox_files),
            sandbox_control: Some(parts.sandbox_control),
            customer_transport: Some(customer_transport),
            agentloop: loop_services.agentloop,
            agentloop_registry: loop_services.agentloop_registry,
            ..BrainServices::default()
        },
        options
            .provider_factory
            .unwrap_or_else(|| brain_providers::default_factory(allow_private)),
    );
    local_hand
        .attach_secret_delivery(brain.clone())
        .map_err(|error| {
            anyhow::anyhow!("local Hand secret delivery: {}", error.message.as_str())
        })?;
    Ok(brain)
}
