//! Reusable durable-local adapter composition.
//!
//! Product binaries may add their own trusted official capability policies and executor while
//! reusing these exact Brain-owned local persistence/storage seams. Selecting local remains an
//! explicit outer composition decision; production is never downgraded implicitly.

use crate::{LocalHand, LocalKeyCustody, LocalSessionStorage, SqliteStore};
use brain::Result;
use brain::hand::{HandPort, SandboxControlPort, SandboxFilesPort, SessionPreparationPort};
use brain::journal::Journal;
use brain::keys::KeyCustody;
use brain::storage::{BundleStoragePort, SessionStoragePort};
use std::path::PathBuf;
use std::sync::Arc;

pub struct DurableLocalParts {
    pub journal: Journal,
    pub custody: Arc<dyn KeyCustody>,
    pub session_storage: Arc<dyn SessionStoragePort>,
    pub bundle_storage: Arc<dyn BundleStoragePort>,
    /// Concrete handle retained so the outer composition can attach Brain's one-purpose secret
    /// delivery port after constructing the circular service graph.
    pub local_hand: Arc<LocalHand>,
    pub hand: Arc<dyn HandPort>,
    pub session_preparation: Arc<dyn SessionPreparationPort>,
    pub sandbox_files: Arc<dyn SandboxFilesPort>,
    pub sandbox_control: Arc<dyn SandboxControlPort>,
}

pub fn durable_local_parts(data_dir: impl Into<PathBuf>) -> Result<DurableLocalParts> {
    let data_dir = data_dir.into();
    std::fs::create_dir_all(&data_dir)
        .map_err(|error| brain::BrainError::Journal(format!("create local data: {error}")))?;
    let store = Arc::new(SqliteStore::open(data_dir.join("journal.sqlite3"))?);
    let custody: Arc<dyn KeyCustody> =
        Arc::new(LocalKeyCustody::open(data_dir.join("master.key"))?);
    let storage = Arc::new(LocalSessionStorage::open(data_dir.join("session-storage"))?);
    let session_storage: Arc<dyn SessionStoragePort> = storage.clone();
    let bundle_storage: Arc<dyn BundleStoragePort> = storage;
    let local_hand = LocalHand::open(data_dir.join("local-hand"))?;
    let hand: Arc<dyn HandPort> = local_hand.clone();
    let session_preparation: Arc<dyn SessionPreparationPort> = local_hand.clone();
    let sandbox_files: Arc<dyn SandboxFilesPort> = local_hand.clone();
    let sandbox_control: Arc<dyn SandboxControlPort> = local_hand.clone();
    Ok(DurableLocalParts {
        journal: Journal::new(store, format!("brain-{}", brain::mint_id("node", 16))),
        custody,
        session_storage,
        bundle_storage,
        local_hand,
        hand,
        session_preparation,
        sandbox_files,
        sandbox_control,
    })
}
