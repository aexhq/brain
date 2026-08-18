//! Process memory sampling. Ported from the prototype harness (`prototype/brain/crates/
//! brain-bench/src/mem.rs`), which itself copied the method from the archived Node harness so
//! every density number in the record is comparable:
//!
//! ```text
//! Rss     = Pss + (shared pages attributed to others)
//! private = Private_Clean + Private_Dirty   <- pages that exist ONLY in this process
//! ```
//!
//! *private* answers "what does the Nth session add"; *Pss* answers "what does this process
//! occupy in the fleet". Both are recorded, never one alone. If `/proc/self/smaps_rollup` is
//! unreadable (macOS, Windows) this module REFUSES rather than falling back to RSS: a bare RSS
//! sum double-counts every shared page and would not be comparable with the banked PD-11
//! figures. Density and reclaim gates are Linux-only by construction; run them on the target.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Mem {
    pub rss_bytes: u64,
    pub pss_bytes: u64,
    pub private_clean_bytes: u64,
    pub private_dirty_bytes: u64,
    pub swap_bytes: u64,
}

impl Mem {
    pub fn private_bytes(&self) -> u64 {
        self.private_clean_bytes + self.private_dirty_bytes
    }
}

pub fn sample() -> Result<Mem, String> {
    let path = "/proc/self/smaps_rollup";
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "{path}: {e}. Refusing to report memory: an RSS fallback would double-count \
             shared pages and the number would not be comparable with the banked figures. \
             Run the density/reclaim arms on Linux (the production target)."
        )
    })?;
    let mut m = Mem::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let kb: u64 = v.trim().trim_end_matches(" kB").trim().parse().unwrap_or(0);
        let bytes = kb * 1024;
        match k {
            "Rss" => m.rss_bytes = bytes,
            "Pss" => m.pss_bytes = bytes,
            "Private_Clean" => m.private_clean_bytes = bytes,
            "Private_Dirty" => m.private_dirty_bytes = bytes,
            "Swap" => m.swap_bytes = bytes,
            _ => {}
        }
    }
    if m.rss_bytes == 0 && m.pss_bytes == 0 {
        return Err(format!("{path} parsed to all zeroes"));
    }
    Ok(m)
}
