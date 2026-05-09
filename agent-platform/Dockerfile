FROM rust:1.95 AS builder

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --workspace --bins

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /workspace/target/release/agent-manager /usr/local/bin/agent-manager
COPY --from=builder /workspace/target/release/agent-orchestrator /usr/local/bin/agent-orchestrator
COPY --from=builder /workspace/target/release/agent-runtime /usr/local/bin/agent-runtime
COPY --from=builder /workspace/target/release/agent-worker /usr/local/bin/agent-worker
COPY --from=builder /workspace/target/release/agentctl /usr/local/bin/agentctl
COPY --from=builder /workspace/target/release/tonglingyu-gateway /usr/local/bin/tonglingyu-gateway

WORKDIR /app
CMD ["agent-manager"]
