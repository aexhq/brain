#!/usr/bin/env bash
# Starts `opencode serve` against the benchmark's scripted provider.
#
# OpenCode reads providers from a JSON config file, so the manifest cannot express the
# model endpoint on its own. This writes that file into a config directory the runner
# owns, points every XDG directory OpenCode persists to at the data directory, and execs
# the server from an empty git repository.
#
# What differs from a stock install, and why:
#   * the model provider is the scripted one, which is the whole point of the fixture. It
#     is declared through `@ai-sdk/openai-compatible`, the package OpenCode's own docs
#     name for a `/chat/completions` endpoint, and used for `small_model` too so the
#     title OpenCode writes for a new session is generated against the fixture as well;
#   * `autoupdate` and `share` are off, keeping startup network calls and the sharing
#     upload out of the measurement;
#   * XDG_CONFIG_HOME, XDG_DATA_HOME and XDG_STATE_HOME sit under the data directory the
#     runner creates empty and removes, so no state from a previous run is inherited and
#     the persistence probe sees every byte OpenCode writes. XDG_CACHE_HOME is left alone
#     on purpose: it holds the provider package OpenCode installs on first use, which is a
#     process artifact cache and not session data.
# Everything else is left at OpenCode's defaults, including its tools and its default
# agent: a turn measured without them would be a different turn from the one a user gets.
set -euo pipefail

: "${BENCH_PORT:?BENCH_PORT is required}"
: "${BENCH_DATA_DIR:?BENCH_DATA_DIR is required}"
: "${BENCH_MODEL_BASE_URL:?BENCH_MODEL_BASE_URL is required}"

ROOT="${BENCH_SUBJECTS_ROOT:-$HOME/subjects}"
MODEL="${BENCH_MODEL:-scripted}"

export XDG_CONFIG_HOME="$BENCH_DATA_DIR/config"
export XDG_DATA_HOME="$BENCH_DATA_DIR/data"
export XDG_STATE_HOME="$BENCH_DATA_DIR/state"
WORKSPACE="$BENCH_DATA_DIR/workspace"
mkdir -p "$XDG_CONFIG_HOME/opencode" "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$WORKSPACE"
[ -d "$WORKSPACE/.git" ] || git -C "$WORKSPACE" init -q

export OPENCODE_CONFIG="$XDG_CONFIG_HOME/opencode/opencode.json"
cat > "$OPENCODE_CONFIG" <<CONF
{
  "\$schema": "https://opencode.ai/config.json",
  "autoupdate": false,
  "share": "disabled",
  "model": "bench/${MODEL}",
  "small_model": "bench/${MODEL}",
  "provider": {
    "bench": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Scripted provider",
      "options": { "baseURL": "${BENCH_MODEL_BASE_URL}", "apiKey": "bench" },
      "models": {
        "${MODEL}": { "name": "Scripted provider", "limit": { "context": 200000, "output": 8192 } }
      }
    }
  }
}
CONF

case "$(uname -m)" in
  aarch64) BIN_DEFAULT="$ROOT/node_modules/opencode-linux-arm64/bin/opencode" ;;
  *) BIN_DEFAULT="$ROOT/node_modules/opencode-linux-x64/bin/opencode" ;;
esac

cd "$WORKSPACE"
exec "${BENCH_OPENCODE_BIN:-$BIN_DEFAULT}" serve --port "$BENCH_PORT" --hostname 127.0.0.1
