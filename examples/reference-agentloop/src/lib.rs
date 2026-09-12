use brain_protocol::{
    ContentBlock, EventPage, Message, ModelRequest, ModelResult, ToolInvocation, ToolResult,
};
use std::collections::BTreeMap;

wit_bindgen::generate!({ path: "../../crates/brain-env/wit/agentloop", world: "agentloop" });

struct Reference;

impl Guest for Reference {
    fn turn(input: TurnInput) -> Result<TurnOutput, TurnError> {
        let mut transcript: Vec<Message> = decode(&input.transcript_json)?;
        let mut kv: BTreeMap<String, serde_json::Value> = decode(&input.kv_json)?;
        let mut after = kv
            .get("observed_sequence")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        loop {
            let page: EventPage = decode(&brain::agentloop::host::events(after)?)?;
            if page.events.is_empty() {
                break;
            }
            for event in &page.events {
                if event.event_type.ends_with("_failed")
                    || matches!(
                        event.event_type.as_str(),
                        "environment_closed" | "environment_unreachable"
                    )
                {
                    transcript.push(Message::user_text(format!(
                        "Runtime observation (data): {} {}",
                        event.event_type, event.data
                    )));
                }
            }
            after = page.next_cursor;
        }
        kv.insert("observed_sequence".into(), after.into());
        brain::agentloop::host::kv_put("observed_sequence", &after.to_string())?;
        let tools: Vec<brain_protocol::ActivationTool> = decode(&input.tools_json)?;
        let input: brain_protocol::UserInput = decode(&input.input_json)?;
        let mut message = Message::user_text(input.message);
        message.content.extend(input.media.into_iter().map(ContentBlock::from));
        transcript.push(message);
        loop {
            brain::agentloop::host::set_transcript(&encode(&transcript)?)?;
            let request = ModelRequest {
                options: Default::default(),
messages: transcript.clone(),
                system: None,
                tools: None,
                response_format: None,
                max_output_tokens: None,
            };
            let result: ModelResult = decode(&brain::agentloop::host::model(&encode(&request)?)?)?;
            let calls = result
                .message
                .tool_uses()
                .map(|(id, name, input)| {
                    let placements = tools.iter().find(|tool| tool.definition.name == name).ok_or_else(|| TurnError { code: "unknown_tool".into(), message: name.into(), retryable: false })?;
                    let [environment] = placements.environments.as_slice() else { return Err(TurnError { code: "ambiguous_placement".into(), message: format!("{name} requires an explicit placement policy"), retryable: false }); };
                    Ok(ToolInvocation {
                    environment: environment.clone(),
                    call_id: id.into(),
                    name: name.into(),
                    input: input.clone(),
                })})
                .collect::<Result<Vec<_>, TurnError>>()?;
            transcript.push(result.message);
            brain::agentloop::host::set_transcript(&encode(&transcript)?)?;
            if calls.is_empty() {
                break;
            }
            let results: Vec<ToolResult> =
                decode(&brain::agentloop::host::dispatch(&encode(&calls)?)?)?;
            transcript.push(Message::tool_results(
                results
                    .into_iter()
                    .map(|result| ContentBlock::ToolResult {
                        media: Vec::new(),
tool_use_id: result.call_id,
                        content: result.output,
                        is_error: result.is_error,
                    })
                    .collect(),
            ));
        }
        Ok(TurnOutput {
            result_json: None,
        })
    }
}

fn decode<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, TurnError> {
    serde_json::from_str(json).map_err(error)
}

fn encode(value: &impl serde::Serialize) -> Result<String, TurnError> {
    serde_json::to_string(value).map_err(error)
}

fn error(error: serde_json::Error) -> TurnError {
    TurnError {
        code: "invalid_json".into(),
        message: error.to_string(),
        retryable: false,
    }
}

export!(Reference);
