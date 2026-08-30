//! Percentiles, and the rule that stops the runner printing one it cannot support.

use crate::schema::Percentiles;

/// Below this many samples a p99 is the maximum wearing a costume, so it is not emitted.
pub const MIN_N_P99: usize = 100;
/// Below this, neither is a median worth quoting.
pub const MIN_N_P50: usize = 20;

/// Nearest-rank percentile over already-sorted samples.
fn at(sorted: &[f64], q: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Summarizes latency samples, populating only the percentiles `n` supports. A caller
/// that wanted a p99 and got `None` has too few samples, not a bug.
pub fn summarize(samples: &mut [f64]) -> Percentiles {
    if samples.is_empty() {
        return Percentiles::default();
    }
    samples.sort_by(f64::total_cmp);
    Percentiles {
        p50_ms: (samples.len() >= MIN_N_P50).then(|| at(samples, 0.50)),
        p90_ms: (samples.len() >= MIN_N_P50).then(|| at(samples, 0.90)),
        p99_ms: (samples.len() >= MIN_N_P99).then(|| at(samples, 0.99)),
        min_ms: samples.first().copied(),
        max_ms: samples.last().copied(),
    }
}

/// Discards the warm-up head of a sample set. Every subject gets the same fraction, so no
/// subject is credited for a slow start another one was charged for.
pub fn drop_warmup(samples: Vec<f64>, fraction: f64) -> Vec<f64> {
    let skip = (samples.len() as f64 * fraction).floor() as usize;
    if skip >= samples.len() {
        return samples;
    }
    samples.into_iter().skip(skip).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn withholds_percentiles_the_sample_count_cannot_support() {
        let mut few: Vec<f64> = (0..10).map(f64::from).collect();
        let summary = summarize(&mut few);
        assert!(summary.p50_ms.is_none(), "10 samples cannot support a p50");
        assert!(summary.p99_ms.is_none());
        assert_eq!(summary.max_ms, Some(9.0));

        let mut enough: Vec<f64> = (0..100).map(f64::from).collect();
        let summary = summarize(&mut enough);
        assert_eq!(summary.p50_ms, Some(49.0));
        assert_eq!(summary.p99_ms, Some(98.0));
    }

    /// The slope is the per-session memory figure, so the fit is load-bearing arithmetic.
    #[test]
    fn a_clean_ramp_gives_the_slope_and_drops_the_fixed_floor() {
        // 40 MiB of runtime floor plus 22 KiB per session.
        let points: Vec<(f64, f64)> = (0..=8)
            .map(|step| {
                let sessions = (step * 64) as f64;
                (sessions, 40_960.0 + 22.0 * sessions)
            })
            .collect();
        let fit = crate::schema::Fit::least_squares(&points).expect("fits");
        assert!((fit.slope - 22.0).abs() < 1e-6, "slope was {}", fit.slope);
        assert!(
            (fit.intercept - 40_960.0).abs() < 1e-6,
            "the intercept is the fixed floor the slope must not include"
        );
        assert!(fit.r2 > 0.999);
    }

    /// A subject whose memory grows non-linearly has no single per-session cost, and r² is
    /// what stops the slope being quoted as if it did.
    #[test]
    fn a_curved_ramp_reports_a_poor_fit() {
        let points: Vec<(f64, f64)> = (0..=8)
            .map(|step| {
                let sessions = (step * 64) as f64;
                (sessions, 40_960.0 + sessions * sessions * 0.05)
            })
            .collect();
        let fit = crate::schema::Fit::least_squares(&points).expect("fits");
        assert!(
            fit.r2 < 0.95,
            "a quadratic must not pass as linear; r² was {}",
            fit.r2
        );
    }

    #[test]
    fn a_ramp_with_too_few_steps_refuses_to_fit() {
        assert!(crate::schema::Fit::least_squares(&[(0.0, 1.0), (1.0, 2.0)]).is_none());
    }

    #[test]
    fn nearest_rank_picks_the_sample_not_an_interpolation() {
        let mut samples = vec![1.0, 2.0, 3.0, 4.0];
        samples.sort_by(f64::total_cmp);
        assert_eq!(at(&samples, 0.5), 2.0);
        assert_eq!(at(&samples, 1.0), 4.0);
    }
}
