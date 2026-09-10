# syntax=docker/dockerfile:1

# Build stage
FROM rust:1.86-slim-bookworm AS builder
WORKDIR /build

# Copy workspace manifests first for caching
COPY Cargo.toml Cargo.toml
COPY crates/goble-core/Cargo.toml crates/goble-core/Cargo.toml
COPY crates/goblin-worker/Cargo.toml crates/goblin-worker/Cargo.toml
COPY crates/goble-cli/Cargo.toml crates/goble-cli/Cargo.toml

# Copy source
COPY crates/goble-core/src crates/goble-core/src
COPY crates/goblin-worker/src crates/goblin-worker/src
COPY crates/goble-cli/src crates/goble-cli/src

# Build release binary for the worker
RUN cargo build --release -p goblin-worker

# Runtime stage: minimal distroless-like image
FROM gcr.io/distroless/cc-debian12:nonroot

# Non-root home is /home/nonroot by default; we use a dedicated /var/goblin path
# via the PVC mount in Kubernetes.
COPY --from=builder --chmod=0755 /build/target/release/goblin /usr/local/bin/goblin

EXPOSE 8787

ENTRYPOINT ["/usr/local/bin/goblin"]
CMD ["--mode=cluster", "--bind=0.0.0.0:8787"]
