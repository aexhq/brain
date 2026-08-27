use std::time::{Duration, Instant};

pub(crate) fn retry_delay(attempt: u32, enqueued_at: Instant, max_age: Duration) -> Option<Duration> {
    let elapsed = enqueued_at.elapsed();
    if elapsed >= max_age { return None; }
    let millis = 10_u64.saturating_mul(2_u64.saturating_pow(attempt.min(8)));
    Some(Duration::from_millis(millis.min(1_000)).min(max_age - elapsed))
}
