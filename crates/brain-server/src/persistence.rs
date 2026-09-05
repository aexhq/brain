use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    path::Path,
};

use serde::{Serialize, de::DeserializeOwned};

pub(crate) fn sync_directory(path: &Path) -> Result<(), brain::Error> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(io)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn open_log<T: DeserializeOwned>(path: &Path) -> Result<(File, Vec<T>), brain::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| brain::Error::Journal("log has no directory".into()))?;
    fs::create_dir_all(parent).map_err(io)?;
    if let Some(root) = parent.parent() {
        sync_directory(root)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(io)?;
    let mut records = Vec::new();
    let mut offset = 0;
    {
        let mut reader = BufReader::new(&mut file);
        let mut line = Vec::new();
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line).map_err(io)? == 0 {
                break;
            }
            if !line.ends_with(b"\n") {
                break;
            }
            records.push(
                serde_json::from_slice(&line)
                    .map_err(|error| brain::Error::Journal(error.to_string()))?,
            );
            offset += line.len() as u64;
        }
    }
    file.set_len(offset).map_err(io)?;
    file.seek(SeekFrom::End(0)).map_err(io)?;
    file.sync_all().map_err(io)?;
    sync_directory(parent)?;
    Ok((file, records))
}

pub(crate) fn append<T: Serialize>(file: &mut File, record: &T) -> Result<(), brain::Error> {
    let mut bytes =
        serde_json::to_vec(record).map_err(|error| brain::Error::Journal(error.to_string()))?;
    bytes.push(b'\n');
    let before = file.metadata().map_err(io)?.len();
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_data()) {
        // Do not let a failed partial append hide later acknowledged records.
        file.set_len(before)
            .and_then(|()| file.sync_data())
            .map_err(io)?;
        file.seek(SeekFrom::End(0)).map_err(io)?;
        return Err(io(error));
    }
    Ok(())
}

pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), brain::Error> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| brain::Error::Journal(error.to_string()))?;
    let temporary = path.with_extension("json.tmp");
    let mut file = File::create(&temporary).map_err(io)?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(io)?;
    drop(file);
    fs::rename(&temporary, path).map_err(io)?;
    sync_directory(path.parent().expect("resource file has a directory"))
}

fn io(error: std::io::Error) -> brain::Error {
    brain::Error::Journal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn torn_tail_is_removed_before_new_records_are_acknowledged() {
        let path = std::env::temp_dir()
            .join(format!("brain-log-{}", rand::random::<u64>()))
            .join("records.log");
        let (mut file, _) = open_log::<String>(&path).unwrap();
        append(&mut file, &"first").unwrap();
        file.write_all(b"{\"torn\"").unwrap();
        drop(file);
        let (mut file, records) = open_log::<String>(&path).unwrap();
        assert_eq!(records, vec!["first"]);
        append(&mut file, &"second").unwrap();
        drop(file);
        let (_, records) = open_log::<String>(&path).unwrap();
        assert_eq!(records, vec!["first", "second"]);
    }

    #[test]
    fn complete_corrupt_record_fails_closed() {
        let path = std::env::temp_dir()
            .join(format!("brain-log-{}", rand::random::<u64>()))
            .join("records.log");
        let (mut file, _) = open_log::<String>(&path).unwrap();
        file.write_all(b"corrupt\n").unwrap();
        drop(file);
        assert!(open_log::<String>(&path).is_err());
    }
}
