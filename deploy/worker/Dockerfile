# syntax=docker/dockerfile:1

# The image a remote agent runs in: `[remote_agents.<name>] image` in
# `~/.goble/config.toml`. Built and published as
# `ghcr.io/<owner>/goble-worker` by `.github/workflows/worker-image.yml`.
#
# Not the same thing as `deploy/goblin/Dockerfile`: that one is the
# Kubernetes/distroless image for `charts/goblin-cluster`, with a PVC, leader
# election and `--mode=cluster`. A sandbox is one throwaway container with no
# volume and no peers, and the agent inside it has to be able to run `git`,
# `rg` and a shell, which distroless cannot do. Hence a Debian runtime, `local`
# mode, and a worker that only dials out to the gateway.
#
# Build from the repository root, because the build stage copies the workspace:
#
#   docker build -f deploy/worker/Dockerfile -t goble-worker:dev .

# --- build ------------------------------------------------------------------
# `1-slim-bookworm` is the newest stable 1.x, which is the channel `release.yml`
# and `ci.yml` build the worker with (`dtolnay/rust-toolchain@stable`). Pinning
# the workspace's declared `rust-version` (1.86) does not work: the repository
# tracks no `Cargo.lock`, and crates in the worker's own closure already declare
# a newer minimum (`sha2` 0.11 wants 1.88, `time` 0.3.55 wants 1.88), so a 1.86
# toolchain refuses to build them. Both stages are bookworm, so the glibc the
# binary links against is the one the runtime ships.
FROM rust:1-slim-bookworm AS builder
WORKDIR /build

# The whole tree, not just `crates/goblin-worker`: cargo loads every workspace
# member's manifest (and `goble-terminal`'s vendored path dependency) before it
# builds one package, so a missing crate fails the build even when the worker
# does not link it. `.dockerignore` keeps `target/` and the rest of a local
# checkout out of the context. `Cargo.lock` is not tracked, so CI resolves the
# dependency graph fresh and a local build uses whatever lock file it has.
COPY . .

# A cache mount on `target` keeps the compiled dependencies between builds on
# the same builder; the binary has to be copied out inside this step because the
# mount is not part of the image.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release -p goblin-worker \
 && cp /build/target/release/goblin /usr/local/bin/goblin

# --- runtime ----------------------------------------------------------------
FROM debian:bookworm-slim

# What an agent inside the sandbox needs on PATH: a shell, `git` to clone and
# commit, `rg` for the search tools, `curl` for the health check and for
# anything the agent fetches by hand. `ca-certificates` is what makes the
# outbound `wss://` gateway connection and `https://` git remotes verify.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
      git \
      ripgrep \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder --chmod=0755 /usr/local/bin/goblin /usr/local/bin/goblin

# A non-root user with a writable home: the worker opens its task store, vault
# and workspace database under it, and `HOME` is where the harness and `git`
# keep their own state. `/workspace` is the shared project directory
# (`GOBLE_WORKSPACE_PATH` is always `/workspace/<slug>`), so it is owned by the
# same user.
RUN useradd --create-home --uid 10001 --shell /bin/bash goble \
 && mkdir -p /workspace /home/goble/.goble \
 && chown -R goble:goble /workspace /home/goble

# The binary's defaults write outside a non-root home (`/var/goblin/tasks.db`,
# `/var/goblin/vault.json`, `/var/run/goblin.pid`, `/var/log/goblin.log`), which
# is the one thing that makes an unconfigured container fail at startup. Every
# path it owns is moved under the home directory here, so the image runs
# unchanged wherever a provider puts it.
ENV HOME=/home/goble \
    GOBLIN_WORKSPACE_ROOT=/workspace \
    GOBLIN_TASK_STORE=/home/goble/.goble/tasks.db \
    GOBLIN_VAULT_PATH=/home/goble/.goble/vault.json \
    GOBLIN_PID_FILE=/home/goble/.goble/goblin.pid \
    GOBLIN_LOG_FILE=/home/goble/.goble/goblin.log \
    GOBLIN_MODE=local

USER goble
WORKDIR /workspace

# The worker still serves its inbound endpoints in local mode; nothing outside
# the sandbox has to reach this port, it is here for `docker run -p` while
# trying the image out.
EXPOSE 8787

# `GET /health` is the worker's own readiness report and needs no token unless
# the worker was started with `--tls-bundle`.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8787/health || exit 1

# `local`, not `cluster`: `--mode=cluster` is the Kubernetes shape, which moves
# the databases under `--pvc-root`, elects a leader through a `Lease` and
# snapshots to a mounted volume. A sandbox has none of that. The gateway
# connection is not a mode — `GOBLE_GATEWAY_URL` plus `GOBLE_WORKER_TOKEN`,
# which the provisioner passes as sandbox env, make the worker dial out. `--mode`
# is a free-form string whose only special value is `cluster`
# (`crates/goblin-worker/src/main.rs`, `cluster_mode = args.mode == "cluster"`),
# and `GOBLIN_MODE` repeats it for a caller that overrides the command.
ENTRYPOINT ["/usr/local/bin/goblin"]
CMD ["--mode=local", "--bind=0.0.0.0:8787"]
