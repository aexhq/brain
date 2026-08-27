use brain_protocol::{Event, EventId, EventPage};

use crate::journal::JournalRecord;

pub fn event_page(records: Vec<JournalRecord>, after: u64) -> EventPage {
    let next_cursor = records.last().map_or(after, |record| record.sequence);
    let events = records
        .into_iter()
        .map(|record| Event {
            event_id: EventId::new(format!(
                "evt_{}_{}",
                record.session_id.as_str(),
                record.sequence
            )),
            sequence: record.sequence,
            event_type: record.kind,
            data: record.payload,
        })
        .collect();
    EventPage {
        events,
        next_cursor,
    }
}
