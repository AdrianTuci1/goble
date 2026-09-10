#!/usr/bin/env bash
#
# Build a Debian package (.deb) for Goble using dpkg-deb only.
#
# The staging tree is assembled by hand and packaged with
# `dpkg-deb --root-owner-group --build`, so no debhelper/dh_make setup is
# required. Ownership is forced to root:root so the archive is reproducible
# regardless of the build user.

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
MAINTAINER="${MAINTAINER:-Goble Contributors <dev@goble.app>}"

DEFAULT_ICON="$ROOT_DIR/crates/goble-desktop-native/resources/icons/128x128@2x.png"
HOMEPAGE="https://github.com/AdrianTuci1/goble"

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Build ${APP_SLUG}_<version>_<arch>.deb from a built binary.

Options:
  --binary PATH     Binary to package (default: target/release/${BIN_NAME})
  --outdir DIR      Output directory (default: <repo>/dist)
  --version VERSION Version used in the package (default: ${VERSION})
  --arch ARCH       amd64 | arm64 (default: host arch)
  --icon PATH       PNG icon, >=256px (default: ${DEFAULT_ICON})
  --keep-stage      Leave the staging tree next to the output
  -h, --help        Show this help
EOF
}

log()  { printf '==> %s\n' "$*" >&2; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required tool '$1' not found in PATH"; }

BINARY=""
OUTDIR="$ROOT_DIR/dist"
DISPLAY_VERSION="$VERSION"
ARCH=""
ICON_SRC="$DEFAULT_ICON"
KEEP_STAGE=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --binary)     BINARY="${2:?--binary needs a path}"; shift 2 ;;
        --outdir)     OUTDIR="${2:?--outdir needs a path}"; shift 2 ;;
        --version)    DISPLAY_VERSION="${2:?--version needs a value}"; shift 2 ;;
        --arch)       ARCH="${2:?--arch needs a value}"; shift 2 ;;
        --icon)       ICON_SRC="${2:?--icon needs a path}"; shift 2 ;;
        --keep-stage) KEEP_STAGE=1; shift ;;
        -h|--help)    usage; exit 0 ;;
        *)            die "unknown argument: $1 (try --help)" ;;
    esac
done

need dpkg-deb
[[ "$(uname -s)" == "Linux" ]] || die "make-deb.sh only runs on Linux"

BINARY="${BINARY:-$ROOT_DIR/target/release/$BIN_NAME}"
[[ -f "$BINARY" ]] || die "binary not found: $BINARY
build it first: cargo build --release -p $PKG_NAME"
[[ -f "$ICON_SRC" ]] || die "icon not found: $ICON_SRC"

if [[ -z "$ARCH" ]]; then
    case "$(uname -m)" in
        x86_64|amd64) ARCH="amd64" ;;
        aarch64|arm64) ARCH="arm64" ;;
        *) die "unsupported host architecture: $(uname -m) (pass --arch)" ;;
    esac
fi
case "$ARCH" in
    x86_64|amd64)  ARCH="amd64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    *) die "unsupported --arch '$ARCH' (expected amd64 or arm64)" ;;
esac

mkdir -p "$OUTDIR"
OUT_DEB="$OUTDIR/${APP_SLUG}_${DISPLAY_VERSION}_${ARCH}.deb"

if [[ "$KEEP_STAGE" -eq 1 ]]; then
    STAGE="$OUTDIR/${APP_SLUG}-deb-stage"
    rm -rf "$STAGE"
    CLEAN_STAGE=0
else
    STAGE="$(mktemp -d "${TMPDIR:-/tmp}/goble-deb.XXXXXX")/stage"
    CLEAN_STAGE=1
fi

cleanup() {
    local status=$?
    if [[ "$CLEAN_STAGE" -eq 1 ]]; then
        rm -rf "$(dirname "$STAGE")"
    fi
    return "$status"
}
trap cleanup EXIT INT TERM

log "Staging package tree at $STAGE"
mkdir -p "$STAGE/DEBIAN" \
         "$STAGE/usr/bin" \
         "$STAGE/usr/share/applications" \
         "$STAGE/usr/share/icons/hicolor/256x256/apps" \
         "$STAGE/usr/share/doc/$APP_SLUG"

install -m 0755 "$BINARY" "$STAGE/usr/bin/$BIN_NAME"
install -m 0644 "$SCRIPT_DIR/$APP_SLUG.desktop" \
    "$STAGE/usr/share/applications/$APP_SLUG.desktop"
install -m 0644 "$ICON_SRC" \
    "$STAGE/usr/share/icons/hicolor/256x256/apps/$APP_SLUG.png"

if [[ -f "$ROOT_DIR/LICENSE" ]]; then
    install -m 0644 "$ROOT_DIR/LICENSE" "$STAGE/usr/share/doc/$APP_SLUG/LICENSE"
fi
if [[ -f "$ROOT_DIR/THIRD-PARTY-NOTICES.md" ]]; then
    install -m 0644 "$ROOT_DIR/THIRD-PARTY-NOTICES.md" \
        "$STAGE/usr/share/doc/$APP_SLUG/THIRD-PARTY-NOTICES.md"
fi

# DEP-5 machine-readable copyright, derived from the same MIT text as LICENSE.
{
    echo "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
    echo "Upstream-Name: $APP_NAME"
    echo "Source: $HOMEPAGE"
    echo
    echo "Files: *"
    echo "Copyright: 2026 Goble Contributors"
    echo "License: MIT"
    sed 's/^/ /' "$ROOT_DIR/LICENSE" | sed 's/^ $/ ./'
} > "$STAGE/usr/share/doc/$APP_SLUG/copyright"
chmod 0644 "$STAGE/usr/share/doc/$APP_SLUG/copyright"

# Runtime dependencies. wgpu resolves Vulkan or GL at runtime and winit needs
# the X11/Wayland client libraries; the set below is the minimum that makes the
# AppImage-less install work on Debian/Ubuntu. See packaging/README.md.
INSTALLED_KB="$(du -sk "$STAGE" | awk '{print $1}')"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: $APP_SLUG
Version: $DISPLAY_VERSION
Section: devel
Priority: optional
Architecture: $ARCH
Maintainer: $MAINTAINER
Installed-Size: $INSTALLED_KB
Depends: libc6 (>= 2.34), libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libx11-6, libxcb1, libvulkan1, libegl1, libgl1, libfontconfig1, libfreetype6
Homepage: $HOMEPAGE
Description: Reversible execution substrate for agentic work
 $APP_NAME is a native desktop application (wgpu + winit) that turns an agent's
 execution into a reversible, Git-like trace and layers live voice and screen
 interaction on top. It attaches to any harness through a transport seam.
CONTROL

log "Building $OUT_DEB"
rm -f "$OUT_DEB"
dpkg-deb --root-owner-group --build "$STAGE" "$OUT_DEB" >/dev/null

log "Package ready: $OUT_DEB"
