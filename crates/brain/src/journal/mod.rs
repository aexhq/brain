mod cursor;
mod log;
mod observed;
mod record;
mod segment;
mod store;

pub use cursor::event_page;
pub use observed::ObservedJournal;
pub use record::{AppendRecord, JournalRecord};
pub use segment::SegmentJournal;
pub use store::{JournalStore, SessionRow, SessionUpdate};

use brain_protocol::SessionStatus;

use crate::Error;

/// Closes turns that the previous process did not finish.
///
/// A session still `Running` after the journal has been read was mid-turn when that
/// process stopped. Whether the model call or the tool call actually happened is not
/// knowable from here, so Brain says exactly that and returns the session to Idle rather
/// than deciding on the client's behalf. Agentloops, tools and SDK clients see
/// `turn_interrupted` on the event stream and resume or abandon as suits them.
///
/// The host calls this once, after opening the store and before serving anything.
pub fn interrupt_unfinished_turns(store: &dyn JournalStore) -> Result<(), Error> {
    for session in store.session_summaries()? {
        if !matches!(session.status, SessionStatus::Running) {
            continue;
        }
        store.append(
            &session.session_id,
            session.last_sequence,
            &[AppendRecord::new(
                "turn_interrupted",
                serde_json::json!({
                    "message": "Brain restarted while this turn was in flight; whether its effects reached the model or a tool is not recorded"
                }),
            )],
            SessionUpdate {
                status: Some(SessionStatus::Idle),
                context: None,
                configuration: None,
            },
        )?;
    }
    Ok(())
}
