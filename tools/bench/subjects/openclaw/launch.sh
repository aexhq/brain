#!/usr/bin/env bash
# Starts OpenClaw's Gateway against the benchmark's scripted provider.
#
# OpenClaw reads a JSON5 config file whose path comes from OPENCLAW_CONFIG_PATH, so the
# manifest cannot express the model endpoint on its own. This writes that file into a
# state directory the runner owns, then execs the gateway.
#
# Three settings differ from a stock install, and each is recorded here:
#   * `gateway.auth.mode = "none"` — documented as the private-ingress mode. The gateway
#     is on loopback on a machine the benchmark owns, and a shared secret would only be
#     one more thing to keep out of the results.
#   * `gateway.http.endpoints.chatCompletions.enabled = true` — off by default. It is the
#     surface the turn probes drive, and the docs say a request through it "runs as a
#     normal Gateway agent run (same codepath as `openclaw agent`)", so enabling it adds
#     a route rather than changing what a turn costs.
#   * the model provider is the scripted one, which is the whole point of the fixture.
# Everything else is left at OpenClaw's own defaults.
set -euo pipefail

: "${BENCH_PORT:?BENCH_PORT is required}"
: "${BENCH_DATA_DIR:?BENCH_DATA_DIR is required}"
: "${BENCH_MODEL_BASE_URL:?BENCH_MODEL_BASE_URL is required}"

export OPENCLAW_STATE_DIR="$BENCH_DATA_DIR/state"
export OPENCLAW_CONFIG_PATH="$OPENCLAW_STATE_DIR/openclaw.json"
export OPENCLAW_WORKSPACE_DIR="$BENCH_DATA_DIR/workspace"
mkdir -p "$OPENCLAW_STATE_DIR" "$OPENCLAW_WORKSPACE_DIR"

MODEL="${BENCH_MODEL:-gpt-4o-mini}"

# `allowPrivateNetwork` is set explicitly rather than relying on the exact-origin trust
# OpenClaw grants a configured loopback baseUrl, so a change in that policy shows up as a
# config difference here instead of as an unexplained model-call failure.
cat > "$OPENCLAW_CONFIG_PATH" <<CONF
{
  "gateway": {
    "mode": "local",
    "port": ${BENCH_PORT},
    "auth": { "mode": "none" },
    "http": { "endpoints": { "chatCompletions": { "enabled": true } } }
  },
  "agents": {
    "defaults": {
      "model": { "primary": "bench/${MODEL}" }
    }
  },
  "models": {
    "mode": "merge",
    "providers": {
      "bench": {
        "baseUrl": "${BENCH_MODEL_BASE_URL}",
        "apiKey": "bench",
        "api": "openai-completions",
        "request": { "allowPrivateNetwork": true },
        "models": [{ "id": "${MODEL}", "name": "Scripted provider" }]
      }
    }
  }
}
CONF

exec "${BENCH_OPENCLAW_BIN:-/home/ubuntu/subjects/openclaw/node_modules/.bin/openclaw}" \
  gateway --port "$BENCH_PORT"
