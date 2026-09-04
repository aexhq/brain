#!/usr/bin/env bash
# Starts `codex app-server`, behind the stdio bridge, against the benchmark's scripted
# provider.
#
# Codex reads its model provider from config.toml under CODEX_HOME, so the manifest cannot
# express the model endpoint on its own. This writes that file into a home directory the
# runner owns, then execs the bridge around the app-server.
#
# What differs from a stock install, and why:
#   * the model provider is the scripted one, which is the whole point of the fixture. It
#     is declared with `wire_api = "responses"`, the only wire Codex still speaks (0.153
#     refuses `"chat"`), which the fixture serves at /v1/responses; retries are off so a
#     fixture hiccup surfaces as a failed turn rather than a slow one;
#   * `approval_policy = "never"` and `sandbox_mode = "read-only"`: a benchmark cannot
#     answer an approval prompt, and a read-only sandbox is the documented safe default
#     for an agent that will not be asked to edit anything;
#   * the update check and analytics are off, keeping startup network calls out of a
#     boot number and off a box that is measuring loopback latency;
#   * the working directory is an empty git repository under the data directory, the
#     habitat a coding agent expects.
# Everything else is left at Codex's defaults, including its tools: a turn measured
# without them would be a different turn from the one a user gets.
set -euo pipefail

: "${BENCH_PORT:?BENCH_PORT is required}"
: "${BENCH_DATA_DIR:?BENCH_DATA_DIR is required}"
: "${BENCH_MODEL_BASE_URL:?BENCH_MODEL_BASE_URL is required}"

ROOT="${BENCH_SUBJECTS_ROOT:-$HOME/subjects}"
MODEL="${BENCH_MODEL:-scripted}"

export CODEX_HOME="$BENCH_DATA_DIR/codex-home"
WORKSPACE="$BENCH_DATA_DIR/workspace"
mkdir -p "$CODEX_HOME" "$WORKSPACE"
[ -d "$WORKSPACE/.git" ] || git -C "$WORKSPACE" init -q

cat > "$CODEX_HOME/config.toml" <<CONF
model = "${MODEL}"
model_provider = "bench"
model_context_window = 200000
approval_policy = "never"
sandbox_mode = "read-only"
check_for_update_on_startup = false

[analytics]
enabled = false

[model_providers.bench]
name = "Scripted provider"
base_url = "${BENCH_MODEL_BASE_URL}"
env_key = "BENCH_API_KEY"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
CONF
export BENCH_API_KEY=bench

# The npm package's `bin/codex.js` is a Node launcher that spawns the platform binary and
# stays resident beside it. The binary is exec'd directly so the memory sampler sees Codex
# and not a Node process it never needed.
case "$(uname -m)" in
  aarch64) BIN_DEFAULT="$ROOT/node_modules/@openai/codex-linux-arm64/vendor/aarch64-unknown-linux-musl/bin/codex" ;;
  *) BIN_DEFAULT="$ROOT/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex" ;;
esac

exec "${BENCH_BRIDGE_BIN:-tools/bench/runner/target/release/brain-bench-bridge}" \
  --port "$BENCH_PORT" \
  --cwd "$WORKSPACE" \
  --ready-send '{"id":"ready","method":"model/list","params":{}}' \
  -- "${BENCH_CODEX_BIN:-$BIN_DEFAULT}" app-server
