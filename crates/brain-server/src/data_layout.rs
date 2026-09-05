//! Brain's v1 data directory.
//!
//! ```text
//! {data_dir}/format             "brain-data/1"
//! {data_dir}/sessions/{id}/     one directory per session (see brain::LocalSessionStore)
//! {data_dir}/agentloops         admitted Agentloop and Tool Components
//! {data_dir}/native-workspaces  session workspaces for Brain Wasm Environments
//! {data_dir}/run                worker sockets
//! {data_dir}/server-metadata    model credentials
//! ```

use std::path::{Path, PathBuf};

pub const FORMAT: &str = "brain-data/1";

/// Hold this handle for the server lifetime, before initializing any mutable stores.
pub fn lock(data_dir: &Path) -> Result<std::fs::File, brain::Error> {
    std::fs::create_dir_all(data_dir).map_err(io)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data_dir.join(".lock"))
        .map_err(io)?;
    file.try_lock().map_err(|error| {
        brain::Error::Journal(format!(
            "data directory is already in use or cannot be locked: {error}"
        ))
    })?;
    Ok(file)
}

/// Initializes an empty directory or validates its format, then returns the sessions directory.
pub fn prepare(data_dir: &Path) -> Result<PathBuf, brain::Error> {
    std::fs::create_dir_all(data_dir).map_err(io)?;
    let marker = data_dir.join("format");
    match std::fs::read_to_string(&marker) {
        Ok(found) if found.trim() == FORMAT => {}
        Ok(found) => {
            return Err(brain::Error::InvalidState(format!(
                "data directory {} is format {found:?}; expected {FORMAT}",
                data_dir.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if std::fs::read_dir(data_dir)
                .map_err(io)?
                .find(|entry| {
                    !entry
                        .as_ref()
                        .is_ok_and(|entry| entry.file_name() == ".lock")
                })
                .transpose()
                .map_err(io)?
                .is_some()
            {
                return Err(brain::Error::InvalidState(format!(
                    "data directory {} is not empty and has no format marker",
                    data_dir.display()
                )));
            }
            std::fs::write(&marker, format!("{FORMAT}\n")).map_err(io)?;
            std::fs::OpenOptions::new()
                .write(true)
                .open(&marker)
                .map_err(io)?
                .sync_all()
                .map_err(io)?;
        }
        Err(error) => return Err(io(error)),
    }
    let sessions = data_dir.join("sessions");
    std::fs::create_dir_all(&sessions).map_err(io)?;
    crate::persistence::sync_directory(data_dir)?;
    if let Some(parent) = data_dir
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        crate::persistence::sync_directory(parent)?;
    }
    Ok(sessions)
}

fn io(error: std::io::Error) -> brain::Error {
    brain::Error::Journal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_writer_is_refused_until_the_handle_is_dropped() {
        let path = temporary("lock");
        let first = lock(&path).unwrap();
        prepare(&path).unwrap();
        assert!(lock(&path).is_err());
        drop(first);
        drop(lock(&path).unwrap());
        std::fs::remove_dir_all(path).unwrap();
    }

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
    fn an_unmarked_nonempty_directory_is_refused() {
        let data_dir = temporary("unmarked");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::write(data_dir.join("existing"), "keep").unwrap();
        let error = prepare(&data_dir).unwrap_err().to_string();
        assert!(error.contains("no format marker"), "{error}");
        assert!(!data_dir.join("format").exists());
        assert_eq!(
            std::fs::read_to_string(data_dir.join("existing")).unwrap(),
            "keep"
        );
        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn an_unknown_format_is_refused() {
        let data_dir = temporary("unknown");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::write(data_dir.join("format"), "unknown-format\n").unwrap();
        let error = prepare(&data_dir).unwrap_err().to_string();
        assert!(error.contains("unknown-format"), "{error}");
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
