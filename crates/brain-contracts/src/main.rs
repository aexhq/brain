//! Writes `contracts/` from the Rust types: `cargo run -p brain-contracts`.
//!
//! Every file this writes is output. CI runs it and fails on a diff, so a contract is
//! changed by changing the Rust type it is rendered from and running this again.

use std::{fs, path::Path};

use serde_json::Value;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let contracts = root.join("contracts");
    write_json(
        &contracts.join("session/v1/schemas.json"),
        &brain_protocol::contracts::session(),
    );
    write_json(
        &contracts.join("session/v1/codes.json"),
        &brain_protocol::contracts::codes(),
    );
    write_yaml(
        &contracts.join("session/v1/openapi.yaml"),
        &brain_http::openapi(),
    );
    write_json(
        &contracts.join("environment/v1/schemas.json"),
        &brain_protocol::contracts::environment(),
    );
    write_json(
        &contracts.join("tool/v1/schemas.json"),
        &brain_protocol::contracts::tool(),
    );
    write_json(
        &contracts.join("agentloop/v1/contract.json"),
        &brain_protocol::contracts::agentloop(),
    );
}

fn write_json(path: &Path, value: &Value) {
    let mut text = serde_json::to_string_pretty(value).expect("a contract serializes");
    text.push('\n');
    write(path, &text);
}

fn write_yaml(path: &Path, value: &Value) {
    let text = serde_norway::to_string(value).expect("a contract serializes");
    write(path, &text);
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}
