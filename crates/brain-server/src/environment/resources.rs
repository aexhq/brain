//! The environments this Brain knows.
//!
//! Each is created with its configuration during one session's admission,
//! and forgotten when its binding is closed. Providers own physical expiry.
//! One JSON file per environment under `{data_dir}/environments/`, written at
//! create, rewritten when the setup receipt arrives, and removed at close. Small, rare,
//! and whole: this is a resource row, not a log.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use brain_protocol::{EnvironmentId, EnvironmentStatus, Resources, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentRecord {
    pub session_id: SessionId,
    pub environment_id: EnvironmentId,
    pub configuration: serde_json::Value,
    pub created_at_ms: u64,
    #[serde(default)]
    pub resources: Resources,
}

struct Row {
    record: EnvironmentRecord,
    status: EnvironmentStatus,
}

pub struct EnvironmentResources {
    directory: PathBuf,
    rows: Mutex<HashMap<EnvironmentId, Row>>,
}

impl EnvironmentResources {
    /// Opens every environment under `directory`. A file that will not parse is skipped
    /// with a warning: one damaged row must not stop a process starting.
    pub fn open(directory: &Path) -> Result<Self, brain::Error> {
        fs::create_dir_all(directory).map_err(io)?;
        if let Some(parent) = directory.parent() {
            crate::persistence::sync_directory(parent)?;
        }
        let mut rows = HashMap::new();
        for entry in fs::read_dir(directory).map_err(io)?.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            match fs::read(&path)
                .map_err(io)
                .and_then(|bytes| serde_json::from_slice::<EnvironmentRecord>(&bytes).map_err(json))
            {
                Ok(record) => {
                    rows.insert(
                        record.environment_id.clone(),
                        Row {
                            record,
                            status: EnvironmentStatus::Unknown,
                        },
                    );
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "environment record skipped");
                }
            }
        }
        Ok(Self {
            directory: directory.to_path_buf(),
            rows: Mutex::new(rows),
        })
    }

    pub fn create(&self, record: EnvironmentRecord) -> Result<(), brain::Error> {
        let mut rows = self.lock()?;
        if rows.contains_key(&record.environment_id) {
            return Err(brain::Error::Conflict(format!(
                "Environment `{}` already exists",
                record.environment_id
            )));
        }
        self.write(&record)?;
        rows.insert(
            record.environment_id.clone(),
            Row {
                record,
                status: EnvironmentStatus::Open,
            },
        );
        Ok(())
    }

    pub fn get(
        &self,
        environment_id: &EnvironmentId,
    ) -> Result<Option<EnvironmentRecord>, brain::Error> {
        Ok(self
            .lock()?
            .get(environment_id)
            .map(|row| row.record.clone()))
    }

    pub fn status(
        &self,
        environment_id: &EnvironmentId,
    ) -> Result<Option<EnvironmentStatus>, brain::Error> {
        Ok(self
            .lock()?
            .get(environment_id)
            .map(|row| row.status.clone()))
    }

    pub fn ids(&self) -> Result<Vec<EnvironmentId>, brain::Error> {
        let mut ids: Vec<EnvironmentId> = self.lock()?.keys().cloned().collect();
        ids.sort();
        Ok(ids)
    }

    /// Changes a record and writes it back.
    pub fn update(
        &self,
        environment_id: &EnvironmentId,
        change: impl FnOnce(&mut EnvironmentRecord),
    ) -> Result<EnvironmentRecord, brain::Error> {
        let mut rows = self.lock()?;
        let row = rows.get_mut(environment_id).ok_or_else(|| {
            brain::Error::NotFound(format!("Environment `{environment_id}` does not exist"))
        })?;
        let mut updated = row.record.clone();
        change(&mut updated);
        self.write(&updated)?;
        row.record = updated;
        Ok(row.record.clone())
    }

    pub fn set_status(
        &self,
        environment_id: &EnvironmentId,
        status: EnvironmentStatus,
    ) -> Result<bool, brain::Error> {
        let mut rows = self.lock()?;
        let Some(row) = rows.get_mut(environment_id) else {
            return Ok(false);
        };
        let changed = row.status != status;
        row.status = status;
        Ok(changed)
    }

    pub fn remove(&self, environment_id: &EnvironmentId) -> Result<(), brain::Error> {
        let mut rows = self.lock()?;
        match fs::remove_file(self.path(environment_id)) {
            Ok(()) => {
                crate::persistence::sync_directory(&self.directory)?;
                rows.remove(environment_id);
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                rows.remove(environment_id);
                Ok(())
            }
            Err(error) => Err(io(error)),
        }
    }

    fn write(&self, record: &EnvironmentRecord) -> Result<(), brain::Error> {
        let target = self.path(&record.environment_id);
        crate::persistence::write_json(&target, record)
    }

    fn path(&self, environment_id: &EnvironmentId) -> PathBuf {
        self.directory
            .join(format!("{}.json", environment_id.as_str()))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<EnvironmentId, Row>>, brain::Error> {
        self.rows
            .lock()
            .map_err(|_| brain::Error::InvalidState("environment table is poisoned".into()))
    }
}

pub fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn io(error: std::io::Error) -> brain::Error {
    brain::Error::Journal(error.to_string())
}

fn json(error: serde_json::Error) -> brain::Error {
    brain::Error::Journal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary() -> PathBuf {
        std::env::temp_dir().join(format!(
            "brain-environments-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    fn record(id: &str) -> EnvironmentRecord {
        EnvironmentRecord {
            session_id: SessionId::new("ses_owner"),
            environment_id: EnvironmentId::new(id),
            configuration: serde_json::json!({"image": "ubuntu"}),

            created_at_ms: 1,
            resources: Default::default(),
        }
    }

    #[test]
    fn records_survive_reopening_and_a_duplicate_is_a_conflict() {
        let directory = temporary();
        let resources = EnvironmentResources::open(&directory).unwrap();
        resources.create(record("env_a")).unwrap();
        assert!(matches!(
            resources.create(record("env_a")),
            Err(brain::Error::Conflict(_))
        ));
        resources
            .update(&EnvironmentId::new("env_a"), |record| {
                record.configuration = serde_json::json!({"image": "debian"});
            })
            .unwrap();
        drop(resources);
        let reopened = EnvironmentResources::open(&directory).unwrap();
        let found = reopened.get(&EnvironmentId::new("env_a")).unwrap().unwrap();
        assert_eq!(found.configuration["image"], "debian");
        assert_eq!(found.session_id.as_str(), "ses_owner");
        reopened.remove(&EnvironmentId::new("env_a")).unwrap();
        assert!(
            reopened
                .get(&EnvironmentId::new("env_a"))
                .unwrap()
                .is_none()
        );
        let _ = fs::remove_dir_all(directory);
    }
}
