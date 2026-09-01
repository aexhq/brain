#!/bin/sh
# Launches the built Mastra production server. Provisioning runs `npm install` and
# `npm run build` in app/ first; the build bundles a Hono server into .mastra/output.
here="$(dirname "$0")"
cd "$here/app" || exit 1
exec node .mastra/output/index.mjs
