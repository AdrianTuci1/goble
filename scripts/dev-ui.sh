#!/usr/bin/env bash
# Native UI dev loop.
#
# The UI builder lives in `app/src/ui` (no more hot-reload cdylib/ABI), so
# editing any `app` or `crates` source needs a normal full rebuild. This script
# builds and runs the app once; with cargo-watch installed it rebuilds and
# restarts the app whenever a source file changes.
set -euo pipefail

cd "$(dirname "$0")/.."

if command -v cargo-watch >/dev/null 2>&1; then
    echo "==> Watching app + crates and restarting goble-app on change"
    cargo watch -q -w app -w crates -x "run -p goble-app"
else
    echo "==> cargo-watch not found; building and running goble-app once"
    cargo run -p goble-app
fi
