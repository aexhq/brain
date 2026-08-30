#!/usr/bin/env bash
# Starts ZeroClaw's daemon against the benchmark's scripted provider.
#
# ZeroClaw is configured by a TOML file rather than by flags, so the manifest cannot
# express the model endpoint on its own. This writes the config the run needs into a
# config directory the runner owns, then execs the daemon: no state from a previous run,
# and the model endpoint is the benchmark's rather than a real provider's.
#
# Three settings differ from a stock install, and each is recorded here because a rival
# tuned differently from Brain produces a number worth nothing:
#   * `require_pairing = false` — the gateway is on loopback on a machine the benchmark
#     owns, and a bearer token would only be one more thing to keep out of the results.
#     It is the same accommodation the OpenFang subject makes.
#   * the model provider is the scripted one, which is the whole point of the fixture.
#   * `[agents.bench]` exists at all. ZeroClaw has no default agent — `/ws/chat` refuses
#     the upgrade without `?agent=<alias>` — so one has to be named, and the driver's
#     AGENT_ALIAS has to match this block.
# Everything else is left at ZeroClaw's own defaults, including the SQLite memory backend.
set -euo pipefail

: "${BENCH_PORT:?BENCH_PORT is required}"
: "${BENCH_DATA_DIR:?BENCH_DATA_DIR is required}"
: "${BENCH_MODEL_BASE_URL:?BENCH_MODEL_BASE_URL is required}"

export ZEROCLAW_CONFIG_DIR="$BENCH_DATA_DIR/config"
export ZEROCLAW_DATA_DIR="$BENCH_DATA_DIR/data"
mkdir -p "$ZEROCLAW_CONFIG_DIR" "$ZEROCLAW_DATA_DIR" "$BENCH_DATA_DIR/workspace"

# `uri` is the full endpoint, not a base: ZeroClaw's own V1→V3 migration merges the old
# `api_url` and `api_path` into exactly this field.
cat > "$ZEROCLAW_CONFIG_DIR/config.toml" <<CONF
schema_version = 3
workspace_dir = "$BENCH_DATA_DIR/workspace"

[gateway]
host = "127.0.0.1"
port = ${BENCH_PORT}
require_pairing = false

[providers.models.custom.bench]
uri = "${BENCH_MODEL_BASE_URL}/chat/completions"
api_key = "bench"
model = "${BENCH_MODEL:-gpt-4o-mini}"

[agents.bench]
enabled = true
model_provider = "custom.bench"
risk_profile = "default"
runtime_profile = "default"

[risk_profiles.default]

[runtime_profiles.default]
CONF

exec "${BENCH_ZEROCLAW_BIN:-/home/ubuntu/subjects/zeroclaw}" daemon \
  --host 127.0.0.1 --port "$BENCH_PORT"
