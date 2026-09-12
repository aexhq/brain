use std::{collections::BTreeMap, fs, path::Path, process::Command, sync::Arc};

use brain::{AppendRecord, Feed, JournalEntry, LocalSessionStore, SessionStore, Writer};
use brain_protocol::{ContentBlock, FileMediaType, Message, Role, SessionId};
use serde_json::json;

fn files(directory: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut contents = BTreeMap::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            contents.extend(files(&path));
        } else if path.file_name().unwrap() != ".lock" {
            // Windows prevents reading the lock file while the writer holds it.
            contents.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    contents
}

#[test]
fn checker_requires_a_stopped_writer_and_checks_history_removed_from_the_projection() {
    let directory = tempfile::tempdir().unwrap();
    let lock = brain_server::data_layout::lock(directory.path()).unwrap();
    let sessions = brain_server::data_layout::prepare(directory.path()).unwrap();
    let config = json!({
        "agentloop":{"implementation":{}, "configuration":{}, "environment":"brain"},
        "model":{"provider":"openai", "name":"model"}, "tools":[], "environments":[]
    });
    let store = LocalSessionStore::create(
        &sessions.join("ses_media"),
        SessionId::new("ses_media"),
        &config,
        Writer::spawn(),
        Arc::new(Feed::new(brain_telemetry::telemetry_channel().0)),
    )
    .unwrap();
    store
        .append_sync(
            &[
                AppendRecord::new("session_creation_started", config.clone()),
                AppendRecord::new("session_creation_ended", json!({"configuration":config})),
            ],
            Default::default(),
        )
        .unwrap();
    store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 0,
            append: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::File {
                    media_type: FileMediaType::Pdf,
                    url: "https://media.example.com/report.pdf".into(),
                }],
            }],
        }])
        .unwrap();
    let check = |arguments: &[&str]| {
        let before = files(directory.path());
        let output = Command::new(env!("CARGO_BIN_EXE_brain-check-media-upgrade"))
            .arg("--data-dir")
            .arg(directory.path())
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(
            files(directory.path()),
            before,
            "inspection must not repair or rewrite data"
        );
        output
    };
    assert!(!check(&[]).status.success());
    drop(lock);
    let output = check(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!check(&["--from-chat"]).status.success());
    store.append_sync(&[AppendRecord::new("turn_started", json!({
        "input":{"message":"old image", "media":[{"type":"image", "url":"data:image/png;base64,PRIVATE"}]}
    }))], Default::default()).unwrap();
    store
        .append_journal_sync(&[JournalEntry::TranscriptDelta {
            keep: 0,
            append: vec![],
        }])
        .unwrap();
    store
        .append_sync(
            &[AppendRecord::new("session_ended", json!({}))],
            Default::default(),
        )
        .unwrap();
    assert!(store.fold().unwrap().transcript.is_empty());
    let output = check(&[]);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("ses_media"));
    assert!(!error.contains("PRIVATE"));
}
