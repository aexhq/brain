use brain_protocol::{
    ContentBlock, EventPage, Message, ModelRequest, ModelResult, Outcome, ToolInvocation,
    ToolResult, ToolReturn,
};
use std::collections::{BTreeMap, BTreeSet};

wit_bindgen::generate!({ path: "../../crates/brain-env/wit/agentloop", world: "agentloop" });

struct Reference;

impl Guest for Reference {
    fn turn(input: TurnInput) -> Result<TurnOutput, TurnError> {
        let mut transcript: Vec<Message> = decode(&input.transcript_json)?;
        let kv: BTreeMap<String, serde_json::Value> = decode(&input.kv_json)?;
        let mut after = kv
            .get("brain.last_activation")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let actionable = observe(&mut transcript, &mut after, &BTreeSet::new())?;
        let tools: Vec<brain_protocol::ActivationTool> = decode(&input.tools_json)?;
        let user: Option<brain_protocol::UserInput> = decode(&input.input_json)?;
        if let Some(user) = user {
            let mut message = Message::user_text(user.message);
            message
                .content
                .extend(user.media.into_iter().map(ContentBlock::from));
            transcript.push(message);
        } else if !actionable {
            return Ok(TurnOutput { result_json: None });
        }
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
                    let placements = tools
                        .iter()
                        .find(|tool| tool.definition.name == name)
                        .ok_or_else(|| TurnError {
                            code: "unknown_tool".into(),
                            message: name.into(),
                            retryable: false,
                        })?;
                    let [environment] = placements.environments.as_slice() else {
                        return Err(TurnError {
                            code: "ambiguous_placement".into(),
                            message: format!("{name} requires an explicit placement policy"),
                            retryable: false,
                        });
                    };
                    Ok(ToolInvocation {
                        environment: environment.clone(),
                        call_id: id.into(),
                        name: name.into(),
                        input: input.clone(),
                    })
                })
                .collect::<Result<Vec<_>, TurnError>>()?;
            transcript.push(result.message);
            brain::agentloop::host::set_transcript(&encode(&transcript)?)?;
            if calls.is_empty() {
                break;
            }
            let returns: Vec<ToolReturn> =
                decode(&brain::agentloop::host::dispatch(&encode(&calls)?)?)?;
            let consumed = returns
                .iter()
                .flat_map(|returned| returned.events.iter().map(|event| event.sequence))
                .collect();
            transcript.push(Message::tool_results(
                returns
                    .into_iter()
                    .map(present)
                    .collect::<Result<Vec<_>, _>>()?,
            ));
            observe(&mut transcript, &mut after, &consumed)?;
        }
        Ok(TurnOutput { result_json: None })
    }
}

fn observe(
    transcript: &mut Vec<Message>,
    after: &mut u64,
    consumed: &BTreeSet<u64>,
) -> Result<bool, TurnError> {
    let mut actionable = false;
    loop {
        let page: EventPage = decode(&brain::agentloop::host::events(*after)?)?;
        if page.events.is_empty() {
            break;
        }
        for event in &page.events {
            if !consumed.contains(&event.sequence)
                && (event.event_type.ends_with("_failed")
                    || matches!(
                        event.event_type.as_str(),
                        "tool_result_emitted"
                            | "tool_call_ended"
                            | "environment_closed"
                            | "environment_unreachable"
                    ))
            {
                let mut data = event.data.clone();
                if event.event_type == "tool_result_emitted"
                    && let Some(content) = data["result"]["content"].as_str().map(str::to_owned)
                {
                    data["result"]["output"] = content.into();
                }
                transcript.push(Message::user_text(format!(
                    "Runtime observation (data): {} {}",
                    event.event_type, data
                )));
                actionable = true;
            }
        }
        *after = page.next_cursor;
    }
    brain::agentloop::host::set_transcript(&encode(transcript)?)?;
    brain::agentloop::host::acknowledge(*after)?;
    Ok(actionable)
}

fn present(returned: ToolReturn) -> Result<ContentBlock, TurnError> {
    let mut results = Vec::new();
    for event in returned.events {
        if event.event_type == "tool_result_emitted" {
            results.push(
                serde_json::from_value::<ToolResult>(event.data["result"].clone())
                    .map_err(error)?,
            );
        } else if event.event_type == "tool_call_ended" {
            let outcome: Outcome =
                serde_json::from_value(event.data["outcome"].clone()).map_err(error)?;
            if !matches!(outcome, Outcome::Ok { .. }) {
                let terminal = ToolResult::from_outcome(returned.call_id.clone(), outcome);
                if !results.last().is_some_and(|result| {
                    result.is_error && result.content.is_some() && result.output == terminal.output
                }) {
                    results.push(terminal);
                }
            }
        }
    }
    let is_error = results.iter().any(|result| result.is_error);
    for result in &mut results {
        if let Some(content) = result.content.take() {
            result.output = serde_json::Value::String(content);
        }
    }
    let content = if returned.finished && results.len() == 1 {
        results.remove(0).output
    } else {
        serde_json::json!({"status": if returned.finished { "finished" } else { "running" }, "results": results})
    };
    Ok(ContentBlock::ToolResult {
        media: Vec::new(),
        tool_use_id: returned.call_id,
        content,
        is_error,
    })
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
