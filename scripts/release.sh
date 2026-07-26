#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/crates/goble-desktop"

echo "==> Format Rust"
cd "$ROOT"
cargo fmt --all

echo "==> Check Rust"
cargo check --workspace --all-targets

echo "==> Test Rust"
cargo test --workspace

echo "==> Test frontend"
cd "$DESKTOP"
npm test

echo "==> Build frontend"
npm run build

echo "==> Build Tauri bundles"
npm run tauri build

echo "==> Release artifacts"
ls -lh "$DESKTOP/src-tauri/target/release/bundle/"*/

echo "==> Done"
