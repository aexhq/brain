FROM rust:1.97.1-bookworm AS build
WORKDIR /src
COPY . .
# Strip the symbol table from the shipped binaries: 22-25% smaller with no measured
# change to process readiness. It is set here rather than in `[profile.release]` so
# CI, benchmarks and local release builds keep symbolicated panic backtraces.
ENV RUSTFLAGS="-C strip=symbols"
RUN cargo build --locked --release -p brain-server --bin brain -p brain-loophost --bin brain-loop-worker

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /var/lib/brain brain \
    && install -d -o brain -g brain /var/lib/brain
COPY --from=build /src/target/release/brain /usr/local/bin/brain
COPY --from=build /src/target/release/brain-loop-worker /usr/local/bin/brain-loop-worker
USER 10001:10001
ENV BRAIN_DATA_DIR=/var/lib/brain \
    BRAIN_LOOP_WORKER=/usr/local/bin/brain-loop-worker \
    BRAIN_LISTEN=0.0.0.0:8080
VOLUME ["/var/lib/brain"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/brain"]
