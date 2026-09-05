wit_bindgen::generate!({
    path: "../../../contracts/agentloop/v1",
    world: "agentloop",
});

struct Diagnostic;

impl Guest for Diagnostic {
    fn turn(input: TurnInput) -> Result<TurnOutput, TurnError> {
        brain::agentloop::host::events(0)?;
        let mut slots: serde_json::Value = serde_json::from_str(&input.slots_json).map_err(error)?;
        let turns = slots
            .get("memory")
            .and_then(|value| value.get("turns"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            + 1;
        slots["memory"] = serde_json::json!({"turns": turns});
        brain::agentloop::host::emit("note", &serde_json::json!({"turns": turns}).to_string())?;
        let message = serde_json::from_str::<serde_json::Value>(&input.input_json)
            .map_err(error)?
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(TurnOutput {
            transcript_json: input.transcript_json,
            slots_json: slots.to_string(),
            result_json: Some(serde_json::json!({"turns": turns, "message": message}).to_string()),
        })
    }
}

fn error(error: serde_json::Error) -> TurnError {
    TurnError {
        code: "invalid_input".into(),
        message: error.to_string(),
        retryable: false,
    }
}

export!(Diagnostic);
