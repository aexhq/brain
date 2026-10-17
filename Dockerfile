FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5 AS runner
WORKDIR /build/runners/pi
COPY runners/pi/package*.json ./
RUN npm ci --ignore-scripts
COPY runners/pi/tsconfig.json ./
COPY runners/pi/src ./src
RUN npm run build && npm prune --omit=dev --ignore-scripts

FROM golang:1.27-bookworm@sha256:5cf287a799e6b94384bad13d16b14904c531f51ba65792237e122ce42b392f61 AS server
WORKDIR /build
COPY go.mod go.sum ./
RUN go mod download
COPY cmd ./cmd
COPY internal ./internal
COPY brain.go server.go ./
RUN CGO_ENABLED=0 go build -trimpath -o /brain ./cmd/brain
RUN CGO_ENABLED=0 go build -trimpath -o /resource-tools ./cmd/resource-tools

FROM node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5
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

