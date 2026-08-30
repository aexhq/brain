#!/usr/bin/env bash
# Starts Letta's own container the way its Docker page documents it, with two additions
# the benchmark needs and one it cannot do without.
#
#   $1  image tag           pinned in subject.json, passed in so it is named once
#   $2  data directory      bind-mounted over the container's PGDATA, so the persistence
#                           probe can watch the store Letta actually writes to
#   $3  scripted provider   where every model call has to go
#
# A wrapper rather than a bare `docker run` in the manifest because a container outlives
# the client that started it: the runner stops a subject by signalling its process group,
# which reaches the docker client and not the server, and a Letta left holding port 8283
# and the data directory would corrupt the next run rather than this one.
set -euo pipefail

image=${1:?image tag}
data_dir=${2:?data directory}
model_base_url=${3:?scripted provider base url}
name=bench-letta

cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT INT TERM
cleanup

# OPENAI_API_KEY is a literal placeholder, not a credential: the scripted provider ignores
# it. It has to be *set to something*, because Letta enables its built-in openai provider
# only when the key is non-empty, and OPENAI_BASE_URL is only read when that provider is
# enabled. Passing the name through with no value — which is what a real key would require
# — is what left Letta with no models at all and sent it to api.openai.com.
docker run --rm --name "$name" --network host \
  -e OPENAI_API_KEY=bench \
  -e OPENAI_BASE_URL="$model_base_url" \
  -v "$data_dir":/var/lib/postgresql/data \
  "$image" &
wait $!
