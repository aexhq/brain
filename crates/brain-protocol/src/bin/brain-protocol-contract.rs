//! Writes this crate's `generated/contract/` from its types:
//! `cargo run -p brain-protocol --bin brain-protocol-contract`.
//!
//! Every file this writes is output. CI runs it and fails on a diff, so a contract is
//! changed by changing the Rust type it is rendered from and running this again.

use std::{fs, path::Path};

use brain_protocol::contract;

fn main() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("generated/contract");
    for (path, document) in [
        ("session/v1/schemas.json", contract::session()),
        ("session/v1/codes.json", contract::codes()),
        ("environment/v1/schemas.json", contract::environment()),
        ("tool/v1/schemas.json", contract::tool()),
        ("agentloop/v1/contract.json", contract::agentloop()),
    ] {
        let path = out.join(path);
        let mut text = serde_json::to_string_pretty(&document).expect("a contract serializes");
        text.push('\n');
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
        println!("wrote {}", path.display());
    }
}
