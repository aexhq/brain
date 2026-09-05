wit_bindgen::generate!({
    path: "../../../contracts/tool/v1",
    world: "tool",
});

struct Diagnostic;

impl Guest for Diagnostic {
    fn run(input: Invocation) -> Result<String, ToolError> {
        brain::tool::host::emit("tool_progress", &serde_json::json!({"call_id": input.call_id}).to_string())?;
        let value: serde_json::Value = serde_json::from_str(&input.input_json).map_err(|error| ToolError {
            code: "invalid_input".into(),
            message: error.to_string(),
        })?;
        if value.get("workspace").is_some() {
            let marker = "/workspace/marker";
            if let Some(text) = value.get("write").and_then(serde_json::Value::as_str) {
                std::fs::write(marker, text).map_err(io_error)?;
            }
            return Ok(serde_json::json!({"marker": std::fs::read_to_string(marker).ok()}).to_string());
        }
        Ok(serde_json::json!({"echo": value}).to_string())
    }
}

fn io_error(error: std::io::Error) -> ToolError {
    ToolError {
        code: "io_error".into(),
        message: error.to_string(),
    }
}

export!(Diagnostic);
