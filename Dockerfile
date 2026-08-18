# The brain server image. Zero-config local mode by default (in-memory journal, subprocess
# tools — NOT durable, NOT a sandbox, the server banners it); AEX_MODE=aws + env wires the
# production substrate (DynamoDB, KMS, S3, Lambda MicroVM hands). See crates/brain-server.
FROM rust:1.97-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p brain-server --bin brain

FROM debian:bookworm-slim
# ca-certificates: TLS to model providers and AWS. git: the most-wanted tool in local mode
# (the curated toolset lives in the MicroVM image; local mode runs on what THIS image has).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/brain /usr/local/bin/brain
ENV AEX_DATA_DIR=/data
VOLUME /data
EXPOSE 8700
ENTRYPOINT ["/usr/local/bin/brain"]
