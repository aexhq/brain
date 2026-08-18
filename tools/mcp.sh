#!/usr/bin/env bash
# Runs the slice-7 real-wire MCP operator gate: starts the OFFICIAL reference MCP server
# (@modelcontextprotocol/server-everything, Streamable HTTP) via npx, then runs `bin/mcp`
# against it with a real model. Provider keys are read from the private env file by NAME only
# and never printed. Local mode -- no AWS.
set -euo pipefail
ENVFILE="${AEX_KEYS_FILE:-$HOME/workspace/aex_workspace/aex-backup/.env.dev}"
pick() { grep "^$1=" "$ENVFILE" | head -1 | cut -d= -f2- | tr -d '\r\n'; }
# Same routing as m0.sh: the direct provider keys on this machine are revoked; the Vercel AI
# Gateway serves the Anthropic Messages dialect. Set ANTHROPIC_API_KEY to use a direct key.
if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
  export ANTHROPIC_API_KEY="$(pick VERCEL_AI_GATEWAY_API_KEY)"
  export AEX_MCP_BASE_URL="https://ai-gateway.vercel.sh"
  export AEX_MCP_MODEL="anthropic/claude-haiku-4.5"
fi

PORT="${AEX_MCP_REF_PORT:-3901}"
export AEX_MCP_REF_URL="http://127.0.0.1:${PORT}/mcp"

# The official server, in the background; kill the whole tree on exit (taskkill on Windows,
# plain kill elsewhere).
PORT="$PORT" npx -y @modelcontextprotocol/server-everything streamableHttp &
REF_PID=$!
cleanup() {
  taskkill //F //T //PID "$REF_PID" > /dev/null 2>&1 || kill "$REF_PID" 2> /dev/null || true
}
trap cleanup EXIT

# Ready when the port answers HTTP at all (any status).
for i in $(seq 1 60); do
  if curl -s -o /dev/null "$AEX_MCP_REF_URL" 2> /dev/null; then break; fi
  if [ "$i" = 60 ]; then
    echo "reference server never came up on port $PORT" >&2
    exit 1
  fi
  sleep 1
done

cd "$(dirname "$0")/.."
cargo run --bin mcp "$@"
