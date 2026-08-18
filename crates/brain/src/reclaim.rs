//! Giving memory back to the operating system, on purpose and on a trigger.
//!
//! # Why this module exists
//!
//! PD-10 found that dropping 10,000 sessions returned **0.00%** of 13.9 GB to
//! the OS, and that one `malloc_trim(0)` recovered 99.96%. PD-13 repeated that
//! across three allocators at the target session shape and found something
//! worse than "glibc is the odd one out":
//!
//! | allocator | dropped 10,000 sessions, no explicit call | after an explicit call |
//! | --- | ---: | ---: |
//! | glibc | −0.01% of 5,826 MB | 99.18% |
//! | jemalloc (default `dirty_decay_ms=10000`) | 0.99% at 1.5 s, **0.02% at 25 s** | 97.07% |
//! | mimalloc (default purge delay) | −0.03% | 99.09% |
//!
//! **No allocator returns the memory on its own.** jemalloc's decay is the
//! interesting case: waiting 25 s against a 10 s decay returned *less* than
//! waiting 1.5 s, because jemalloc's decay is driven by allocator activity
//! rather than by a timer. A process that has just evicted its sessions and is
//! now idle is precisely the process that never triggers it.
//!
//! So the return has to be asked for, and the question PD-10 left open --
//! "`malloc_trim` is not the answer, only the proof ... there is no principled
//! place to call it" -- is answerable once you know what the call costs. PD-13
//! measured that too, at ~6 GB held:
//!
//! | mechanism | call duration |
//! | --- | ---: |
//! | `mi_collect(true)` (mimalloc) | 30.1 ms |
//! | `malloc_trim(0)` (glibc) | 225.8 ms |
//! | `arena.<all>.purge` (jemalloc) | 262.8 ms |
//!
//! A synchronous quarter-second is far too expensive per session and entirely
//! affordable per **eviction batch**. That is the principled place: not "when a
//! session ends" and not "on a timer", but "when enough has been freed that the
//! call pays for itself". `SessionManager` therefore counts the transcript
//! bytes it drops and calls this once per threshold's worth.
//!
//! # What this is not
//!
//! It is not a substitute for an allocator that can hand a session's memory
//! back as one `munmap` (arena-per-session), and it is not free. It is the
//! cheapest change that turns a resident-set high-water mark back into a
//! working set, and the threshold makes its cost bounded and predictable
//! rather than absent.

use std::sync::atomic::{AtomicU64, Ordering};

/// Bytes freed before an arena return is worth its own cost.
///
/// **256 MiB, chosen by measuring both candidates rather than by taste.**
/// Dropping 10,000 target-shape sessions holding 5,826 MB:
///
/// | threshold | returns | recovered on drop | total stall | per call |
/// | --- | ---: | ---: | ---: | ---: |
/// | disabled | 0 | **−0.01%** | 0 ms | — |
/// | 1 GiB | 4 | 82.7% | 416.6 ms | 104.2 ms |
/// | **256 MiB** | 19 | **98.15%** | 548.9 ms | **28.9 ms** |
///
/// 1 GiB leaves a gigabyte stranded, because the eviction ends mid-threshold
/// and the remainder never triggers. 256 MiB clears the pre-registered 95% bar
/// on its own, and -- the part that was not obvious -- each individual call is
/// **3.6x cheaper**, because a trim walks less. The larger threshold costs less
/// total time and more stranded memory *and* a longer worst-case stall, which
/// is the wrong trade on all three axes.
///
/// Configurable, and **0 disables it**: a deployment that would rather keep the
/// high-water mark than pay any stall can say so explicitly, and its density
/// figures can then be read as the high-water marks they are.
pub const DEFAULT_THRESHOLD_BYTES: u64 = 256 << 20;

static RETURNS: AtomicU64 = AtomicU64::new(0);
static RETURN_NS: AtomicU64 = AtomicU64::new(0);
static BYTES_TRIGGERED: AtomicU64 = AtomicU64::new(0);

/// What one arena return did. Reported rather than logged: a return that did
/// not happen and a return that recovered nothing are different states, and
/// only the counters can tell them apart afterwards.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ReclaimStats {
    /// How many times an arena return has been performed in this process.
    pub returns: u64,
    /// Total wall time spent inside those calls. The stall budget.
    pub total_ns: u64,
    /// Freed bytes that triggered them.
    pub bytes_triggered: u64,
}

pub fn stats() -> ReclaimStats {
    ReclaimStats {
        returns: RETURNS.load(Ordering::Relaxed),
        total_ns: RETURN_NS.load(Ordering::Relaxed),
        bytes_triggered: BYTES_TRIGGERED.load(Ordering::Relaxed),
    }
}

/// The name of the mechanism this build would use. Never "none" silently: a
/// build with no way to return memory says so, so a density figure taken on it
/// can be rejected rather than believed.
pub const MECHANISM: &str = if cfg!(all(unix, target_env = "gnu")) {
    "glibc malloc_trim(0)"
} else {
    "none available for this target"
};

/// Return free pages to the OS **now**. Synchronous, and it blocks the calling
/// thread for as long as it takes to walk every arena. Returns the duration so
/// a caller can account for the stall rather than discover it in a tail.
pub fn arena_return() -> u64 {
    let t = std::time::Instant::now();
    #[cfg(all(unix, target_env = "gnu"))]
    {
        // Safety: `malloc_trim` takes no pointers and is safe to call at any
        // time from any thread.
        unsafe {
            libc::malloc_trim(0);
        }
    }
    let ns = t.elapsed().as_nanos() as u64;
    RETURNS.fetch_add(1, Ordering::Relaxed);
    RETURN_NS.fetch_add(ns, Ordering::Relaxed);
    ns
}

/// Accumulates freed bytes and performs an arena return once the threshold is
/// crossed.
///
/// The counter is decremented by the threshold rather than reset to zero, so a
/// burst that frees ten thousand sessions at once triggers the return once and
/// keeps the remainder toward the next one, instead of throwing it away.
#[derive(Debug)]
pub struct ReclaimPolicy {
    threshold: AtomicU64,
    pending: AtomicU64,
}

impl Default for ReclaimPolicy {
    fn default() -> Self {
        ReclaimPolicy::new(DEFAULT_THRESHOLD_BYTES)
    }
}

impl ReclaimPolicy {
    pub fn new(threshold_bytes: u64) -> Self {
        ReclaimPolicy {
            threshold: AtomicU64::new(threshold_bytes),
            pending: AtomicU64::new(0),
        }
    }

    pub fn threshold_bytes(&self) -> u64 {
        self.threshold.load(Ordering::Relaxed)
    }

    pub fn set_threshold_bytes(&self, n: u64) {
        self.threshold.store(n, Ordering::Relaxed);
    }

    pub fn pending_bytes(&self) -> u64 {
        self.pending.load(Ordering::Relaxed)
    }

    /// Record `bytes` of freed transcript. Returns `Some(duration_ns)` if this
    /// call performed an arena return.
    pub fn freed(&self, bytes: u64) -> Option<u64> {
        let threshold = self.threshold.load(Ordering::Relaxed);
        if threshold == 0 {
            return None;
        }
        let before = self.pending.fetch_add(bytes, Ordering::Relaxed);
        if before + bytes < threshold {
            return None;
        }
        // Claim exactly one threshold's worth. If two threads cross at once,
        // whichever loses the subtraction race simply does not trim, so the
        // stall is paid once rather than once per thread.
        if self
            .pending
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| {
                if p >= threshold {
                    Some(p - threshold)
                } else {
                    None
                }
            })
            .is_err()
        {
            return None;
        }
        BYTES_TRIGGERED.fetch_add(threshold, Ordering::Relaxed);
        Some(arena_return())
    }

    /// Return now regardless of the threshold, and clear the pending count.
    /// For an operator-driven "give it back" rather than the automatic policy.
    pub fn force(&self) -> u64 {
        self.pending.store(0, Ordering::Relaxed);
        arena_return()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_threshold_of_zero_disables_the_policy() {
        // A deployment that would rather keep the high-water mark than pay a
        // 226 ms stall must be able to say so, and must not get a surprise
        // trim at some other threshold.
        let p = ReclaimPolicy::new(0);
        assert!(p.freed(u64::MAX / 2).is_none());
        assert!(p.freed(u64::MAX / 2).is_none());
    }

    #[test]
    fn the_remainder_carries_to_the_next_return() {
        // Freeing 1.5 thresholds must trim once and leave 0.5 pending, not
        // trim once and throw the remainder away -- otherwise a workload that
        // frees in large bursts trims far less often than its byte volume says.
        let p = ReclaimPolicy::new(1000);
        assert!(p.freed(600).is_none(), "below the threshold, no return");
        assert!(p.freed(900).is_some(), "crossing it returns");
        assert_eq!(p.pending_bytes(), 500, "the remainder is kept");
    }

    #[test]
    fn one_huge_free_returns_once_not_once_per_threshold() {
        // Dropping 10,000 sessions at once frees ~6 GB. That must cost ONE
        // 226 ms stall, not six.
        let p = ReclaimPolicy::new(1 << 20);
        let before = stats().returns;
        assert!(p.freed(64 << 20).is_some());
        assert_eq!(stats().returns - before, 1, "one burst, one return");
    }

    #[test]
    fn the_mechanism_is_named_never_silently_absent() {
        assert!(!MECHANISM.is_empty());
        if cfg!(all(unix, target_env = "gnu")) {
            assert!(MECHANISM.contains("malloc_trim"));
        }
    }
}
