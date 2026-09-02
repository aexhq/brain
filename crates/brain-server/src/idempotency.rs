//! Answers already given to requests that carried an idempotency key.
//!
//! An HTTP concern, so it lives in the server: a retried create, message, or tool result
//! is answered from here instead of being run again. Held in memory with a retention
//! window. It does not survive a restart on purpose: what these answers are about is
//! sessions, and a replayed `POST /v1/sessions` answer after a restart would hand the
//! client a session id that refers to nothing.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use brain_protocol::Identity;

/// How long a recorded answer is kept. Far longer than any retry a client makes and far
/// shorter than forever, so memory is bounded by traffic within the window.
pub const DEFAULT_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// Below this an eviction sweep costs more than the entries it would find.
const MIN_SWEEP: usize = 64;

pub struct IdempotencyStore {
    state: Mutex<State>,
    retention: Duration,
}

struct State {
    entries: HashMap<(String, String), Stored>,
    /// Sweep expired entries once the table has doubled since the last sweep, so the
    /// total cost of sweeping stays linear in the number of entries written.
    sweep_at: usize,
}

struct Stored {
    /// The request the answer was given to, compared on every hit: the same key with a
    /// different request is a different request wearing that key's name, and an error.
    request: Identity,
    response: serde_json::Value,
    expires_at_ms: u64,
}

impl IdempotencyStore {
    pub fn new(retention: Duration) -> Self {
        Self {
            state: Mutex::new(State {
                entries: HashMap::new(),
                sweep_at: MIN_SWEEP,
            }),
            retention,
        }
    }

    /// The answer already recorded under `key`, if the request is the same one.
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
            None => Ok(None),
            // Past its retention it is not a record any more. Dropped here rather than
            // returned, so the caller executes the request instead of replaying an
            // answer Brain has stopped promising to keep.
            Some(stored) if stored.expires_at_ms <= now => {
                state.entries.remove(&entry);
                Ok(None)
            }
            Some(stored) if stored.request != request => Err(brain::Error::InvalidState(
                "idempotency key reused with different request".into(),
            )),
            Some(stored) => Ok(Some(stored.response.clone())),
        }
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
        if state
            .entries
            .get(&entry)
            .is_some_and(|stored| stored.expires_at_ms > now)
        {
            return Err(brain::Error::InvalidState(
                "idempotency key already recorded".into(),
            ));
        }
        if state.entries.len() >= state.sweep_at {
            state.entries.retain(|_, stored| stored.expires_at_ms > now);
            state.sweep_at = state.entries.len().saturating_mul(2).max(MIN_SWEEP);
        }
        state.entries.insert(
            entry,
            Stored {
                request,
                response: response.clone(),
                expires_at_ms,
            },
        );
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, brain::Error> {
        self.state
            .lock()
            .map_err(|_| brain::Error::InvalidState("idempotency table poisoned".into()))
    }
}

fn digest<T: serde::Serialize>(request: &T) -> Result<Identity, brain::Error> {
    Identity::of(request).map_err(|error| brain::Error::InvalidState(error.to_string()))
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

    #[test]
    fn a_recorded_answer_is_replayed_for_the_same_request_only() {
        let store = IdempotencyStore::new(Duration::from_secs(60));
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
    fn an_expired_answer_is_not_replayed() {
        let store = IdempotencyStore::new(Duration::ZERO);
        store
            .put("create", "key", &"request", &serde_json::json!({}))
            .unwrap();
        assert_eq!(store.get("create", "key", &"request").unwrap(), None);
    }

    #[test]
    fn expired_answers_are_swept_as_the_table_grows() {
        let store = IdempotencyStore::new(Duration::ZERO);
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
