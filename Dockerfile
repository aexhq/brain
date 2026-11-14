FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5 AS pi-build
WORKDIR /app/runtimes/pi
COPY runtimes/pi/package*.json ./
RUN npm ci --ignore-scripts
COPY runtimes/pi/tsconfig.json ./
COPY runtimes/pi/src ./src
COPY runtimes/common /app/runtimes/common
RUN npm run build && npm prune --omit=dev --ignore-scripts

FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5 AS pi
LABEL org.opencontainers.image.source="https://github.com/aexhq/brain"
COPY --from=pi-build --chown=node:node /app/runtimes /app/runtimes
USER node
WORKDIR /workspace
ENTRYPOINT ["node", "/app/runtimes/pi/dist/main.js"]

FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5 AS tool-runtime
LABEL org.opencontainers.image.source="https://github.com/aexhq/brain"
COPY tools/hosted/runner.mjs /app/tools/hosted/runner.mjs
USER node
WORKDIR /workspace
ENTRYPOINT ["node", "/app/tools/hosted/runner.mjs"]

FROM golang:1.26-bookworm@sha256:d9c68c2c51161e12fd77e4c6320687c9cd86e1af1e3ad6e6cd63ff970641453c AS build
WORKDIR /build
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN --mount=type=cache,target=/root/.cache/go-build CGO_ENABLED=0 go build -trimpath -o /out/brain ./services/brain/cmd/brain && CGO_ENABLED=0 go build -trimpath -o /out/storage ./services/storage/cmd/storage && CGO_ENABLED=0 go build -trimpath -o /out/tools ./services/tools/cmd/tools && CGO_ENABLED=0 go build -trimpath -o /out/sandbox ./services/sandbox/cmd/sandbox

FROM busybox:1.37.0-glibc@sha256:42a9ff19af33811ee466b24c66384461d37c32dcc28d1d27d49b8aca816bd5ee AS base
LABEL org.opencontainers.image.source="https://github.com/aexhq/brain"
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN mkdir -p /data /external && chown 1000:1000 /data /external
USER 1000:1000
WORKDIR /data

FROM base AS storage
COPY --from=build /out/storage /app/storage
EXPOSE 8081
ENTRYPOINT ["/app/storage"]
CMD ["-config", "/config/storage.json"]

FROM base AS tools
COPY --from=build /out/tools /app/tools
EXPOSE 8082
ENTRYPOINT ["/app/tools"]
CMD ["-config", "/config/tools.json"]

FROM docker:29-cli@sha256:5a7af0dcb0e46c803efe9b45371dd21cb500e5b4453250394ac6f06d49b6e6f1 AS sandbox
LABEL org.opencontainers.image.source="https://github.com/aexhq/brain"
COPY --from=build /out/sandbox /app/sandbox
EXPOSE 8083
ENTRYPOINT ["/app/sandbox"]
CMD ["-config", "/config/sandbox.json"]

FROM base AS brain
COPY --from=build /out/brain /app/brain
EXPOSE 8080
ENTRYPOINT ["/app/brain"]
CMD ["-config", "/config/brain.json"]
