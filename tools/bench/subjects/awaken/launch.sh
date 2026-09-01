#!/bin/sh
# Launches the Awaken starter server built by build.sh.
#
# OPENAI_API_KEY must be non-empty: without it the starter silently falls back to its
# built-in scripted executor, and every number would be measured against that instead of
# the benchmark's own scripted provider. The trailing slash on OPENAI_BASE_URL is
# load-bearing — the underlying client joins paths with Url::join, and a base without
# the slash loses its /v1 segment.
here="$(dirname "$0")"
export OPENAI_BASE_URL="${BENCH_MODEL_BASE_URL%/}/"
exec "$here/checkout/target/release/ai-sdk-starter-agent"
