#!/bin/sh
# Provisioning: downloads restate-server at the pinned version and installs the harness
# service's dependencies. The binary and node_modules live beside this script and are
# gitignored; the launch block only runs what this built.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
version="1.7.8"
arch="$(uname -m)"
if [ ! -x "$here/bin/restate-server" ]; then
  mkdir -p "$here/bin"
  curl -fsSL "https://restate.gateway.scarf.sh/v${version}/restate-server-${arch}-unknown-linux-musl.tar.xz" \
    | tar -xJ -C "$here/bin" --strip-components=1
fi
"$here/bin/restate-server" --version
cd "$here/app" && npm install --no-audit --no-fund
