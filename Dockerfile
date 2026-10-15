FROM node:24-bookworm-slim AS runner
WORKDIR /build/runners/pi
COPY runners/pi/package*.json ./
RUN npm ci --ignore-scripts
COPY runners/pi/tsconfig.json ./
COPY runners/pi/src ./src
RUN npm run build && npm prune --omit=dev --ignore-scripts

FROM golang:1.26-bookworm AS server
WORKDIR /build
COPY go.mod go.sum ./
RUN go mod download
COPY cmd ./cmd
COPY internal ./internal
RUN CGO_ENABLED=0 go build -trimpath -o /brain ./cmd/brain

FROM node:24-bookworm-slim
WORKDIR /app
COPY --from=server /brain /app/brain
COPY --from=runner --chown=node:node /build/runners/pi /app/runners/pi
RUN mkdir /data && chown node:node /data
USER node
EXPOSE 8080
ENTRYPOINT ["/app/brain"]
CMD ["-config", "/config/brain.json"]

