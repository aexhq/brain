#!/usr/bin/env python3
"""Generates the provider catalog from the vendored models.dev snapshot.

Emits, from the one pinned source:
  - crates/brain/src/model/generated/catalog.rs  (the static Rust table)
  - packages/brain-sdk/src/generated/providers.ts (the known-provider union)

Admission: a provider is included iff its `npm` package is one of the SDKs
whose wire shape Brain already speaks, and its base URL satisfies the same
rules the transport enforces at startup. Rows failing the URL rules (upstream
placeholders, non-loopback plain HTTP) are excluded deterministically and
listed in the generated header, so a refresh stays reviewable instead of a
CI fight over third-party data.
"""

from __future__ import annotations

import hashlib
import ipaddress
import json
import pathlib
import urllib.parse

ROOT = pathlib.Path(__file__).resolve().parents[1]
SNAPSHOT = ROOT / "catalog/models-dev/api.json"
DIGEST = ROOT / "catalog/models-dev/api.digest"
RUST_OUTPUT = ROOT / "crates/brain/src/model/generated/catalog.rs"
TYPESCRIPT_OUTPUT = ROOT / "packages/brain-sdk/src/generated/providers.ts"

# npm package -> (dialect, max_tokens_field). Only SDKs whose wire shape Brain
# already speaks; dedicated per-provider SDKs (xai, groq, google, bedrock...)
# need their own dialect work before they can be admitted.
ADMITTED_NPM = {
    "@ai-sdk/openai": ("OpenAiChat", "MaxCompletionTokens"),
    "@ai-sdk/openai-compatible": ("OpenAiChat", "MaxTokens"),
    "@ai-sdk/anthropic": ("AnthropicMessages", "MaxCompletionTokens"),
}

# The two first-party providers models.dev lists without an endpoint.
BASE_URL_DEFAULTS = {
    "openai": "https://api.openai.com/v1",
    "anthropic": "https://api.anthropic.com/v1",
}

# Curated separately (models.dev files the gateway under @ai-sdk/gateway); part
# of the SDK's known-provider union all the same.
CURATED_TS_PROVIDERS = ["vercel-ai-gateway"]


def valid_identifier(value: str) -> bool:
    if not value or len(value) > 128 or not value[0].isalnum() or not value.isascii():
        return False
    return all(byte.isalnum() or byte in "._:-" for byte in value)


def valid_base_url(url: str) -> bool:
    """Mirror of brain::model::validate_base_url: HTTPS, or literal-loopback-IP
    HTTP; no credentials, query, or fragment."""
    try:
        parsed = urllib.parse.urlsplit(url)
    except ValueError:
        return False
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        return False
    if parsed.scheme == "https":
        return bool(parsed.hostname)
    if parsed.scheme != "http" or not parsed.hostname:
        return False
    try:
        return ipaddress.ip_address(parsed.hostname).is_loopback
    except ValueError:
        return False


def rust_str(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def rust_f64(value: float) -> str:
    text = repr(float(value))
    return text if ("." in text or "e" in text or "inf" in text or "nan" in text) else text + ".0"


def rust_opt_u64(value: object) -> str:
    return f"Some({int(value)})" if isinstance(value, (int, float)) and value >= 0 else "None"


def rust_opt_bool(value: object) -> str:
    return {True: "Some(true)", False: "Some(false)"}.get(value, "None")


def main() -> None:
    raw = SNAPSHOT.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    pinned = DIGEST.read_text(encoding="ascii").strip()
    if digest != pinned:
        raise SystemExit(
            f"snapshot digest mismatch: api.json is {digest} but api.digest pins {pinned};"
            " refresh with tools/fetch-models-dev.mjs instead of editing the snapshot"
        )
    snapshot = json.loads(raw)

    providers = []
    excluded = []
    for key in sorted(snapshot):
        entry = snapshot[key]
        npm = entry.get("npm")
        if npm not in ADMITTED_NPM:
            continue
        dialect, max_tokens_field = ADMITTED_NPM[npm]
        base_url = (entry.get("api") or BASE_URL_DEFAULTS.get(key, "")).rstrip("/")
        if not valid_identifier(key):
            excluded.append(f"{key}: provider id is not a valid identifier")
            continue
        if not valid_base_url(base_url):
            excluded.append(f"{key}: base URL {base_url!r} fails the transport rules")
            continue
        models = []
        for model_key in sorted(entry.get("models", {})):
            model = entry["models"][model_key]
            model_id = model.get("id") or model_key
            limit = model.get("limit") or {}
            cost = model.get("cost")
            if isinstance(cost, dict) and "input" in cost and "output" in cost:
                cost_rust = (
                    f"Some(({rust_f64(cost['input'])}, {rust_f64(cost['output'])}, "
                    + (f"Some({rust_f64(cost['cache_read'])})" if "cache_read" in cost else "None")
                    + ", "
                    + (f"Some({rust_f64(cost['cache_write'])})" if "cache_write" in cost else "None")
                    + "))"
                )
            else:
                cost_rust = "None"
            models.append(
                "        CatalogModel { id: "
                + rust_str(model_id)
                + f", context_window_tokens: {rust_opt_u64(limit.get('context'))}"
                + f", max_output_tokens: {rust_opt_u64(limit.get('output'))}"
                + f", tool_call: {rust_opt_bool(model.get('tool_call'))}"
                + f", structured_output: {rust_opt_bool(model.get('structured_output'))}"
                + f", reasoning: {rust_opt_bool(model.get('reasoning'))}"
                + f", cost: {cost_rust}"
                + " },"
            )
        supports_response_format = "true" if dialect == "OpenAiChat" else "false"
        block = [
            "    CatalogProvider {",
            f"        name: {rust_str(key)},",
            f"        dialect: Dialect::{dialect},",
            f"        base_url: {rust_str(base_url)},",
            f"        supports_response_format: {supports_response_format},",
            f"        max_tokens_field: MaxTokensField::{max_tokens_field},",
            "        models: &[",
            *models,
            "        ],",
            "    },",
        ]
        providers.append((key, block))

    header = [
        "// Generated by tools/generate-providers.py from catalog/models-dev/api.json.",
        "// Do not edit; refresh with tools/fetch-models-dev.mjs and `npm run gen`.",
        "#![cfg_attr(rustfmt, rustfmt::skip)]",
        "//",
        f"// {len(providers)} providers admitted. Excluded rows:",
        *[f"//   {reason}" for reason in excluded],
        "",
        "use crate::model::{Dialect, MaxTokensField};",
        "",
        "use super::{CatalogModel, CatalogProvider};",
        "",
        f"pub const SNAPSHOT_DIGEST: &str = \"{digest}\";",
        "",
        "pub const CATALOG: &[CatalogProvider] = &[",
    ]
    body = [line for _, block in providers for line in block]
    RUST_OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    RUST_OUTPUT.write_text("\n".join(header + body + ["];", ""]), encoding="utf-8", newline="\n")

    names = sorted({key for key, _ in providers} | set(CURATED_TS_PROVIDERS))
    typescript = [
        "// Generated by tools/generate-providers.py from catalog/models-dev/api.json.",
        "// Do not edit; refresh with tools/fetch-models-dev.mjs and `npm run gen`.",
        "",
        "export type KnownProviderId =",
        *[f'  | "{name}"' for name in names],
        ";",
        "",
        "export const knownProviders: readonly KnownProviderId[] = [",
        *[f'  "{name}",' for name in names],
        "];",
        "",
    ]
    TYPESCRIPT_OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    TYPESCRIPT_OUTPUT.write_text("\n".join(typescript), encoding="utf-8", newline="\n")
    print(
        f"catalog: {len(providers)} providers, "
        f"{sum(1 for _, block in providers for line in block if 'CatalogModel' in line)} models, "
        f"{len(excluded)} excluded"
    )


if __name__ == "__main__":
    main()
