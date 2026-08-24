#!/usr/bin/env bash
# Native UI dev loop with live hot reload.
#
# Builds the goble-app binary once, then watches ONLY the hot-reloadable
# crate (goble-ui-hot). Editing crates/goble-ui-hot/src/lib.rs rebuilds a
# tiny cdylib and the running app swaps it in live — no full binary rebuild
# each iteration, so storage stays clean.
#
# Note: editing crates/goble-ui (the ABI the executable is linked against)
# still requires restarting this script.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Building goble-app with hot-reload feature"
cargo build -p goble-app --features hot-reload

echo "==> Building goble-ui-hot cdylib"
cargo build -p goble-ui-hot

echo "==> Starting goble-app"
cargo run -p goble-app --features hot-reload &
APP_PID=$!
trap 'kill "$APP_PID" 2>/dev/null || true' EXIT INT TERM

if command -v cargo-watch >/dev/null 2>&1; then
    echo "==> Watching crates/goble-ui-hot (cargo-watch)"
    cargo watch -q -w crates/goble-ui-hot -x "build -p goble-ui-hot"
else
    echo "==> cargo-watch not found; falling back to a polling loop"
    while true; do
        cargo build -p goble-ui-hot -q
        sleep 1
    done
fi
