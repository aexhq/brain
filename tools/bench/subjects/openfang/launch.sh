#!/usr/bin/env bash
# Starts OpenFang against the benchmark's scripted provider.
#
# OpenFang is configured by a TOML file rather than by flags, so the manifest cannot
# express the model endpoint on its own. This writes the config the run needs into a home
# the runner owns, then execs the daemon: no state from a previous run, and the model
# endpoint is the benchmark's rather than a real provider's.
set -euo pipefail

: "${BENCH_PORT:?BENCH_PORT is required}"
: "${BENCH_DATA_DIR:?BENCH_DATA_DIR is required}"
: "${OPENAI_BASE_URL:?OPENAI_BASE_URL is required}"

export OPENFANG_HOME="$BENCH_DATA_DIR"
export OPENFANG_LISTEN="127.0.0.1:${BENCH_PORT}"
# The API is bound to loopback on a machine the benchmark owns; a token would only be one
# more thing to keep out of the results.
export OPENFANG_ALLOW_NO_AUTH=1
export OPENAI_API_KEY="${OPENAI_API_KEY:-bench}"

mkdir -p "$OPENFANG_HOME"
cat > "$OPENFANG_HOME/config.toml" <<CONF
api_listen = "127.0.0.1:${BENCH_PORT}"

[default_model]
provider = "openai"
model = "${BENCH_MODEL:-gpt-4o-mini}"
api_key_env = "OPENAI_API_KEY"
base_url = "${OPENAI_BASE_URL}"

[memory]
decay_rate = 0.05
CONF

exec "${BENCH_OPENFANG_BIN:-/home/ubuntu/openfang/target/release/openfang}" start --yolo
