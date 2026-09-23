import json

print(json.dumps({
    "definition": {
        "name": "python_echo",
        "description": "Echo input with the installed Python dependency version.",
        "inputSchema": {"type": "string"},
        "outputSchema": {
            "type": "object",
            "properties": {"echo": {"type": "string"}, "dependency": {"type": "string"}},
            "required": ["echo", "dependency"],
        },
    },
}))
