wit_bindgen::generate!({
    path: "../../../crates/brain-env/wit/agentloop",
    world: "agentloop",
});

struct Diagnostic;

impl Guest for Diagnostic {
    fn turn(input: TurnInput) -> Result<TurnOutput, TurnError> {
        brain::agentloop::host::events(0)?;
        let mut kv = serde_json::json!({
            "memory": brain::agentloop::host::kv_read("memory")?
                .map(|value| serde_json::from_str::<serde_json::Value>(&value)).transpose().map_err(error)?
        });
        let turns = kv
            .get("memory")
            .and_then(|value| value.get("turns"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            + 1;
        kv["memory"] = serde_json::json!({"turns": turns});
        brain::agentloop::host::kv_put("memory", &kv["memory"].to_string())?;
        brain::agentloop::host::emit("note", &serde_json::json!({"turns": turns}).to_string())?;
        let message = serde_json::from_str::<serde_json::Value>(&input.input_json)
            .map_err(error)?
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if message == "kv" {
        assert!(brain::agentloop::host::kv_read("deleted")?.is_none());
        brain::agentloop::host::kv_put("deleted", "null")?;
        assert_eq!(brain::agentloop::host::kv_read("deleted")?.as_deref(), Some("null"));
        brain::agentloop::host::kv_delete("deleted")?;
        assert!(brain::agentloop::host::kv_read("deleted")?.is_none());
        brain::agentloop::host::kv_delete("deleted")?;
        }
        Ok(TurnOutput {
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
