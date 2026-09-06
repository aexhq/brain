//! Writes the configuration reference table in `docs/reference/configuration.mdx` from
//! the server's clap definition: `cargo run -p brain-server --bin contract`.
//!
//! The table between the markers is output. CI runs this and fails on a diff, so a
//! flag, variable, default, or description is changed by changing the field it is
//! rendered from and running this again.

use std::{fs, path::Path};

use brain_server::ServerConfig;
use clap::CommandFactory as _;

const START: &str = "{/* configuration:start */}";
const END: &str = "{/* configuration:end */}";

fn main() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/configuration.mdx");
    let document = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let start = document
        .find(START)
        .unwrap_or_else(|| panic!("{} has no {START} marker", path.display()));
    let end = document
        .find(END)
        .unwrap_or_else(|| panic!("{} has no {END} marker", path.display()));
    let rendered = format!(
        "{}{START}\n{}{END}{}",
        &document[..start],
        table(),
        &document[end + END.len()..]
    );
    fs::write(&path, rendered).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}

fn table() -> String {
    let mut out =
        String::from("| Variable | Flag | Default | What it does |\n| --- | --- | --- | --- |\n");
    for arg in ServerConfig::command().get_arguments() {
        let Some(long) = arg.get_long() else {
            continue;
        };
        if matches!(long, "help" | "version") {
            continue;
        }
        let variable = arg
            .get_env()
            .map(|env| format!("`{}`", env.to_string_lossy()))
            .unwrap_or_default();
        let default = match arg.get_default_values() {
            [] => "none".to_owned(),
            values => values
                .iter()
                .map(|value| format!("`{}`", value.to_string_lossy()))
                .collect::<Vec<_>>()
                .join(", "),
        };
        let help = arg
            .get_help()
            .map(|help| help.to_string().replace('\n', " "))
            .unwrap_or_default();
        out.push_str(&format!(
            "| {variable} | `--{long}` | {default} | {help} |\n"
        ));
    }
    out
}
