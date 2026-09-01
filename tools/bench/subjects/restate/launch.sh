#!/bin/sh
# Launches the harness service (port 9080) beside restate-server. Both live in the
# process group the runner signals on teardown, so neither outlives the measurement.
here="$(dirname "$0")"
"$here/app/node_modules/.bin/tsx" "$here/app/src/agent.ts" &
export RESTATE_BASE_DIR="${BENCH_DATA_DIR}/restate-data"
export RESTATE_INGRESS__BIND_ADDRESS="127.0.0.1:${BENCH_PORT}"
export DO_NOT_TRACK=1
exec "$here/bin/restate-server"
