//! Writes this crate's `generated/contract/` from the route annotations:
//! `cargo run -p brain-http --bin contract`.
//!
//! The file this writes is output. CI runs it and fails on a diff, so the OpenAPI document
//! is changed by changing a `#[utoipa::path]` annotation or a `brain-protocol` type.

use std::{fs, path::Path};

fn main() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("generated/contract/session/v1/openapi.yaml");
    let text = serde_norway::to_string(&brain_http::openapi()).expect("the document serializes");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}
