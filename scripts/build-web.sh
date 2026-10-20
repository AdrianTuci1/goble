#!/usr/bin/env bash
# Build the browser client and publish it where goble-gateway serves /app/.
#
# Everything is local: cargo builds the crate for wasm32-unknown-unknown and
# the wasm-bindgen CLI turns it into an ES module. Nothing is downloaded at
# runtime and the served page has no CDN references.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${1:-$ROOT/crates/goble-gateway/web}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
WASM="$TARGET_DIR/wasm32-unknown-unknown/release/goble_web.wasm"
STATIC_INDEX="$ROOT/crates/goble-web/static/index.html"

# The wasm-bindgen CLI and the wasm-bindgen crate must be the same version; a
# mismatch produces glue that cannot load the module. Read the crate version
# from the lockfile so a dependency bump cannot silently drift from this script.
locked_version() {
  awk '
    $0 == "name = \"wasm-bindgen\"" {
      getline
      gsub(/.*version = "/, "")
      gsub(/".*/, "")
      print
      exit
    }
  ' Cargo.lock
}

WASM_BINDGEN_VERSION="$(locked_version)"
if [[ -z "$WASM_BINDGEN_VERSION" ]]; then
  echo "error: could not read the wasm-bindgen version from Cargo.lock" >&2
  exit 1
fi

if ! rustup target list --installed 2>/dev/null | grep -qx "wasm32-unknown-unknown"; then
  cat >&2 <<EOF
error: the wasm32-unknown-unknown target is not installed.

Install it with:
  rustup target add wasm32-unknown-unknown
EOF
  exit 1
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  cat >&2 <<EOF
error: the wasm-bindgen CLI is not on PATH.

Install the version this workspace uses:
  cargo install wasm-bindgen-cli --version $WASM_BINDGEN_VERSION --locked
EOF
  exit 1
fi

INSTALLED_VERSION="$(wasm-bindgen --version | awk '{print $2}')"
if [[ "$INSTALLED_VERSION" != "$WASM_BINDGEN_VERSION" ]]; then
  cat >&2 <<EOF
error: wasm-bindgen CLI version mismatch.

  installed: $INSTALLED_VERSION
  required:  $WASM_BINDGEN_VERSION (from Cargo.lock)

Install the matching CLI:
  cargo install wasm-bindgen-cli --version $WASM_BINDGEN_VERSION --locked
EOF
  exit 1
fi

echo "==> building goble-web for wasm32-unknown-unknown (release)"
cargo build -p goble-web --target wasm32-unknown-unknown --release

if [[ ! -f "$WASM" ]]; then
  echo "error: expected wasm at $WASM" >&2
  exit 1
fi

echo "==> generating web glue in $OUT_DIR"
mkdir -p "$OUT_DIR"
wasm-bindgen --target web --no-typescript --out-dir "$OUT_DIR" "$WASM"
cp "$STATIC_INDEX" "$OUT_DIR/index.html"

for asset in index.html goble_web.js goble_web_bg.wasm; do
  if [[ ! -f "$OUT_DIR/$asset" ]]; then
    echo "error: build did not produce $OUT_DIR/$asset" >&2
    exit 1
  fi
done

echo
echo "client assets:"
for asset in index.html goble_web.js goble_web_bg.wasm; do
  bytes="$(wc -c < "$OUT_DIR/$asset" | tr -d ' ')"
  printf '  %-18s %8s bytes\n' "$asset" "$bytes"
done
echo
echo "Serve with: cargo run -p goble-gateway"
