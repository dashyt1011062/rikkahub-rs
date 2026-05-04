# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS build

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN --mount=type=cache,id=rikkahub-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=rikkahub-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=rikkahub-target,target=/app/target \
    cargo build --release \
    && cp /app/target/release/rikkahub-rs /tmp/rikkahub-rs

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /tmp/rikkahub-rs /usr/local/bin/rikkahub-rs

ENV HOST=0.0.0.0 \
    PORT=8080 \
    DATA_DIR=/data \
    WEB_UI_DIR=/web-ui \
    JWT_ENABLED=true \
    APP_VERSION=rust-preview

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/rikkahub-rs"]
