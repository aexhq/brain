//! AWS adapters for the aex brain, behind the core's generic seams:
//!
//! - [`dynamo::DynamoJournal`] -- `brain::journal::JournalStore` on DynamoDB (lease + fence,
//!   one `TransactWriteItems` per decision);
//! - [`kms::KmsCustody`] -- `brain::keys::KeyCustody` on KMS (per-session encryption context);
//! - [`lambda::LambdaFactory`] / [`lambda::LambdaHand`] -- `brain::adapter::HandFactory` /
//!   `HandAdapter` on AWS Lambda MicroVMs (Firecracker isolation, suspend/resume, workspace
//!   sync to S3, wall survival by re-materialisation).
//!
//! The core never depends on this crate; a server composes it in (see `brain-server`), and a
//! different substrate implements the same traits instead.

pub mod dynamo;
pub mod kms;
pub mod lambda;

use brain::Result;
use brain::journal::Journal;
use brain::session::{Brain, BrainConfig};
use std::sync::Arc;

/// The standard AWS composition, entirely from the environment (see
/// [`lambda::HandPlaneConfig::from_env`] plus `AEX_JOURNAL_TABLE` and `AEX_KMS_KEY_ID`).
/// Fails fast on anything missing: production is configured, never guessed.
pub async fn brain_from_env(cfg: BrainConfig) -> Result<Arc<Brain>> {
    // D14: production never runs with a permissive SSRF guard. The local-mode constructor
    // defaults this to true; the AWS composition refuses to start with it rather than carry
    // a developer convenience into the trusted tier.
    if cfg.outbound_allow_private {
        return Err(brain::BrainError::Invalid(
            "AEX_OUTBOUND_ALLOW_PRIVATE may not be true in AEX_MODE=aws (SSRF guard, D14)".into(),
        ));
    }
    let get = |k: &str| {
        std::env::var(k).map_err(|_| brain::BrainError::Invalid(format!("{k} is not set")))
    };
    let journal_table = get("AEX_JOURNAL_TABLE")?;
    let kms_key_id = get("AEX_KMS_KEY_ID")?;
    let plane_cfg = lambda::HandPlaneConfig::from_env()?;
    let aws = aws_config::from_env()
        .region(aws_config::Region::new(plane_cfg.region.clone()))
        .load()
        .await;
    let owner = format!("brain-{}", brain::mint_id("i", 12));
    Ok(Brain::with_parts(
        cfg,
        Journal::new(
            Arc::new(dynamo::DynamoJournal::new(
                aws_sdk_dynamodb::Client::new(&aws),
                journal_table,
            )),
            owner,
        ),
        Arc::new(kms::KmsCustody::new(
            aws_sdk_kms::Client::new(&aws),
            kms_key_id,
        )),
        Arc::new(lambda::LambdaFactory::new(Arc::new(
            lambda::HandPlane::from_env(plane_cfg).await,
        ))),
        None,
    ))
}
