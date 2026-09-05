//! Durable HTTP operation claims and answers. An unresolved claim is never executed again.

use std::{
    collections::HashMap,
    fs::File,
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use brain_protocol::Identity;

/// Completed answers expire; unresolved claims remain to prevent duplicate effects.
pub const DEFAULT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// Below this an eviction sweep costs more than the entries it would find.
const MIN_SWEEP: usize = 64;

pub struct IdempotencyStore {
    state: Mutex<State>,
    retention: Duration,
}

struct State {
    log: File,
    entries: HashMap<(String, String), Stored>,
    /// Sweep expired entries once the table has doubled since the last sweep, so the
    /// total cost of sweeping stays linear in the number of entries written.
    sweep_at: usize,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Stored {
    /// The request the answer was given to, compared on every hit: the same key with a
    /// different request is a different request wearing that key's name, and an error.
    request: Identity,
    response: Option<serde_json::Value>,
    expires_at_ms: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Record {
    scope: String,
    key: String,
    stored: Stored,
}

impl IdempotencyStore {
    pub fn open(path: &Path, retention: Duration) -> Result<Self, brain::Error> {
        let (log, records) = crate::persistence::open_log::<Record>(path)?;
        let now = wall_clock_ms()?;
        let mut entries = HashMap::new();
        for record in records {
            entries.insert((record.scope, record.key), record.stored);
        }
        entries.retain(|_, stored| stored.response.is_none() || stored.expires_at_ms > now);
        Ok(Self {
            state: Mutex::new(State {
                log,
                entries,
                sweep_at: MIN_SWEEP,
            }),
            retention,
        })
    }

    /// Replays a completed answer or durably claims a new request before the caller acts.
    pub fn get<T: serde::Serialize>(
        &self,
        scope: &str,
        key: &str,
        request: &T,
    ) -> Result<Option<serde_json::Value>, brain::Error> {
        let request = digest(request)?;
        let now = wall_clock_ms()?;
        let mut state = self.lock()?;
        let entry = (scope.to_string(), key.to_string());
        match state.entries.get(&entry) {
            None => self.claim(&mut state, entry, request),
            // Past its retention it is not a record any more. Dropped here rather than
            // returned, so the caller executes the request instead of replaying an
            // answer Brain has stopped promising to keep.
            Some(stored) if stored.response.is_some() && stored.expires_at_ms <= now => {
                state.entries.remove(&entry);
                self.claim(&mut state, entry, request)
            }
            Some(stored) if stored.request != request => Err(brain::Error::Conflict(
                "idempotency key reused with different request".into(),
            )),
            Some(stored) => stored.response.clone().map(Some).ok_or_else(|| brain::Error::Ambiguous(
                "this request was already accepted but its outcome is not recorded; it will not be executed again".into()
            )),
        }
    }

    fn claim(
        &self,
        state: &mut State,
        entry: (String, String),
        request: Identity,
    ) -> Result<Option<serde_json::Value>, brain::Error> {
        let stored = Stored {
            request,
            response: None,
            expires_at_ms: u64::MAX,
        };
        crate::persistence::append(
            &mut state.log,
            &Record {
                scope: entry.0.clone(),
                key: entry.1.clone(),
                stored: stored.clone(),
            },
        )?;
        state.entries.insert(entry, stored);
        Ok(None)
    }

    pub fn put<T: serde::Serialize>(
        &self,
        scope: &str,
        key: &str,
        request: &T,
        response: &serde_json::Value,
    ) -> Result<(), brain::Error> {
        let request = digest(request)?;
        let now = wall_clock_ms()?;
        let expires_at_ms =
            now.saturating_add(u64::try_from(self.retention.as_millis()).unwrap_or(u64::MAX));
        let mut state = self.lock()?;
        let entry = (scope.to_string(), key.to_string());
        if state.entries.get(&entry).is_some_and(|stored| {
            stored.request != request || (stored.response.is_some() && stored.expires_at_ms > now)
        }) {
            return Err(brain::Error::Conflict(
                "idempotency key already recorded".into(),
            ));
        }
        if state.entries.len() >= state.sweep_at {
            state
                .entries
                .retain(|_, stored| stored.response.is_none() || stored.expires_at_ms > now);
            state.sweep_at = state.entries.len().saturating_mul(2).max(MIN_SWEEP);
        }
        let stored = Stored {
            request,
            response: Some(response.clone()),
            expires_at_ms,
        };
        crate::persistence::append(
            &mut state.log,
            &Record {
                scope: scope.into(),
                key: key.into(),
                stored: stored.clone(),
            },
        )?;
        state.entries.insert(entry, stored);
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, brain::Error> {
        self.state
            .lock()
            .map_err(|_| brain::Error::InvalidState("idempotency table poisoned".into()))
    }
}

fn digest<T: serde::Serialize>(request: &T) -> Result<Identity, brain::Error> {
    crate::digest::identity_of(request)
}

fn wall_clock_ms() -> Result<u64, brain::Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| brain::Error::InvalidState("system time is before the Unix epoch".into()))?
        .as_millis()
        .try_into()
        .map_err(|_| brain::Error::InvalidState("system time exceeds the clock range".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(retention: Duration) -> IdempotencyStore {
        IdempotencyStore::open(
            &std::env::temp_dir()
                .join(format!("brain-idempotency-{}", rand::random::<u64>()))
                .join("requests.log"),
            retention,
        )
        .unwrap()
    }

    #[test]
    fn a_recorded_answer_is_replayed_for_the_same_request_only() {
        let store = store(Duration::from_secs(60));
        store
            .put(
                "create",
                "key",
                &"request",
                &serde_json::json!({"ok": true}),
            )
            .unwrap();
        assert_eq!(
            store.get("create", "key", &"request").unwrap(),
            Some(serde_json::json!({"ok": true}))
        );
        assert!(store.get("create", "key", &"other request").is_err());
        assert_eq!(store.get("create", "other key", &"request").unwrap(), None);
    }

    #[test]
    fn restart_replays_answers_and_never_reexecutes_unfinished_requests() {
        let path = std::env::temp_dir()
            .join(format!("brain-requests-{}", rand::random::<u64>()))
            .join("requests.log");
        let store = IdempotencyStore::open(&path, DEFAULT_RETENTION).unwrap();
        assert_eq!(store.get("create", "done", &"body").unwrap(), None);
        store
            .put(
                "create",
                "done",
                &"body",
                &serde_json::json!({"id": "ses_1"}),
            )
            .unwrap();
        assert_eq!(store.get("create", "pending", &"body").unwrap(), None);
        drop(store);
        let store = IdempotencyStore::open(&path, DEFAULT_RETENTION).unwrap();
        assert_eq!(
            store.get("create", "done", &"body").unwrap(),
            Some(serde_json::json!({"id": "ses_1"}))
        );
        assert!(matches!(
            store.get("create", "pending", &"body"),
            Err(brain::Error::Ambiguous(_))
        ));
        assert!(matches!(
            store.get("create", "pending", &"changed"),
            Err(brain::Error::Conflict(_))
        ));
    }

    #[test]
    fn an_expired_answer_is_not_replayed() {
        let store = store(Duration::ZERO);
        store
            .put("create", "key", &"request", &serde_json::json!({}))
            .unwrap();
        assert_eq!(store.get("create", "key", &"request").unwrap(), None);
    }

    #[test]
    fn expired_answers_are_swept_as_the_table_grows() {
        let store = store(Duration::ZERO);
        for index in 0..(2 * MIN_SWEEP) {
            store
                .put("scope", &index.to_string(), &index, &serde_json::json!({}))
                .unwrap();
        }
        let held = store.lock().unwrap().entries.len();
        assert!(
            held <= MIN_SWEEP,
            "expired entries must not accumulate: {held} remain"
        );
    }
}
