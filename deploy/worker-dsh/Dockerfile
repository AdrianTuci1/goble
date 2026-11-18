# syntax=docker/dockerfile:1

# The `goble-worker-dsh` image, built on top of an already-published worker
# image instead of rebuilding the Rust binary.
#
#   docker build -f deploy/worker-dsh/Dockerfile \
#     --build-arg WORKER_IMAGE=ghcr.io/adriantuci1/goble-worker:latest \
#     -t goble-worker-dsh:dev .
#
# The same runtime environment is also built directly by
# `deploy/worker/Dockerfile --build-arg RUNTIME=dsh` (that is the path
# `scripts/build-worker-dsh-image.sh` takes, and the one a release should use
# because it builds the worker from this checkout). This file exists for a fork
# or operator that already has a base worker image and only wants to add the
# harness — e.g. a local `goble-worker:dev` or a registry copy in the
# provider's own cloud.
#
# Both variants must stay in sync; the shared pieces are the pinned `DSH_VERSION`
# and `deploy/worker/harness.dsh.yml`, which both files install.

ARG WORKER_IMAGE=ghcr.io/adriantuci1/goble-worker:latest
ARG DSH_VERSION=0.2.2-rc.3

# The Node 24 pair the harness needs. Bookworm ships Node 18, which the
# harness does not support.
FROM node:24-bookworm-slim AS dsh-node

FROM ${WORKER_IMAGE}

USER root

# `procps` gives the harness's process supervision `ps`/`pkill`; the base image
# already carries the shell, git, ripgrep, curl and ca-certificates an agent
# needs.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends procps \
 && rm -rf /var/lib/apt/lists/*

COPY --from=dsh-node /usr/local /usr/local

# The Goble-owned composition (see `deploy/worker/harness.dsh.yml`). Credentials
# are not part of it; the worker installs them per run and deletes them again.
COPY deploy/worker/harness.dsh.yml /opt/dsh/harness.yml

RUN mkdir -p /opt/dsh \
 && npm i -g --allow-scripts=@deepseek-ai/dsh-subprocess-local,koffi,node-pty,protobufjs \
      "@deepseek-ai/dsh@${DSH_VERSION}" \
 && npm cache clean --force \
 && dsh --version \
 && chown -R goble:goble /home/goble

# Select the remote DSH runtime (read by the worker, `GOBLE_HARNESS_ENV`) and
# point it at the pinned harness home and composition. `GOBLE_DSH_MODEL` is the
# default the worker writes into its generated overlay; a client overrides it by
# setting the variable on the sandbox.
ENV DSH_HOME=/home/goble/.dsh \
    GOBLE_HARNESS=dsh \
    GOBLE_DSH_BIN=dsh \
    GOBLE_DSH_HARNESS=/opt/dsh/harness.yml \
    GOBLE_DSH_MODEL=deepseek-flash \
    DSH_TELEMETRY_DISABLED=1 \
    npm_config_update_notifier=false

# The base image's entrypoint (`goblin --mode=local --bind=0.0.0.0:8787`) and
# working directory are inherited unchanged: the container still starts the
# worker, and the worker runs the harness for a turn.
USER goble
WORKDIR /workspace
