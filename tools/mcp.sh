#!/usr/bin/env bash
# Runs the real-wire MCP interoperability check: starts the official reference MCP server
# (@modelcontextprotocol/server-everything, Streamable HTTP) via npx, then runs `bin/mcp`
# against it with a real model. Provider credentials come from the environment and are never
# printed. Local mode -- no AWS.
set -euo pipefail
# The optional Vercel AI Gateway serves the Anthropic Messages dialect. Set
# ANTHROPIC_API_KEY to use a direct key.
if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
  : "${VERCEL_AI_GATEWAY_API_KEY:?set ANTHROPIC_API_KEY or VERCEL_AI_GATEWAY_API_KEY}"
  export ANTHROPIC_API_KEY="$VERCEL_AI_GATEWAY_API_KEY"
  export BRAIN_MCP_BASE_URL="https://ai-gateway.vercel.sh"
  export BRAIN_MCP_MODEL="anthropic/claude-haiku-4.5"
fi

PORT="${BRAIN_MCP_REF_PORT:-3901}"
export BRAIN_MCP_REF_URL="http://127.0.0.1:${PORT}/mcp"

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
  if curl -s -o /dev/null "$BRAIN_MCP_REF_URL" 2> /dev/null; then break; fi
  if [ "$i" = 60 ]; then
    echo "reference server never came up on port $PORT" >&2
    exit 1
  fi
  sleep 1
done

cd "$(dirname "$0")/.."
cargo run --bin mcp "$@"
