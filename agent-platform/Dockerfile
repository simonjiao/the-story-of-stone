# syntax=docker/dockerfile:1.7

FROM rust:1.95 AS builder

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/workspace/target \
    cargo build --release \
        -p agent-manager \
        -p agent-orchestrator \
        -p agent-runtime \
        -p agent-worker \
        -p agentctl \
    && mkdir -p /artifacts \
    && cp /workspace/target/release/agent-manager /artifacts/agent-manager \
    && cp /workspace/target/release/agent-orchestrator /artifacts/agent-orchestrator \
    && cp /workspace/target/release/agent-runtime /artifacts/agent-runtime \
    && cp /workspace/target/release/agent-worker /artifacts/agent-worker \
    && cp /workspace/target/release/agentctl /artifacts/agentctl

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /artifacts/agent-manager /usr/local/bin/agent-manager
COPY --from=builder /artifacts/agent-orchestrator /usr/local/bin/agent-orchestrator
COPY --from=builder /artifacts/agent-runtime /usr/local/bin/agent-runtime
COPY --from=builder /artifacts/agent-runtime /usr/local/bin/agent-action-gateway
COPY --from=builder /artifacts/agent-worker /usr/local/bin/agent-worker
COPY --from=builder /artifacts/agentctl /usr/local/bin/agentctl

WORKDIR /app
CMD ["agent-manager"]
