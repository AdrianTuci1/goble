#!/usr/bin/env bash
#
# Build a Linux AppImage for Goble.
#
# Builds an AppDir by hand (no linuxdeploy plugins required), then packages it:
#   1. linuxdeploy --output appimage, when linuxdeploy is on PATH;
#   2. otherwise appimagetool, when appimagetool is on PATH;
#   3. otherwise a clear error explaining how to obtain either tool.
#
# Both tools are expected to bundle the shared-library dependencies that are
# already resolvable; this script does not vendor GTK/Qt stacks because Goble
# only needs the system Vulkan/GL loader and the usual X11/Wayland client libs.

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

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Build ${APP_NAME}-<version>-<arch>.AppImage from a built binary.

Options:
  --binary PATH     Binary to package (default: target/release/${BIN_NAME})
  --outdir DIR      Output directory (default: <repo>/dist)
  --version VERSION Version used in file names (default: ${VERSION})
  --arch ARCH       x86_64 | aarch64 (default: host arch)
  --icon PATH       PNG icon, >=256px (default: ${DEFAULT_ICON})
  --keep-appdir     Leave the generated AppDir next to the output
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
KEEP_APPDIR=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --binary)      BINARY="${2:?--binary needs a path}"; shift 2 ;;
        --outdir)      OUTDIR="${2:?--outdir needs a path}"; shift 2 ;;
        --version)     DISPLAY_VERSION="${2:?--version needs a value}"; shift 2 ;;
        --arch)        ARCH="${2:?--arch needs a value}"; shift 2 ;;
        --icon)        ICON_SRC="${2:?--icon needs a path}"; shift 2 ;;
        --keep-appdir) KEEP_APPDIR=1; shift ;;
        -h|--help)     usage; exit 0 ;;
        *)             die "unknown argument: $1 (try --help)" ;;
    esac
done

[[ "$(uname -s)" == "Linux" ]] || die "make-appimage.sh only runs on Linux"

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

TOOL=""
if command -v linuxdeploy >/dev/null 2>&1; then
    TOOL="linuxdeploy"
elif command -v appimagetool >/dev/null 2>&1; then
    TOOL="appimagetool"
else
    die "neither linuxdeploy nor appimagetool found on PATH.
Install one of them, e.g.:
  linuxdeploy:  https://github.com/linuxdeploy/linuxdeploy/releases  (continuous x86_64.AppImage)
  appimagetool: https://github.com/AppImage/appimagetool/releases
Both are self-contained AppImages; chmod +x and put them on PATH."
fi

mkdir -p "$OUTDIR"

if [[ "$KEEP_APPDIR" -eq 1 ]]; then
    APPDIR="$OUTDIR/${APP_NAME}.AppDir"
    rm -rf "$APPDIR"
    mkdir -p "$APPDIR"
    CLEAN_APPDIR=0
else
    APPDIR="$(mktemp -d "${TMPDIR:-/tmp}/goble-appdir.XXXXXX")/${APP_NAME}.AppDir"
    mkdir -p "$APPDIR"
    CLEAN_APPDIR=1
fi
OUT_APPIMAGE="$OUTDIR/${APP_NAME}-${DISPLAY_VERSION}-${ARCH}.AppImage"

cleanup() {
    local status=$?
    if [[ "$CLEAN_APPDIR" -eq 1 ]]; then
        rm -rf "$(dirname "$APPDIR")"
    fi
    return "$status"
}
trap cleanup EXIT INT TERM

log "Assembling AppDir at $APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
         "$APPDIR/usr/share/doc/$APP_SLUG"

install -m 0755 "$BINARY" "$APPDIR/usr/bin/$BIN_NAME"
install -m 0644 "$SCRIPT_DIR/$APP_SLUG.desktop" \
    "$APPDIR/usr/share/applications/$APP_SLUG.desktop"
install -m 0644 "$SCRIPT_DIR/$APP_SLUG.desktop" "$APPDIR/$APP_SLUG.desktop"
install -m 0644 "$ICON_SRC" \
    "$APPDIR/usr/share/icons/hicolor/256x256/apps/$APP_SLUG.png"
install -m 0644 "$ICON_SRC" "$APPDIR/$APP_SLUG.png"

[[ -f "$ROOT_DIR/LICENSE" ]] && install -m 0644 "$ROOT_DIR/LICENSE" \
    "$APPDIR/usr/share/doc/$APP_SLUG/LICENSE"
[[ -f "$ROOT_DIR/THIRD-PARTY-NOTICES.md" ]] && install -m 0644 \
    "$ROOT_DIR/THIRD-PARTY-NOTICES.md" "$APPDIR/usr/share/doc/$APP_SLUG/"

# AppRun: the AppImage entry point. exec keeps signalling correct (no wrapper
# process lingering between the launcher and the app).
cat > "$APPDIR/AppRun" <<APPRUN
#!/bin/sh
set -e
HERE="\$(dirname "\$(readlink -f "\$0")")"
exec "\$HERE/usr/bin/$BIN_NAME" "\$@"
APPRUN
chmod 0755 "$APPDIR/AppRun"

# appimagetool refuses to run without FUSE in containers; force extraction mode
# there instead of failing with a confusing mount error.
if [[ ! -e /dev/fuse ]]; then
    export APPIMAGE_EXTRACT_AND_RUN=1
fi
export ARCH="$ARCH"

log "Packaging AppImage with $TOOL"
if [[ "$TOOL" == "linuxdeploy" ]]; then
    # linuxdeploy derives the output name from the desktop file + version.
    linuxdeploy --appdir "$APPDIR" --desktop-file "$APPDIR/$APP_SLUG.desktop" \
        --icon-file "$APPDIR/$APP_SLUG.png" --output appimage
    PRODUCED="$(find "$PWD" "$APPDIR" -maxdepth 1 -name '*.AppImage' -type f 2>/dev/null | head -n1 || true)"
    [[ -n "$PRODUCED" ]] || PRODUCED="$APPDIR/${APP_NAME}-${DISPLAY_VERSION}-${ARCH}.AppImage"
else
    appimagetool "$APPDIR" "$OUT_APPIMAGE"
    PRODUCED="$OUT_APPIMAGE"
fi

if [[ "$PRODUCED" != "$OUT_APPIMAGE" ]]; then
    [[ -f "$PRODUCED" ]] || die "packaging tool reported success but produced no AppImage"
    mv -f "$PRODUCED" "$OUT_APPIMAGE"
fi

chmod 0755 "$OUT_APPIMAGE"
log "AppImage ready: $OUT_APPIMAGE"
