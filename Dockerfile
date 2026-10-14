FROM rust:1.97.1-bookworm AS build
WORKDIR /src
COPY . .
# Strip the symbol table from the shipped binaries: 22-25% smaller with no measured
# change to process readiness. It is set here rather than in `[profile.release]` so
# CI, benchmarks and local release builds keep symbolicated panic backtraces.
ENV RUSTFLAGS="-C strip=symbols"
RUN cargo build --locked --release -p brain-server --bin brain --bin brain-check-media-upgrade -p brain-env-worker --bin brain-env-worker

FROM node:22.23.2-bookworm-slim AS harnesses
WORKDIR /opt/brain
COPY package.json package-lock.json .npmrc ./
COPY packages/brain-sdk packages/brain-sdk
COPY examples/package.json examples/package.json
RUN npm ci --ignore-scripts && npm run build -w packages/brain-sdk && npm prune --omit=dev --ignore-scripts

FROM oven/bun:1.3.14 AS bun

FROM node:22.23.2-bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates perl-base \
    && rm -rf /var/lib/apt/lists/* \
    && rm -rf /usr/local/lib/node_modules/npm \
    && rm -f /usr/local/bin/npm /usr/local/bin/npx \
    && useradd --system --uid 10001 --home /var/lib/brain brain \
    && install -d -o brain -g brain /var/lib/brain
COPY --from=build /src/target/release/brain /usr/local/bin/brain
COPY --from=build /src/target/release/brain-check-media-upgrade /usr/local/bin/brain-check-media-upgrade
COPY --from=build /src/target/release/brain-env-worker /usr/local/bin/brain-env-worker
COPY --from=harnesses /opt/brain /opt/brain
COPY --from=bun /usr/local/bin/bun /usr/local/bin/bun
USER 10001:10001
ENV BRAIN_DATA_DIR=/var/lib/brain \
    BRAIN_ENV_WORKER=/usr/local/bin/brain-env-worker \
    BRAIN_LISTEN=0.0.0.0:8080 \
    PATH=/opt/brain/node_modules/.bin:$PATH
VOLUME ["/var/lib/brain"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/brain"]
