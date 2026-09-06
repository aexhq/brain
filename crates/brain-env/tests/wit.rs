//! The hand-written WIT under `wit/` is the surface this crate hosts.

use std::{fs, path::Path};

#[test]
fn agentloop_world_exports_turn_and_imports_only_the_host() {
    let wit = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("wit/agentloop/agentloop.wit"),
    )
    .unwrap();
    let world = wit.split("world agentloop").nth(1).unwrap();
    assert_eq!(world.matches("import host;").count(), 1);
    assert_eq!(world.matches("import ").count(), 1);
    assert_eq!(world.matches("export turn:").count(), 1);
}
