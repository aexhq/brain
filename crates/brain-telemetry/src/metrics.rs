use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

#[derive(Clone, Default)]
pub struct TelemetryMetrics {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    queued_records: AtomicUsize,
    queued_bytes: AtomicUsize,
    dropped_records: AtomicU64,
}

impl TelemetryMetrics {
    pub fn queued_records(&self) -> usize {
        self.inner.queued_records.load(Ordering::Relaxed)
    }
    pub fn queued_bytes(&self) -> usize {
        self.inner.queued_bytes.load(Ordering::Relaxed)
    }
    pub fn dropped_records(&self) -> u64 {
        self.inner.dropped_records.load(Ordering::Relaxed)
    }

    pub(crate) fn accepted(&self, bytes: usize) {
        self.inner.queued_records.fetch_add(1, Ordering::Relaxed);
        self.inner.queued_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn removed(&self, bytes: usize) {
        self.inner.queued_records.fetch_sub(1, Ordering::Relaxed);
        self.inner.queued_bytes.fetch_sub(bytes, Ordering::Relaxed);
    }

    pub(crate) fn dropped(&self) {
        self.inner.dropped_records.fetch_add(1, Ordering::Relaxed);
    }
}
