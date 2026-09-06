#!/usr/bin/env bash
set -euo pipefail
# Use an isolated build tool installation; the deployed worker embeds no Python executable.
root=$(cd "$(dirname "$0")/.." && pwd)
output=${1:?provide an output directory}
mkdir -p "$output"
python3 -m venv "$output/venv"
"$output/venv/bin/pip" install --disable-pip-version-check componentize-py==0.25.0
for world in agentloop tool; do
  "$output/venv/bin/componentize-py" -d "$root/crates/brain-env/wit/$world" -w "$world" \
    componentize --stub-wasi -p "$root/tests/fixtures/python" "$world" -o "$output/python-$world.wasm"
done
