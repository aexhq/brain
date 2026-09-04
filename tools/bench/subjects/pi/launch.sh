#!/usr/bin/env bash
# Starts pi in RPC mode, behind the stdio bridge, against the benchmark's scripted provider.
#
# pi reads custom providers from models.json in its agent directory, so the manifest cannot
# express the model endpoint on its own. This writes that file into an agent directory the
# runner owns, then execs the bridge around `pi --mode rpc`.
#
# What differs from a stock install, and why:
#   * the model provider is the scripted one, which is the whole point of the fixture. It
#     is an `openai-completions` provider, pi's most compatible API type;
#   * PI_OFFLINE, PI_SKIP_VERSION_CHECK and PI_TELEMETRY=0 keep pi's startup network
#     calls — update check, package refresh, install telemetry — out of a boot number
#     and off a box that is measuring loopback latency;
#   * the working directory is an empty git repository under the data directory, the
#     habitat a coding agent expects, so its startup project scan finds nothing to read.
# Everything else is left at pi's defaults, including its built-in tools: a turn measured
# without them would be a different turn from the one a user gets.
set -euo pipefail

: "${BENCH_PORT:?BENCH_PORT is required}"
: "${BENCH_DATA_DIR:?BENCH_DATA_DIR is required}"
: "${BENCH_MODEL_BASE_URL:?BENCH_MODEL_BASE_URL is required}"

ROOT="${BENCH_SUBJECTS_ROOT:-$HOME/subjects}"
MODEL="${BENCH_MODEL:-scripted}"

export PI_CODING_AGENT_DIR="$BENCH_DATA_DIR/agent"
WORKSPACE="$BENCH_DATA_DIR/workspace"
mkdir -p "$PI_CODING_AGENT_DIR" "$WORKSPACE"
[ -d "$WORKSPACE/.git" ] || git -C "$WORKSPACE" init -q

cat > "$PI_CODING_AGENT_DIR/models.json" <<CONF
{
  "providers": {
    "bench": {
      "baseUrl": "${BENCH_MODEL_BASE_URL}",
      "api": "openai-completions",
      "apiKey": "bench",
      "models": [{ "id": "${MODEL}", "name": "Scripted provider", "contextWindow": 200000, "maxTokens": 8192 }]
    }
  }
}
CONF

export PI_OFFLINE=1 PI_SKIP_VERSION_CHECK=1 PI_TELEMETRY=0

exec "${BENCH_BRIDGE_BIN:-tools/bench/runner/target/release/brain-bench-bridge}" \
  --port "$BENCH_PORT" \
  --cwd "$WORKSPACE" \
  --ready-send '{"type":"get_state"}' \
  -- "${BENCH_PI_BIN:-$ROOT/node_modules/.bin/pi}" --mode rpc --provider bench --model "$MODEL"
