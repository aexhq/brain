#!/bin/sh
# Launches the VoltAgent bench subject with the tsx the app's own lockfile installed.
here="$(dirname "$0")"
exec "$here/app/node_modules/.bin/tsx" "$here/app/server.ts"
