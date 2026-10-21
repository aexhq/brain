"""Model and tool declarations lowered to the generated admission contract."""
from dataclasses import dataclass
from typing import Any

from .runtime import RuntimeTool
from .types import CreateSession, ModelCredential, SessionConfig


def ApiKey(value: str) -> ModelCredential:
    return {"api_key": value}


def CredentialReference(value: str) -> ModelCredential:
    return {"reference": value}


@dataclass
class Model:
    name: str
    credential: ModelCredential | None = None


Tool = RuntimeTool


@dataclass
class ManagedTool:
    name: str
    operations: list[str] | None = None
    alias: str | None = None
    parameters: Any = None
    credential: Any = None


def _integration(name, options, parameters, secrets, operations):
    unknown = options.keys() - (parameters.keys() | secrets.keys() | {"operations"})
    if unknown:
        raise ValueError(f"Unknown {name} option: {sorted(unknown)[0]}")
    values = {wire: options[key] for key, wire in parameters.items() if key in options}
    credential = {wire: options[key] for key, wire in secrets.items() if key in options}
    return ManagedTool(name, operations=options.get("operations", operations),
                       parameters=values, credential=credential if credential else None)


def AwsS3(**options) -> ManagedTool:
    return _integration("aws_s3", options, {key: key for key in ["bucket", "region", "endpoint", "prefix"]},
                        {"access_key_id": "accessKeyId", "secret_access_key": "secretAccessKey", "session_token": "sessionToken"}, ["list", "read"])


def Modal(**options) -> ManagedTool:
    return _integration("modal", options, {"app": "app", "image": "image"}, {"token_id": "tokenId", "token_secret": "tokenSecret"},
                        ["create", "exec", "inspect", "terminate"])


def E2B(**options) -> ManagedTool:
    return _integration("e2b", options, {"template": "template"}, {"api_key": "apiKey"}, ["create", "exec", "inspect", "terminate"])


def Daytona(**options) -> ManagedTool:
    return _integration("daytona", options, {"api_url": "apiUrl", "target": "target"}, {"api_key": "apiKey"}, ["create", "exec", "inspect", "terminate"])


def lower(harness: str, model: Model, tools: list[ManagedTool | RuntimeTool],
          instructions: str | None, reasoning_effort: str | None) -> tuple[CreateSession, list[RuntimeTool]]:
    config: SessionConfig = {"model": model.name, "tools": []}
    wire: CreateSession = {"harness": harness, "config": config}
    runtime: list[RuntimeTool] = []
    for tool in tools:
        if isinstance(tool, RuntimeTool):
            runtime.append(tool)
        elif tool.operations is None:
            if tool.alias is not None or tool.parameters is not None or tool.credential is not None:
                raise ValueError("provider tool options require operations")
            config["tools"].append(tool.name)
        else:
            alias = tool.alias if tool.alias is not None else tool.name
            config.setdefault("bindings", []).append({"alias": alias, "provider": tool.name,
                "operations": tool.operations, "parameters": tool.parameters if tool.parameters is not None else {}})
            if tool.credential is not None:
                wire.setdefault("credentials", []).append({"binding": alias, "value": tool.credential})
    if model.credential is not None:
        wire["model_credential"] = model.credential
    if instructions is not None:
        config["instructions"] = instructions
    if reasoning_effort is not None:
        config["reasoning_effort"] = reasoning_effort
    return wire, runtime
