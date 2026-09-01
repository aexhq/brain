#!/bin/sh
# Launches the Temporal dev server beside the harness worker and its shim. Both live in
# the process group the runner signals on teardown, so neither outlives the measurement.
here="$(dirname "$0")"
"$here/bin/temporal" server start-dev \
  --port 7233 \
  --db-filename "${BENCH_DATA_DIR}/temporal.db" \
  --headless &
exec "$here/venv/bin/python" "$here/worker.py"
