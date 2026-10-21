FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5 AS runner
WORKDIR /build/runners/pi
COPY runners/pi/package*.json ./
RUN npm ci --ignore-scripts
COPY runners/pi/tsconfig.json ./
COPY runners/pi/src ./src
COPY runners/common /build/runners/common
RUN npm run build && npm prune --omit=dev --ignore-scripts
WORKDIR /build/runners/codex
COPY runners/codex/package*.json ./
RUN npm ci --ignore-scripts
COPY runners/codex/src ./src
WORKDIR /build/runners/opencode
COPY runners/opencode/package*.json ./
RUN npm ci --ignore-scripts
COPY runners/opencode/src ./src

FROM golang:1.26-bookworm@sha256:d9c68c2c51161e12fd77e4c6320687c9cd86e1af1e3ad6e6cd63ff970641453c AS server
WORKDIR /build
COPY go.mod go.sum ./
RUN go mod download
COPY cmd ./cmd
COPY internal ./internal
COPY integrations ./integrations
COPY *.go ./
RUN CGO_ENABLED=0 go build -trimpath -o /brain ./cmd/brain
RUN CGO_ENABLED=0 go build -trimpath -o /resource-tools ./cmd/resource-tools

FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5
LABEL org.opencontainers.image.source="https://github.com/aexhq/brain"
WORKDIR /app
COPY --from=server /brain /app/brain
COPY --from=server /resource-tools /app/resource-tools
COPY --from=runner --chown=node:node /build/runners /app/runners
RUN mkdir /data && chown node:node /data
USER node
EXPOSE 8080
ENTRYPOINT ["/app/brain"]
CMD ["-config", "/config/brain.json"]

