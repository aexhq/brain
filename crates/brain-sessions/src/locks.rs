use std::{
    collections::HashMap,
    hash::Hash,
    sync::{Arc, Mutex as StdMutex, Weak},
};
use tokio::sync::Mutex;
pub struct KeyedLocks<K> {
    entries: StdMutex<Table<K>>,
}

struct Table<K> {
    locks: HashMap<K, Weak<Mutex<()>>>,
    /// Sweep once the table has doubled since the last sweep, rather than on every
    /// acquire. A sweep is O(table) under a process-global lock, and every mutating
    /// request acquires at least one lock, so sweeping per call made request cost grow
    /// with the number of live sessions.
    sweep_at: usize,
}

/// Below this a sweep is cheaper than the arithmetic deciding whether to sweep.
const MIN_SWEEP: usize = 16;

impl<K> Default for KeyedLocks<K> {
    fn default() -> Self {
        Self {
            entries: StdMutex::new(Table {
                locks: HashMap::new(),
                sweep_at: MIN_SWEEP,
            }),
        }
    }
}

impl<K: Clone + Eq + Hash> KeyedLocks<K> {
    pub fn acquire(&self, key: K) -> Result<Arc<Mutex<()>>, brain_protocol::ApiError> {
        let mut table = self
            .entries
            .lock()
            .map_err(|_| brain_protocol::ApiError::internal("keyed lock table is poisoned"))?;
        if table.locks.len() >= table.sweep_at {
            table.locks.retain(|_, lock| lock.strong_count() > 0);
            // Amortised: the next sweep is at least as far away as the work this one
            // did, so the total sweeping stays linear in the number of acquires.
            table.sweep_at = table.locks.len().saturating_mul(2).max(MIN_SWEEP);
        }
        if let Some(lock) = table.locks.get(&key).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        table.locks.insert(key, Arc::downgrade(&lock));
        Ok(lock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyed_locks_share_live_keys_and_reclaim_stale_keys() {
        let locks = KeyedLocks::default();
        let first = locks.acquire("same").unwrap();
        let replay = locks.acquire("same").unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &replay));
        drop(first);
        drop(replay);

        // Enough distinct keys to cross the first sweep threshold, so the dead entry
        // above is reclaimed rather than accumulating.
        let mut held = Vec::new();
        for key in KEYS {
            held.push(locks.acquire(key).unwrap());
        }
        assert!(
            locks.entries.lock().unwrap().locks.len() <= super::MIN_SWEEP,
            "a sweep must reclaim the dead key once the table reaches the threshold"
        );
        assert!(
            held.iter()
                .all(|lock| std::sync::Arc::strong_count(lock) == 1)
        );
    }

    /// Distinct static keys, so the test does not depend on a `String` key type.
    const KEYS: [&str; super::MIN_SWEEP] = [
        "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "k10", "k11", "k12", "k13",
        "k14", "k15",
    ];

    /// The table must not grow without bound when every key is used once and dropped,
    /// which is what an idempotency key looks like.
    #[test]
    fn keyed_locks_stay_bounded_under_single_use_keys() {
        let locks: KeyedLocks<u64> = KeyedLocks::default();
        for key in 0..10_000_u64 {
            drop(locks.acquire(key).unwrap());
        }
        let held = locks.entries.lock().unwrap().locks.len();
        assert!(
            held <= 2 * super::MIN_SWEEP,
            "single-use keys must not accumulate: {held} entries remain"
        );
    }
}
