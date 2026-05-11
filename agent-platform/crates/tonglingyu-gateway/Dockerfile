# syntax=docker/dockerfile:1.7.0

FROM rust:1.95 AS builder

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/workspace/target \
    cargo build --release -p tonglingyu-gateway \
    && mkdir -p /artifacts \
    && cp /workspace/target/release/tonglingyu-gateway /artifacts/tonglingyu-gateway

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /artifacts/tonglingyu-gateway /usr/local/bin/tonglingyu-gateway

WORKDIR /app
CMD ["tonglingyu-gateway", "serve", "--bind", "0.0.0.0:8090"]
