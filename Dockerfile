FROM rust:1.97.1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p brain-server --bin brain

FROM debian:bookworm-slim
# Local mode executes managed Tool bundles through the host node runtime (and bash for shell
# tools); pin the same node major the repository's JS toolchain uses. The runtime executes
# prebuilt bundles with node alone: npm (and its vendored node-tar, the standing Trivy
# CRITICAL) never belongs in the image.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gnupg \
    && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && apt-get purge -y curl gnupg \
    && rm -rf /var/lib/apt/lists/* /usr/lib/node_modules/npm /usr/bin/npm /usr/bin/npx
COPY --from=build /src/target/release/brain /usr/local/bin/brain
EXPOSE 3210
ENTRYPOINT ["/usr/local/bin/brain"]
