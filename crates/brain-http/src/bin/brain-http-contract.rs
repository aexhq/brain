//! Writes this crate's `generated/contract/` from the route annotations:
//! `cargo run -p brain-http --bin brain-http-contract`.
//!
//! The file this writes is output. CI runs it and fails on a diff, so the OpenAPI document
//! is changed by changing a `#[utoipa::path]` annotation or a `brain-protocol` type.

use std::{fs, path::Path};

fn main() {
    for (version, document) in [
        ("v1", brain_http::openapi()),
        ("v2", brain_http::openapi_v2()),
    ] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("generated/contract/session/{version}"));
        fs::create_dir_all(&root).unwrap();
        for (extension, text) in [
            (
                "yaml",
                serde_norway::to_string(&document).expect("OpenAPI serializes"),
            ),
            (
                "json",
                format!("{}\n", serde_json::to_string_pretty(&document).unwrap()),
            ),
        ] {
            let path = root.join(format!("openapi.{extension}"));
            fs::write(&path, text)
                .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
            println!("wrote {}", path.display());
        }
    }
}
