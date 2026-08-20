FROM rust:1.97.1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p brain-server --bin brain

FROM docker:28.3.3-cli AS docker-cli

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=docker-cli /usr/local/bin/docker /usr/local/bin/docker
COPY --from=build /src/target/release/brain /usr/local/bin/brain
EXPOSE 3210
ENTRYPOINT ["/usr/local/bin/brain"]
