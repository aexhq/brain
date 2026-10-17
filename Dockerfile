FROM node:25-bookworm-slim@sha256:81db02c4b671288a03915da9534dbd54f96d0e7c24d80ccc54f5b36b2e684370 AS runner
WORKDIR /build/runners/pi
COPY runners/pi/package*.json ./
RUN npm ci --ignore-scripts
COPY runners/pi/tsconfig.json ./
COPY runners/pi/src ./src
RUN npm run build && npm prune --omit=dev --ignore-scripts

FROM golang:1.26-bookworm@sha256:d9c68c2c51161e12fd77e4c6320687c9cd86e1af1e3ad6e6cd63ff970641453c AS server
WORKDIR /build
COPY go.mod go.sum ./
RUN go mod download
COPY cmd ./cmd
COPY internal ./internal
COPY brain.go server.go ./
RUN CGO_ENABLED=0 go build -trimpath -o /brain ./cmd/brain
RUN CGO_ENABLED=0 go build -trimpath -o /resource-tools ./cmd/resource-tools

FROM node:25-bookworm-slim@sha256:81db02c4b671288a03915da9534dbd54f96d0e7c24d80ccc54f5b36b2e684370
LABEL org.opencontainers.image.source="https://github.com/aexhq/brain"
WORKDIR /app
COPY --from=server /brain /app/brain
COPY --from=server /resource-tools /app/resource-tools
COPY --from=runner --chown=node:node /build/runners/pi /app/runners/pi
RUN mkdir /data && chown node:node /data
USER node
EXPOSE 8080
ENTRYPOINT ["/app/brain"]
CMD ["-config", "/config/brain.json"]

