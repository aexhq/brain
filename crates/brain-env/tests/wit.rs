//! The hand-written WIT under `wit/` is the surface this crate hosts.

use std::{fs, path::Path};

#[test]
fn tool_world_exports_run_and_imports_only_the_host() {
    let wit = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("wit/tool/tool.wit"))
        .unwrap();
    let world = wit.split("world tool").nth(1).unwrap();
    assert_eq!(world.matches("import host;").count(), 1);
    assert_eq!(world.matches("import ").count(), 1);
    assert_eq!(world.matches("export run:").count(), 1);
}
