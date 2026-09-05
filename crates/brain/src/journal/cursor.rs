use brain_protocol::{Event, EventPage};

use crate::journal::SessionRecord;

pub fn event_page(records: Vec<SessionRecord>, after: u64) -> EventPage {
    let next_cursor = records.last().map_or(after, |record| record.sequence);
    let events = records
        .into_iter()
        .map(|record| Event {
            event_id: record.event_id(),
            sequence: record.sequence,
            recorded_at_ms: record.recorded_at_ms,
            event_type: record.kind,
            data: record.payload,
        })
        .collect();
    EventPage {
        events,
        next_cursor,
    }
}
