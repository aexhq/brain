//! Percentiles that refuse to lie. Ported from the prototype harness; the `n/a<100` string is
//! deliberate — a consumer that formats it blindly prints the disclaimer, and one that does
//! arithmetic on it crashes. Both beat a plausible-looking p99 from n=30.

pub const P99_MIN_N: usize = 100;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Summary {
    pub n: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    /// String `n/a<100` when unsupportable.
    pub p99: serde_json::Value,
}

pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let (lo, hi) = (rank.floor() as usize, rank.ceil() as usize);
    if lo == hi {
        return sorted[lo];
    }
    sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo as f64)
}

pub fn summarize(values: &[f64]) -> Summary {
    let mut v: Vec<f64> = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    let mean = if n == 0 {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / n as f64
    };
    Summary {
        n,
        min: v.first().copied().unwrap_or(f64::NAN),
        max: v.last().copied().unwrap_or(f64::NAN),
        mean,
        p50: percentile(&v, 50.0),
        p90: percentile(&v, 90.0),
        p95: percentile(&v, 95.0),
        p99: if n >= P99_MIN_N {
            serde_json::json!(percentile(&v, 99.0))
        } else {
            serde_json::json!(format!("n/a<{P99_MIN_N}"))
        },
    }
}

/// p99 as a number, or `max` when n is too small to support one — the conservative substitute
/// for a gate comparison.
pub fn p99_or_max(s: &Summary) -> f64 {
    s.p99.as_f64().unwrap_or(s.max)
}
