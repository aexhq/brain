# Standalone Brain

Standalone mode is one durable Brain server for a trusted operator. It stores the journal in
SQLite, encrypts provider keys, MCP headers, and Hand environment values under a local AES-256-GCM
master key, and launches one compatible Docker Hand per active session.

It is a single-node deployment, not an HA scheduler. Docker containers isolate processes and
filesystems but share the host kernel; this is not the hostile multi-tenant isolation provided by
the Aex MicroVM composition.

## Host binary

Set an immutable Hand image, then run the server:

```sh
export BRAIN_HAND_IMAGE='ghcr.io/aexhq/hands@sha256:<digest>'
export BRAIN_DATA_DIR="$PWD/brain-data"
cargo run --release -p brain-server --bin brain
```

Brain binds `127.0.0.1:3210` by default. If `BRAIN_API_TOKEN` is absent it creates a durable
operator token at `$BRAIN_DATA_DIR/operator.token`, prints it once, and subsequently reads it from
that mode-0600 file. The master key is `$BRAIN_DATA_DIR/master.key`; losing it makes encrypted
sessions intentionally unreadable. Back up the complete data directory while Brain is stopped.

The configured Hand image must already exist in the local Docker daemon. Brain fails startup if
Docker, the image, SQLite, the custody key, or stored Hand state cannot be opened; it never falls
back to the in-memory runtime.

## Docker Compose (Linux)

Compose needs an absolute host data path because the Brain container asks the host Docker daemon
to mount session directories into sibling Hand containers:

```sh
export BRAIN_DATA_DIR="$(pwd)/brain-data"
export BRAIN_HAND_IMAGE='ghcr.io/aexhq/hands@sha256:<digest>'
mkdir -p "$BRAIN_DATA_DIR"
docker pull "$BRAIN_HAND_IMAGE"
docker compose up --build -d
```

The server is exposed only on host loopback. Dynamically launched Hands join the private
`brain-runtime` network and publish no host ports. The server container has access to the Docker
socket and therefore has Docker-host authority; protect it as operator infrastructure.

## Development mode

`BRAIN_MODE=development` explicitly selects the old in-memory journal and unsandboxed host
subprocess adapter. It is for tests and local debugging only: sessions do not survive restart and
customer tools are not isolated. Standalone mode is the default.

Important neutral settings include `BRAIN_LISTEN`, `BRAIN_DATA_DIR`, `BRAIN_HAND_IMAGE`,
`BRAIN_DOCKER_BIN`, `BRAIN_DOCKER_NETWORK`, `BRAIN_OUTBOUND_ALLOW_PRIVATE`, and the bounded
`BRAIN_MAX_*` / `BRAIN_MCP_*` controls. Hand tool environment values are supplied only in the
session-create request and are encrypted before the journal write.
