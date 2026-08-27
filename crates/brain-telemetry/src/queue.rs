use std::{collections::VecDeque, time::Instant};

use crate::TelemetryRecord;

pub(crate) struct QueuedRecord {
    pub record: TelemetryRecord,
    pub bytes: usize,
    pub enqueued_at: Instant,
}

pub(crate) struct BoundedQueue {
    records: VecDeque<QueuedRecord>,
    bytes: usize,
    max_records: usize,
    max_bytes: usize,
}

impl BoundedQueue {
    pub fn new(max_records: usize, max_bytes: usize) -> Self {
        Self { records: VecDeque::new(), bytes: 0, max_records, max_bytes }
    }

    pub fn try_push(&mut self, record: TelemetryRecord) -> Result<usize, TelemetryRecord> {
        let bytes = record.encoded_len();
        if bytes > self.max_bytes || self.records.len() >= self.max_records || self.bytes + bytes > self.max_bytes {
            return Err(record);
        }
        self.bytes += bytes;
        self.records.push_back(QueuedRecord { record, bytes, enqueued_at: Instant::now() });
        Ok(bytes)
    }

    pub fn pop(&mut self) -> Option<QueuedRecord> {
        let queued = self.records.pop_front()?;
        self.bytes -= queued.bytes;
        Some(queued)
    }
}
