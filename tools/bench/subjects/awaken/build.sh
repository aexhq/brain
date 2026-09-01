#!/bin/sh
# Provisioning: clones Awaken at the pinned tag and builds its starter server.
# The checkout lives beside this script and is gitignored; the launch block only runs
# the built binary.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
tag="v0.6.0"
if [ ! -d "$here/checkout" ]; then
  git clone --depth 1 --branch "$tag" https://github.com/AwakenWorks/awaken "$here/checkout"
fi
cd "$here/checkout"
cargo build --release -p ai-sdk-starter-agent
ls -la target/release/ai-sdk-starter-agent
