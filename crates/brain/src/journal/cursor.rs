use brain_protocol::EventPage;

use crate::journal::SessionRecord;

pub fn event_page(records: Vec<SessionRecord>, after: u64) -> EventPage {
    let next_cursor = records.last().map_or(after, |record| record.sequence);
    let events = records.into_iter().map(SessionRecord::into_event).collect();
    EventPage {
        events,
        next_cursor,
    }
}
