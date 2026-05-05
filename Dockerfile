# syntax=docker/dockerfile:1.7

FROM node:20-bookworm-slim AS web-build

WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --no-fund --no-audit
COPY frontend ./
RUN npm run build

FROM rust:1-bookworm AS build

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release \
    && cp /app/target/release/rikkahub-rs /tmp/rikkahub-rs

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /tmp/rikkahub-rs /usr/local/bin/rikkahub-rs
COPY --from=web-build /app/dist/web-ui-static /web-ui

ENV HOST=0.0.0.0 \
    PORT=8080 \
    DATA_DIR=/data \
    WEB_UI_DIR=/web-ui \
    JWT_ENABLED=true \
    APP_VERSION=rust-preview

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/rikkahub-rs"]
