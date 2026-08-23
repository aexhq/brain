//! Neutral AWS persistence and custody adapters for Brain.
//!
//! Environment implementations intentionally do not live here: the selected Environments substrate implements
//! Brain's public ports and is supplied by the downstream composition.

pub mod dynamo;
pub mod kms;
pub mod s3;

use std::sync::Arc;

use brain::Result;
use brain::adapter::ToolExecutor;
use brain::environment::{EnvironmentPort, SandboxControlPort, SandboxFilesPort, SessionPreparationPort};
use brain::journal::Journal;
use brain::session::{Brain, BrainConfig};

pub struct AwsRuntimePorts {
    pub environment: Arc<dyn EnvironmentPort>,
    pub session_preparation: Arc<dyn SessionPreparationPort>,
    pub sandbox_files: Arc<dyn SandboxFilesPort>,
    pub sandbox_control: Arc<dyn SandboxControlPort>,
    pub external_executor: Option<Arc<dyn ToolExecutor>>,
    pub customer_delivery: Option<Arc<dyn brain::customer::CustomerEnvironmentDeliveryPort>>,
    pub customer_transport: Option<brain::customer::CustomerTransportConfig>,
    pub agentloop_registry: Option<Arc<dyn brain::agentloop::AgentloopRegistry>>,
    /// Overrides the live providers; the hosted default is the guarded transport with private
    /// addresses denied.
    pub provider_factory: Option<brain::session::ProviderFactory>,
}

#[derive(Debug, Clone)]
pub struct AwsPersistenceConfig {
    pub region: String,
    pub journal_table: String,
    pub kms_key_id: String,
    pub session_storage_bucket: String,
    pub session_storage_prefix: String,
}

impl AwsPersistenceConfig {
    /// Read only neutral Brain-owned environment names. A downstream product may map its own
    /// outer configuration into this value without teaching Brain about that product.
    pub fn from_env() -> Result<Self> {
        let get = |name: &str| {
            std::env::var(name)
                .map_err(|_| brain::BrainError::Invalid(format!("{name} is not set")))
        };
        Ok(Self {
            // No fallback region: a hosted composition booted without AWS_REGION would
            // otherwise silently pin itself to one region — a deploy mistake must fail boot.
            region: get("AWS_REGION")?,
            journal_table: get("BRAIN_JOURNAL_TABLE")?,
            kms_key_id: get("BRAIN_KMS_KEY_ID")?,
            session_storage_bucket: get("BRAIN_SESSION_STORAGE_BUCKET")?,
            session_storage_prefix: std::env::var("BRAIN_SESSION_STORAGE_PREFIX")
                .unwrap_or_else(|_| "sessions".into()),
        })
    }
}

/// Compose AWS durability with an independently supplied Environment implementation.
pub async fn compose(
    cfg: BrainConfig,
    persistence: AwsPersistenceConfig,
    ports: AwsRuntimePorts,
) -> Result<Arc<Brain>> {
    cfg.validate()?;
    if cfg.outbound_allow_private {
        return Err(brain::BrainError::Invalid(
            "BRAIN_OUTBOUND_ALLOW_PRIVATE may not be true in a hosted AWS composition".into(),
        ));
    }
    let aws = aws_config::from_env()
        .region(aws_config::Region::new(persistence.region))
        .load()
        .await;
    let journal = Journal::new(
        Arc::new(dynamo::DynamoJournal::new(
            aws_sdk_dynamodb::Client::new(&aws),
            persistence.journal_table,
        )),
        format!("brain-{}", brain::mint_id("i", 12)),
    );
    let custody: Arc<dyn brain::keys::KeyCustody> = Arc::new(kms::KmsCustody::new(
        aws_sdk_kms::Client::new(&aws),
        persistence.kms_key_id,
    ));
    let storage = Arc::new(
        s3::S3SessionStorage::new(
            aws_sdk_s3::Client::new(&aws),
            persistence.session_storage_bucket,
        )
        .with_prefix(persistence.session_storage_prefix)?
        .with_transfer_ttl(cfg.storage_transfer_ttl)?,
    );
    let executor: Arc<dyn ToolExecutor> = match ports.external_executor {
        Some(executor) => executor,
        None => Arc::new(brain::adapter::DisabledToolExecutor),
    };
    Ok(Brain::with_parts_and_services(
        cfg,
        journal,
        custody,
        executor,
        brain::session::BrainServices {
            session_storage: Some(storage.clone()),
            bundle_storage: Some(storage),
            environment: Some(ports.environment),
            session_preparation: Some(ports.session_preparation),
            sandbox_files: Some(ports.sandbox_files),
            sandbox_control: Some(ports.sandbox_control),
            customer_delivery: ports.customer_delivery,
            customer_transport: ports.customer_transport,
            compactor: None,
            agentloop_registry: ports.agentloop_registry,
        },
        ports
            .provider_factory
            .unwrap_or_else(|| brain_providers::default_factory(false)),
    ))
}
