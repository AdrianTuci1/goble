#!/usr/bin/env bash
# Smoke-test a built `goble-worker-dsh` image: the harness, its composition and
# the worker binary are all inside, and the runtime selection is on by default.
#
#   scripts/smoke-worker-dsh.sh [TAG]
#
# Nothing here talks to a model or a gateway. It proves the image carries what
# the remote DSH runtime needs:
#   * `dsh --version` runs (the harness and its Node runtime are installed),
#   * /opt/dsh/harness.yml is the Goble composition,
#   * `GOBLE_HARNESS=dsh` is in the image environment (selects the run path),
#   * the worker binary is still the entrypoint and is executable.
#
# Environment:
#   DOCKER          docker binary to use (default `docker`)
#   RUN_AS          user to run the container as (default whatever the image sets)
set -euo pipefail

tag="${1:-goble-worker-dsh:dev}"
docker_bin="${DOCKER:-docker}"

if ! command -v "$docker_bin" >/dev/null 2>&1; then
  echo "smoke-worker-dsh: $docker_bin is not on PATH" >&2
  exit 2
fi
if ! "$docker_bin" info >/dev/null 2>&1; then
  echo "smoke-worker-dsh: the Docker daemon is not reachable" >&2
  exit 2
fi

"$docker_bin" run --rm "$tag" sh -lc '
  set -eu
  echo "dsh: $(dsh --version)"
  test -f /opt/dsh/harness.yml
  grep -q "session-persistence-jsonl" /opt/dsh/harness.yml
  test "${GOBLE_HARNESS:-}" = "dsh"
  test "${GOBLE_DSH_HARNESS:-}" = "/opt/dsh/harness.yml"
  test -x /usr/local/bin/goblin
  echo "harness-home: ${DSH_HOME:-}"
  echo "ok"
'
