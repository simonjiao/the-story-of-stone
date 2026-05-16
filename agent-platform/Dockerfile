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

# Base digest verified 2026-05-16: sha256:d7aa809c1ae88f97cd727d6f5f91e4edab579a14f86f52fd0dd8c658c854286d.
FROM cgr.dev/chainguard/glibc-dynamic:latest AS runtime

COPY --from=builder /artifacts/agent-manager /usr/local/bin/agent-manager
COPY --from=builder /artifacts/agent-orchestrator /usr/local/bin/agent-orchestrator
COPY --from=builder /artifacts/agent-runtime /usr/local/bin/agent-runtime
COPY --from=builder /artifacts/agent-runtime /usr/local/bin/agent-action-gateway
COPY --from=builder /artifacts/agent-worker /usr/local/bin/agent-worker
COPY --from=builder /artifacts/agentctl /usr/local/bin/agentctl
COPY crates/agent-store/migrations /usr/local/share/agent-platform/migrations

ENV AGENT_STORE_MIGRATIONS_DIR=/usr/local/share/agent-platform/migrations

WORKDIR /app
CMD ["agent-manager"]
