#!/usr/bin/env bash
#
# Build a plain .tar.gz fallback distribution for Linux.
#
# This is the lowest-common-denominator artifact: no FUSE, no dpkg, no
# packaging metadata. It is what the updater falls back to on hosts where the
# AppImage cannot be executed. The archive expands to a single versioned
# directory so it can be extracted anywhere:
#
#   goble-<version>-linux-<arch>/
#     goble-app
#     goble.desktop
#     goble.png
#     LICENSE
#     THIRD-PARTY-NOTICES.md
#     README.txt

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGING_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$PACKAGING_DIR/.." && pwd)"

# shellcheck source=../version.env
source "$PACKAGING_DIR/version.env"

APP_NAME="${APP_NAME:-Goble}"
APP_SLUG="${APP_SLUG:-goble}"
BIN_NAME="${BIN_NAME:-goble-app}"
VERSION="${VERSION:-0.0.0}"

DEFAULT_ICON="$ROOT_DIR/crates/goble-desktop-native/resources/icons/128x128@2x.png"
HOMEPAGE="https://github.com/AdrianTuci1/goble"

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Build ${APP_SLUG}-<version>-linux-<arch>.tar.gz from a built binary.

Options:
  --binary PATH     Binary to package (default: target/release/${BIN_NAME})
  --outdir DIR      Output directory (default: <repo>/dist)
  --version VERSION Version used in file names (default: ${VERSION})
  --arch ARCH       x86_64 | aarch64 (default: host arch)
  --icon PATH       PNG icon (default: ${DEFAULT_ICON})
  -h, --help        Show this help
EOF
}

log()  { printf '==> %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required tool '$1' not found in PATH"; }

BINARY=""
OUTDIR="$ROOT_DIR/dist"
DISPLAY_VERSION="$VERSION"
ARCH=""
ICON_SRC="$DEFAULT_ICON"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --binary)   BINARY="${2:?--binary needs a path}"; shift 2 ;;
        --outdir)   OUTDIR="${2:?--outdir needs a path}"; shift 2 ;;
        --version)  DISPLAY_VERSION="${2:?--version needs a value}"; shift 2 ;;
        --arch)     ARCH="${2:?--arch needs a value}"; shift 2 ;;
        --icon)     ICON_SRC="${2:?--icon needs a path}"; shift 2 ;;
        -h|--help)  usage; exit 0 ;;
        *)          die "unknown argument: $1 (try --help)" ;;
    esac
done

need tar
BINARY="${BINARY:-$ROOT_DIR/target/release/$BIN_NAME}"
[[ -f "$BINARY" ]] || die "binary not found: $BINARY
build it first: cargo build --release -p $PKG_NAME"
[[ -f "$ICON_SRC" ]] || die "icon not found: $ICON_SRC"

if [[ -z "$ARCH" ]]; then
    case "$(uname -m)" in
        x86_64|amd64) ARCH="x86_64" ;;
        aarch64|arm64) ARCH="aarch64" ;;
        *) die "unsupported host architecture: $(uname -m) (pass --arch)" ;;
    esac
fi
case "$ARCH" in
    x86_64|amd64)  ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) die "unsupported --arch '$ARCH' (expected x86_64 or aarch64)" ;;
esac

mkdir -p "$OUTDIR"
DIST_NAME="${APP_SLUG}-${DISPLAY_VERSION}-linux-${ARCH}"
OUT_TARBALL="$OUTDIR/${DIST_NAME}.tar.gz"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goble-tar.XXXXXX")"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT INT TERM

PKG_DIR="$TMP_DIR/$DIST_NAME"
mkdir -p "$PKG_DIR"

log "Staging $DIST_NAME"
install -m 0755 "$BINARY" "$PKG_DIR/$BIN_NAME"
install -m 0644 "$SCRIPT_DIR/$APP_SLUG.desktop" "$PKG_DIR/$APP_SLUG.desktop"
install -m 0644 "$ICON_SRC" "$PKG_DIR/$APP_SLUG.png"
[[ -f "$ROOT_DIR/LICENSE" ]] && install -m 0644 "$ROOT_DIR/LICENSE" "$PKG_DIR/LICENSE"
[[ -f "$ROOT_DIR/THIRD-PARTY-NOTICES.md" ]] && install -m 0644 \
    "$ROOT_DIR/THIRD-PARTY-NOTICES.md" "$PKG_DIR/THIRD-PARTY-NOTICES.md"

cat > "$PKG_DIR/README.txt" <<README
$APP_NAME $DISPLAY_VERSION (linux-$ARCH)
Licensed under the MIT License; see LICENSE. Provided "AS IS", without warranty.

Run it in place:
    ./$BIN_NAME

Install per-user (no root):
    install -Dm755 $BIN_NAME           ~/.local/bin/$BIN_NAME
    install -Dm644 $APP_SLUG.desktop   ~/.local/share/applications/$APP_SLUG.desktop
    install -Dm644 $APP_SLUG.png       ~/.local/share/icons/hicolor/256x256/apps/$APP_SLUG.png

Runtime requirements: a Vulkan or GL driver, plus the X11/Wayland client
libraries and fontconfig (see the .deb Depends list for the exact package
names on Debian/Ubuntu).

Source and releases: $HOMEPAGE
README
chmod 0644 "$PKG_DIR/README.txt"

log "Creating $OUT_TARBALL"
rm -f "$OUT_TARBALL"
tar -czf "$OUT_TARBALL" -C "$TMP_DIR" "$DIST_NAME"

log "Tarball ready: $OUT_TARBALL"
