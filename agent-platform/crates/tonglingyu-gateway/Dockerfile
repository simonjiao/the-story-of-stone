ARG APP_VERSION=0.1.14
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

# Base digest verified 2026-05-16: sha256:d7aa809c1ae88f97cd727d6f5f91e4edab579a14f86f52fd0dd8c658c854286d.
FROM cgr.dev/chainguard/glibc-dynamic:latest AS runtime

ARG APP_VERSION=0.1.14
LABEL org.opencontainers.image.title="tonglingyu-gateway" \
    org.opencontainers.image.version="${APP_VERSION}"
ENV TONGLINGYU_VERSION="${APP_VERSION}"

COPY --from=builder /artifacts/tonglingyu-gateway /usr/local/bin/tonglingyu-gateway

WORKDIR /app
CMD ["tonglingyu-gateway", "serve", "--bind", "0.0.0.0:8090"]
