//! What a Brain data directory holds, and the refusal to open one from before the
//! per-session layout.
//!
//! ```text
//! {data_dir}/format             "brain-data/1"
//! {data_dir}/sessions/{id}/     one directory per session (see brain::SessionStore)
//! {data_dir}/agentloops         admitted loop packages
//! {data_dir}/run                worker sockets
//! {data_dir}/server-metadata    model credentials
//! ```

use std::path::{Path, PathBuf};

pub const FORMAT: &str = "brain-data/1";

/// Creates the layout if the directory is new, checks it if it is not, and returns the
/// sessions directory. A directory written by an earlier Brain is refused with a message
/// that names it: there is no migration from the shared journal, and starting over it
/// would silently begin a second history beside the first.
pub fn prepare(data_dir: &Path) -> Result<PathBuf, brain::Error> {
    std::fs::create_dir_all(data_dir).map_err(io)?;
    let marker = data_dir.join("format");
    match std::fs::read_to_string(&marker) {
        Ok(found) if found.trim() == FORMAT => {}
        Ok(found) => {
            return Err(brain::Error::InvalidState(format!(
                "data directory {} is format {found:?}; this Brain reads {FORMAT} and does not migrate",
                data_dir.display()
            )));
        }
        Err(_) if data_dir.join("journal").join("journal").exists() => {
            return Err(brain::Error::InvalidState(format!(
                "data directory {} holds a shared journal from an earlier Brain; this Brain keeps one directory per session and does not migrate. Start with an empty data directory",
                data_dir.display()
            )));
        }
        Err(_) => {
            std::fs::write(&marker, format!("{FORMAT}\n")).map_err(io)?;
        }
    }
    let sessions = data_dir.join("sessions");
    std::fs::create_dir_all(&sessions).map_err(io)?;
    Ok(sessions)
}

fn io(error: std::io::Error) -> brain::Error {
    brain::Error::Journal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "brain-data-layout-{name}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn a_new_directory_is_marked_and_reopens() {
        let data_dir = temporary("new");
        let sessions = prepare(&data_dir).unwrap();
        assert!(sessions.is_dir());
        assert_eq!(
            std::fs::read_to_string(data_dir.join("format"))
                .unwrap()
                .trim(),
            FORMAT
        );
        prepare(&data_dir).unwrap();
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn a_shared_journal_from_an_earlier_brain_is_refused() {
        let data_dir = temporary("legacy");
        std::fs::create_dir_all(data_dir.join("journal").join("journal")).unwrap();
        let error = prepare(&data_dir).unwrap_err().to_string();
        assert!(error.contains("does not migrate"), "{error}");
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn an_unknown_format_is_refused() {
        let data_dir = temporary("unknown");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::write(
            data_dir.join("format"),
            "brain-data/9
",
        )
        .unwrap();
        let error = prepare(&data_dir).unwrap_err().to_string();
        assert!(error.contains("brain-data/9"), "{error}");
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
