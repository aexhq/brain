//! Neutral AWS persistence and custody adapters for Brain.
//!
//! Hand implementations intentionally do not live here: the selected Hands substrate implements
//! Brain's public ports and is supplied by the downstream composition.

pub mod dynamo;
pub mod kms;

use std::sync::Arc;

use brain::Result;
use brain::adapter::{HandFactory, ToolExecutor};
use brain::journal::Journal;
use brain::session::{Brain, BrainConfig};

#[derive(Debug, Clone)]
pub struct AwsPersistenceConfig {
    pub region: String,
    pub journal_table: String,
    pub kms_key_id: String,
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
            region: std::env::var("AWS_REGION").unwrap_or_else(|_| "eu-west-1".into()),
            journal_table: get("BRAIN_JOURNAL_TABLE")?,
            kms_key_id: get("BRAIN_KMS_KEY_ID")?,
        })
    }
}

/// Compose AWS durability with an independently supplied Hand implementation.
pub async fn compose(
    cfg: BrainConfig,
    persistence: AwsPersistenceConfig,
    hand_factory: Arc<dyn HandFactory>,
    external_executor: Option<Arc<dyn ToolExecutor>>,
) -> Result<Arc<Brain>> {
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
    Ok(match external_executor {
        Some(executor) => {
            Brain::with_parts_and_external(cfg, journal, custody, hand_factory, executor, None)
        }
        None => Brain::with_parts(cfg, journal, custody, hand_factory, None),
    })
}
