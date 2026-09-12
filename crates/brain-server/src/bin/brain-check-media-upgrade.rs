use std::{fs, path::PathBuf};

use brain::{
    Error, LocalSessionStore,
    model::{Dialect, ProviderRegistry, validate_context},
};
use brain_protocol::{Message, MessageRequest, SessionConfig};
use clap::Parser;
use serde_json::Value;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    data_dir: PathBuf,
    /// The previous runtime used Chat for the openai and vercel-ai-gateway bindings.
    #[arg(long)]
    from_chat: bool,
    #[arg(long)]
    providers_file: Option<PathBuf>,
}

fn invalid() -> Error {
    Error::InvalidState("retained session requires an explicit media/model migration".into())
}

fn inspect_record(
    registry: &ProviderRegistry,
    from_chat: bool,
    dialect: &mut Option<Dialect>,
    kind: &str,
    value: Value,
) -> Result<(), Error> {
    match kind {
        "session_creation_started" | "session_creation_ended" => {
            let configuration = if kind == "session_creation_ended" {
                value["configuration"].clone()
            } else {
                value
            };
            let config: SessionConfig =
                serde_json::from_value(configuration).map_err(|_| invalid())?;
            if from_chat
                && matches!(
                    config.model.provider.as_str(),
                    "openai" | "vercel-ai-gateway"
                )
            {
                return Err(invalid());
            }
            *dialect = Some(
                registry
                    .get(&config.model.provider)
                    .ok_or_else(invalid)?
                    .dialect,
            );
        }
        "turn_started" => {
            let request: MessageRequest = serde_json::from_value(value).map_err(|_| invalid())?;
            for media in request.input.media {
                media.validate().map_err(|_| invalid())?;
            }
        }
        "transcript_delta" | "model_call_started" => {
            let delta = if kind == "model_call_started" {
                &value["context"]["delta"]
            } else {
                &value
            };
            let messages: Vec<Message> =
                serde_json::from_value(delta["append"].clone()).map_err(|_| invalid())?;
            validate_context(dialect.ok_or_else(invalid)?, messages).map_err(|_| invalid())?;
        }
        _ => {}
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let lock = fs::File::open(args.data_dir.join(".lock"))?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("stop the Brain writer before checking retained sessions"))?;
    anyhow::ensure!(
        fs::read_to_string(args.data_dir.join("format"))?.trim()
            == brain_server::data_layout::FORMAT,
        "unknown Brain data format"
    );
    let providers = args
        .providers_file
        .as_deref()
        .map(brain_server::load_providers_file)
        .transpose()?
        .unwrap_or_default();
    let registry = ProviderRegistry::compose(providers, &[])?;
    let mut count = 0;
    for entry in fs::read_dir(args.data_dir.join("sessions"))? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let mut dialect = None;
        let result = LocalSessionStore::inspect(&path, |kind, value| {
            inspect_record(&registry, args.from_chat, &mut dialect, kind, value)
        });
        anyhow::ensure!(
            result.is_ok() && dialect.is_some(),
            "incompatible or unreadable retained session: {}",
            path.file_name().unwrap().to_string_lossy()
        );
        count += 1;
    }
    println!("checked {count} retained sessions; media/model contract is compatible");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn changed_bindings_and_removed_providers_require_explicit_migration() {
        let registry = ProviderRegistry::compose(vec![], &[]).unwrap();
        for (provider, from_chat, compatible) in [
            ("openai", true, false),
            ("vercel-ai-gateway", true, false),
            ("openai", false, true),
            ("vercel-ai-gateway", false, true),
            ("anthropic", true, true),
            ("openai-responses", false, false),
            ("groq", false, false),
        ] {
            let config = json!({
                "agentloop": {"implementation": {}, "configuration": {}, "environment": "brain"},
                "model": {"provider": provider, "name": "model"}, "tools": [], "environments": []
            });
            for (kind, value) in [
                ("session_creation_started", config.clone()),
                ("session_creation_ended", json!({"configuration": config})),
            ] {
                let mut dialect = None;
                assert_eq!(
                    inspect_record(&registry, from_chat, &mut dialect, kind, value).is_ok(),
                    compatible,
                    "{provider}, from_chat={from_chat}"
                );
                assert_eq!(dialect.is_some(), compatible);
            }
        }
    }

    #[test]
    fn inline_and_chat_state_are_rejected_without_inspecting_arbitrary_tool_json() {
        let registry = ProviderRegistry::compose(vec![], &[]).unwrap();
        let mut dialect = Some(Dialect::OpenAiResponses);
        for block in [
            json!({"type":"image","url":"data:image/png;base64,AAAA"}),
            json!({"type":"native","format":"openai.chat.v1","data":{}}),
        ] {
            assert!(
                inspect_record(
                    &registry,
                    false,
                    &mut dialect,
                    "transcript_delta",
                    json!({"append":[{"role":"user","content":[block]}]})
                )
                .is_err()
            );
        }
        assert!(inspect_record(&registry, false, &mut dialect, "transcript_delta", json!({"append":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"call","content":{"text":"data:image/png;base64,AAAA"},"is_error":false}]}]})).is_ok());
    }
}
