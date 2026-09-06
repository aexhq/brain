#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d "$root/target/release-fixtures.XXXXXX")
data=$(mktemp -d)
server_pid=
cleanup() {
  if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" || true; fi
  rm -rf "$work" "$data"
}
trap cleanup EXIT
cp "$root"/tests/release/{sdk,http,sdk-turns,sdk-events,http-contract}.mjs "$work/"
cp "${BRAIN_TEST_AGENTLOOP_PACKAGE:?}" "$work/diagnostic-agentloop.wasm"
export BRAIN_API_TOKEN=release-fixture-token
export BRAIN_BASE_URL=http://127.0.0.1:18089
BRAIN_LISTEN=127.0.0.1:18089 BRAIN_DATA_DIR="$data" BRAIN_ENV_WORKER="${BRAIN_TEST_WORKER:?}" \
  "${BRAIN_TEST_SERVER:?}" > "$work/server.log" 2>&1 &
server_pid=$!
for attempt in $(seq 1 120); do
  if curl --fail --silent "$BRAIN_BASE_URL/health/ready" >/dev/null; then break; fi
  if [[ "$attempt" == 120 ]]; then cat "$work/server.log"; exit 1; fi
  sleep 0.1
done
for fixture in sdk http sdk-turns sdk-events http-contract; do
  timeout 120 node "$work/$fixture.mjs"
done
