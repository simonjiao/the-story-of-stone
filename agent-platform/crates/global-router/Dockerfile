# syntax=docker/dockerfile:1.7

FROM rust:1.95 AS builder

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/workspace/target \
    cargo build --release -p global-router \
    && mkdir -p /artifacts \
    && cp /workspace/target/release/global-router /artifacts/global-router

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /artifacts/global-router /usr/local/bin/global-router

WORKDIR /app
CMD ["global-router", "serve", "--bind", "0.0.0.0:8099"]
