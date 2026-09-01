#!/bin/sh
# Provisioning: downloads the Temporal CLI at the pinned version and builds the harness
# worker's venv. Both live beside this script and are gitignored; the launch block only
# runs what this built.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
version="1.8.2"
case "$(uname -m)" in
  x86_64) arch="amd64" ;;
  aarch64) arch="arm64" ;;
  *) echo "unsupported arch" >&2; exit 1 ;;
esac
if [ ! -x "$here/bin/temporal" ]; then
  mkdir -p "$here/bin"
  curl -fsSL "https://github.com/temporalio/cli/releases/download/v${version}/temporal_cli_${version}_linux_${arch}.tar.gz" \
    | tar -xz -C "$here/bin"
fi
"$here/bin/temporal" --version
if [ ! -x "$here/venv/bin/python" ]; then
  python3 -m venv "$here/venv"
fi
"$here/venv/bin/pip" install -q "temporalio[openai-agents,opentelemetry]==1.32.0"
"$here/venv/bin/python" -c "import temporalio.contrib.openai_agents; print('worker deps ok')"
