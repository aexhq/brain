//! The neutral Brain server library: one local composition shared by the shipped binary and
//! the canonical e2e, so the wiring under test is the wiring that runs.

use std::path::PathBuf;
use std::sync::Arc;

pub mod api;

use brain::session::{Brain, BrainConfig, BrainServices, ProviderFactory};
use brain_standalone::durable_local_parts;

/// Bounded Agentloop component workers and content-addressed storage wiring.
#[cfg(feature = "loophost")]
pub struct LoophostOptions {
    pub component_host: PathBuf,
    pub workers: usize,
}

pub struct LocalOptions {
    pub data_dir: PathBuf,
    pub cfg: BrainConfig,
    /// Address the customer-environment transport advertises (`ws://{address}/v1/customer-environment/socket`
    /// and `http://{address}`), or explicit URL overrides via [`LocalOptions::transport_urls`].
    pub advertised_address: String,
    /// Explicit customer-environment transport URLs winning over the derived defaults.
    pub transport_urls: Option<(String, String)>,
    /// `None` composes the real providers.
    pub provider_factory: Option<ProviderFactory>,
    /// Deployment-owned Environment host operations. Absence keeps provider-backed Environment
    /// components fail-closed while deterministic components remain usable.
    pub environment_capabilities: Option<Arc<dyn brain_component_host::CapabilityHandler>>,
    #[cfg(feature = "loophost")]
    pub loophost: Option<LoophostOptions>,
}

pub struct AwsOptions {
    pub data_dir: PathBuf,
    pub cfg: BrainConfig,
    pub persistence: brain_aws::AwsPersistenceConfig,
    pub environment_capabilities: Option<Arc<dyn brain_component_host::CapabilityHandler>>,
    pub customer_delivery: Option<Arc<dyn brain::customer::CustomerEnvironmentDeliveryPort>>,
    pub customer_transport: Option<brain::customer::CustomerTransportConfig>,
    #[cfg(feature = "loophost")]
    pub loophost: Option<LoophostOptions>,
}

/// The product-neutral hosted composition: AWS durability plus the same four bounded component
/// hosts used by the standalone server. Product policy and Environment providers attach only
/// through process configuration and the generic dispatch capability.
pub async fn compose_aws(options: AwsOptions) -> anyhow::Result<Arc<Brain>> {
    std::fs::create_dir_all(&options.data_dir)?;
    #[cfg(feature = "loophost")]
    let loophost = options
        .loophost
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("loophost configuration is required"))?;
    #[cfg(feature = "loophost")]
    let loop_services = brain_loophost::registry::services_with_component_store(
        &options.data_dir.join("loops"),
        &loophost.component_host,
        loophost.workers,
    )
    .await?;
    #[cfg(feature = "loophost")]
    let model_registry = brain_providers::component::registry_with_component_store(
        &options.data_dir.join("models"),
        &loophost.component_host,
        loophost.workers,
        brain_providers::Outbound::new(options.cfg.outbound_allow_private),
    )
    .await?;
    #[cfg(feature = "loophost")]
    let tool_registry = brain_toolhost::registry_with_component_store(
        &options.data_dir.join("tools"),
        &loophost.component_host,
        loophost.workers,
    )
    .await?;
    #[cfg(feature = "loophost")]
    let component_environment_registry = brain_environment_host::registry_with_component_store(
        &options.data_dir.join("environments"),
        &loophost.component_host,
        loophost.workers,
        options
            .environment_capabilities
            .unwrap_or_else(|| Arc::new(brain_environment_host::RejectEnvironmentCapabilities)),
    )
    .await?;
    #[cfg(not(feature = "loophost"))]
    return Err(anyhow::anyhow!(
        "brain-server requires the loophost feature"
    ));
    let external_executor = brain_providers::external_executor_from_cfg(&options.cfg)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    brain_aws::compose(
        options.cfg,
        options.persistence,
        brain_aws::AwsRuntimePorts {
            environments: brain::environment::EnvironmentRegistry::default(),
            external_executor: Some(external_executor),
            customer_delivery: options.customer_delivery,
            customer_transport: options.customer_transport,
            agentloop_registry: loop_services.agentloop_registry,
            model_registry: Some(model_registry),
            tool_registry: Some(tool_registry),
            component_environment_registry: Some(component_environment_registry),
            provider_factory: None,
        },
    )
    .await
    .map_err(|error| anyhow::anyhow!("{error}"))
}

/// The explicit durable local composition: SQLite WAL journal, persistent local custody and
/// session-object storage under `data_dir`, Tool execution through the host node runtime, and
/// the customer-environment transport served from the same listener. This is the whole wiring — the
/// binary adds only env parsing, the operator token and the startup audit.
pub async fn compose_local(options: LocalOptions) -> anyhow::Result<Arc<Brain>> {
    let allow_private = options.cfg.outbound_allow_private;
    let parts =
        durable_local_parts(&options.data_dir).map_err(|error| anyhow::anyhow!("{error}"))?;
    let external_executor = brain_providers::external_executor_from_cfg(&options.cfg)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let (websocket_url, observation_base_url) = options.transport_urls.unwrap_or_else(|| {
        (
            format!(
                "ws://{}/v1/customer-environment/socket",
                options.advertised_address
            ),
            format!("http://{}", options.advertised_address),
        )
    });
    let customer_transport =
        brain::customer::CustomerTransportConfig::new(websocket_url, observation_base_url)?;
    #[cfg(feature = "loophost")]
    let loophost = options
        .loophost
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("loophost configuration is required"))?;
    #[cfg(feature = "loophost")]
    let loop_services = brain_loophost::registry::services_with_component_store(
        &options.data_dir.join("loops"),
        &loophost.component_host,
        loophost.workers,
    )
    .await?;
    #[cfg(feature = "loophost")]
    let model_registry = brain_providers::component::registry_with_component_store(
        &options.data_dir.join("models"),
        &loophost.component_host,
        loophost.workers,
        brain_providers::Outbound::new(allow_private),
    )
    .await?;
    #[cfg(feature = "loophost")]
    let tool_registry = brain_toolhost::registry_with_component_store(
        &options.data_dir.join("tools"),
        &loophost.component_host,
        loophost.workers,
    )
    .await?;
    #[cfg(feature = "loophost")]
    let component_environment_registry = brain_environment_host::registry_with_component_store(
        &options.data_dir.join("environments"),
        &loophost.component_host,
        loophost.workers,
        options
            .environment_capabilities
            .unwrap_or_else(|| Arc::new(brain_environment_host::RejectEnvironmentCapabilities)),
    )
    .await?;
    #[cfg(not(feature = "loophost"))]
    return Err(anyhow::anyhow!(
        "brain-server requires the loophost feature"
    ));
    let local_environment = parts.local_environment.clone();
    let brain = Brain::with_parts_and_services(
        options.cfg,
        parts.journal,
        parts.custody,
        external_executor,
        BrainServices {
            session_storage: Some(parts.session_storage),
            bundle_storage: Some(parts.bundle_storage),
            environments: brain::environment::EnvironmentRegistry::new([(
                "brain.local".into(),
                brain::environment::EnvironmentAdapter {
                    execution: parts.environment,
                    preparation: parts.session_preparation,
                    files: Some(parts.sandbox_files),
                },
            )])?,
            customer_transport: Some(customer_transport),
            agentloop_registry: loop_services.agentloop_registry,
            model_registry: Some(model_registry),
            tool_registry: Some(tool_registry),
            component_environment_registry: Some(component_environment_registry),
            ..BrainServices::default()
        },
        options
            .provider_factory
            .unwrap_or_else(|| brain_providers::default_factory(allow_private)),
    );
    local_environment
        .attach_secret_delivery(brain.clone())
        .map_err(|error| {
            anyhow::anyhow!(
                "local Environment secret delivery: {}",
                error.message.as_str()
            )
        })?;
    Ok(brain)
}
