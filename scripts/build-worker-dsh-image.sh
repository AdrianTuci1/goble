#!/usr/bin/env bash
# Build the worker image with the remote DSH runtime installed.
#
#   scripts/build-worker-dsh-image.sh [TAG]
#
# The result is the normal Goble worker image plus the pinned
# `@deepseek-ai/dsh`, a Goble-owned harness composition at /opt/dsh/harness.yml,
# and `GOBLE_HARNESS=dsh` in the environment, which selects the worker's remote
# DSH run path. See docs/remote-dsh-runtime.md.
#
# Environment:
#   DSH_VERSION   harness npm version to pin (default 0.2.2-rc.3)
#   DOCKER        docker binary to use (default `docker`)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="${1:-goble-worker-dsh:dev}"
dsh_version="${DSH_VERSION:-0.2.2-rc.3}"
docker_bin="${DOCKER:-docker}"

if ! command -v "$docker_bin" >/dev/null 2>&1; then
  echo "build-worker-dsh-image: $docker_bin is not on PATH" >&2
  exit 2
fi
if ! "$docker_bin" info >/dev/null 2>&1; then
  echo "build-worker-dsh-image: the Docker daemon is not reachable" >&2
  exit 2
fi

echo "build-worker-dsh-image: building $tag (dsh $dsh_version)"
"$docker_bin" build \
  -f "$root/deploy/worker/Dockerfile" \
  --build-arg RUNTIME=dsh \
  --build-arg "DSH_VERSION=$dsh_version" \
  -t "$tag" \
  "$root"

echo "build-worker-dsh-image: built $tag"
echo
echo "Smoke it without a gateway:"
echo "  scripts/smoke-worker-dsh.sh $tag"
echo
echo "Or point a [remote_agents.<name>] table at it (image = \"$tag\")."
