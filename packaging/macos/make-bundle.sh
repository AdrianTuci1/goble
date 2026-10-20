#!/usr/bin/env bash
#
# Assemble a macOS .app bundle for Goble from an already-built binary.
#
# Layout produced (default: dist/Goble.app):
#
#   Goble.app/Contents/Info.plist          rendered from packaging/macos/Info.plist
#   Goble.app/Contents/MacOS/goble-app     the release binary (chmod +x)
#   Goble.app/Contents/MacOS/goble-pty-daemon   the attachable PTY daemon
#   Goble.app/Contents/MacOS/goble-cli          the control CLI
#   Goble.app/Contents/MacOS/goble-mcp          the MCP stdio server
#   Goble.app/Contents/Resources/AppIcon.icns
#   Goble.app/Contents/PkgInfo
#
# The daemon and control clients share Contents/MacOS with the app because that
# is the sibling path `goble_pty::Client` looks in before falling back to PATH.
#
# The script never builds Rust. It expects
# `cargo build --release -p goble-app -p goble-pty -p goble-cli -p goble-mcp`
# to have run already (or --binary/--*-binary to point at equivalent artifacts).
#
# Signing: when APPLE_SIGNING_IDENTITY is set the bundle is hardened-runtime
# signed with a secure timestamp. Without it the bundle is produced unsigned
# and a clear warning is printed; the DMG script relies on this script having
# signed the app *before* notarization.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGING_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$PACKAGING_DIR/.." && pwd)"

# shellcheck source=../version.env
source "$PACKAGING_DIR/version.env"
# shellcheck source=../signing.sh
source "$PACKAGING_DIR/signing.sh"

APP_NAME="${APP_NAME:-Goble}"
BIN_NAME="${BIN_NAME:-goble-app}"
PTY_DAEMON_NAME="${PTY_DAEMON_NAME:-goble-pty-daemon}"
CLI_NAME="${CLI_NAME:-goble-cli}"
MCP_NAME="${MCP_NAME:-goble-mcp}"
BUNDLE_ID="${BUNDLE_ID:-com.goble.app}"
VERSION="${VERSION:-0.0.0}"

# Default icon source. It is a 256x256 PNG in-repo; pass --icon with a >=1024
# PNG or an existing .icns for a crisp Retina icon (see packaging/README.md).
DEFAULT_ICON="$ROOT_DIR/crates/goble-desktop-native/resources/icons/icon.png"

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Assemble dist/${APP_NAME}.app from a built binary.

Options:
  --binary PATH          Binary to bundle (default: target/release/${BIN_NAME})
  --pty-daemon-binary PATH  PTY daemon (default: target/release/${PTY_DAEMON_NAME})
  --cli-binary PATH         Control CLI (default: target/release/${CLI_NAME})
  --mcp-binary PATH         MCP server (default: target/release/${MCP_NAME})
  --outdir DIR           Output directory for the bundle (default: <repo>/dist)
  --version VERSION      Version in Info.plist (default: ${VERSION})
  --arch ARCH            Label only; one of aarch64, x86_64 (informational)
  --universal            Build a universal app; requires both
                         --x86_64-binary and --aarch64-binary for every binary
  --x86_64-binary PATH   x86_64 slice of the app, used with --universal
  --aarch64-binary PATH  arm64 slice of the app, used with --universal
  --pty-daemon-x86_64-binary PATH   x86_64 PTY daemon slice
  --pty-daemon-aarch64-binary PATH  arm64 PTY daemon slice
  --cli-x86_64-binary PATH          x86_64 CLI slice
  --cli-aarch64-binary PATH         arm64 CLI slice
  --mcp-x86_64-binary PATH          x86_64 MCP slice
  --mcp-aarch64-binary PATH         arm64 MCP slice
  --icon PATH            .icns file, or a PNG (>=512px recommended)
  --sign-identity ID     codesign identity (default: \$APPLE_SIGNING_IDENTITY)
  --entitlements PATH    Entitlements plist (default: macos/entitlements.plist)
  --app-name NAME        Bundle display/name (default: ${APP_NAME})
  --bundle-id ID         CFBundleIdentifier (default: ${BUNDLE_ID})
  --build-number N       CFBundleVersion (default: \$GOBLE_BUILD_NUMBER or version)
  -h, --help             Show this help

Environment:
  APPLE_SIGNING_IDENTITY  Developer ID identity, e.g.
                          "Developer ID Application: Example (TEAMID)"
EOF
}

log()  { printf '==> %s\n' "$*" >&2; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required tool '$1' not found in PATH"; }

# Escape a value for use on the right-hand side of a sed s||| expression.
sed_escape() { printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'; }

BINARY=""
PTY_DAEMON_BINARY=""
CLI_BINARY=""
MCP_BINARY=""
OUTDIR="$ROOT_DIR/dist"
DISPLAY_VERSION="$VERSION"
ARCH_LABEL=""
UNIVERSAL=0
X86_BINARY=""
ARM_BINARY=""
PTY_DAEMON_X86_BINARY=""
PTY_DAEMON_ARM_BINARY=""
CLI_X86_BINARY=""
CLI_ARM_BINARY=""
MCP_X86_BINARY=""
MCP_ARM_BINARY=""
ICON_SRC="${APP_ICON:-$DEFAULT_ICON}"
SIGN_IDENTITY="${APPLE_SIGNING_IDENTITY:-}"
ENTITLEMENTS="$SCRIPT_DIR/entitlements.plist"
BUILD_NUMBER="${GOBLE_BUILD_NUMBER:-}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --binary)          BINARY="${2:?--binary needs a path}"; shift 2 ;;
        --pty-daemon-binary) PTY_DAEMON_BINARY="${2:?--pty-daemon-binary needs a path}"; shift 2 ;;
        --cli-binary)      CLI_BINARY="${2:?--cli-binary needs a path}"; shift 2 ;;
        --mcp-binary)      MCP_BINARY="${2:?--mcp-binary needs a path}"; shift 2 ;;
        --outdir)          OUTDIR="${2:?--outdir needs a path}"; shift 2 ;;
        --version)         DISPLAY_VERSION="${2:?--version needs a value}"; shift 2 ;;
        --arch)            ARCH_LABEL="${2:?--arch needs a value}"; shift 2 ;;
        --universal)       UNIVERSAL=1; shift ;;
        --x86_64-binary)   X86_BINARY="${2:?--x86_64-binary needs a path}"; shift 2 ;;
        --aarch64-binary)  ARM_BINARY="${2:?--aarch64-binary needs a path}"; shift 2 ;;
        --pty-daemon-x86_64-binary)  PTY_DAEMON_X86_BINARY="${2:?--pty-daemon-x86_64-binary needs a path}"; shift 2 ;;
        --pty-daemon-aarch64-binary) PTY_DAEMON_ARM_BINARY="${2:?--pty-daemon-aarch64-binary needs a path}"; shift 2 ;;
        --cli-x86_64-binary)         CLI_X86_BINARY="${2:?--cli-x86_64-binary needs a path}"; shift 2 ;;
        --cli-aarch64-binary)        CLI_ARM_BINARY="${2:?--cli-aarch64-binary needs a path}"; shift 2 ;;
        --mcp-x86_64-binary)         MCP_X86_BINARY="${2:?--mcp-x86_64-binary needs a path}"; shift 2 ;;
        --mcp-aarch64-binary)        MCP_ARM_BINARY="${2:?--mcp-aarch64-binary needs a path}"; shift 2 ;;
        --icon)            ICON_SRC="${2:?--icon needs a path}"; shift 2 ;;
        --sign-identity)   SIGN_IDENTITY="${2:?--sign-identity needs a value}"; shift 2 ;;
        --entitlements)    ENTITLEMENTS="${2:?--entitlements needs a path}"; shift 2 ;;
        --app-name)        APP_NAME="${2:?--app-name needs a value}"; shift 2 ;;
        --bundle-id)       BUNDLE_ID="${2:?--bundle-id needs a value}"; shift 2 ;;
        --build-number)    BUILD_NUMBER="${2:?--build-number needs a value}"; shift 2 ;;
        -h|--help)         usage; exit 0 ;;
        *)                 die "unknown argument: $1 (try --help)" ;;
    esac
done

need mktemp
[[ "$(uname -s)" == "Darwin" ]] || die "make-bundle.sh only runs on macOS"

# Fail before doing any work, not after assembling a bundle nobody can ship.
require_macos_signing "$SIGN_IDENTITY"
if [[ -n "$SIGN_IDENTITY" ]]; then
    [[ -f "$ENTITLEMENTS" ]] || die "entitlements file not found: $ENTITLEMENTS"
fi

BUNDLE="$OUTDIR/$APP_NAME.app"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goble-bundle.XXXXXX")"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT INT TERM

resolve_single_binary() {
    if [[ -n "$BINARY" ]]; then
        BINARY="$(cd "$(dirname "$BINARY")" && pwd)/$(basename "$BINARY")"
    else
        BINARY="$ROOT_DIR/target/release/$BIN_NAME"
    fi
    [[ -f "$BINARY" ]] || die "binary not found: $BINARY
build it first: cargo build --release -p $PKG_NAME"
    [[ -x "$BINARY" ]] || die "binary is not executable: $BINARY"
}

STAGED_BINARY="$TMP_DIR/$BIN_NAME"
if [[ "$UNIVERSAL" -eq 1 ]]; then
    need lipo
    [[ -n "$X86_BINARY" && -n "$ARM_BINARY" ]] || die \
        "--universal requires both --x86_64-binary and --aarch64-binary"
    for f in "$X86_BINARY" "$ARM_BINARY"; do
        [[ -f "$f" ]] || die "slice not found: $f"
    done
    log "Merging universal binary (x86_64 + arm64)"
    lipo -create -output "$STAGED_BINARY" "$X86_BINARY" "$ARM_BINARY"
    ARCH_LABEL="universal"
else
    resolve_single_binary
    cp "$BINARY" "$STAGED_BINARY"
    if [[ -z "$ARCH_LABEL" ]]; then
        ARCH_LABEL="$(lipo -archs "$BINARY" 2>/dev/null || uname -m)"
    fi
fi
chmod +x "$STAGED_BINARY"

# --- companions ------------------------------------------------------------
# The daemon and the two control clients are plain executables, not nested
# bundles, so they sit directly in Contents/MacOS. Each is fattened separately
# for a universal build, exactly like the app binary.
COMPANION_NAMES=("$PTY_DAEMON_NAME" "$CLI_NAME" "$MCP_NAME")
COMPANION_SINGLE=("$PTY_DAEMON_BINARY" "$CLI_BINARY" "$MCP_BINARY")
COMPANION_X86=("$PTY_DAEMON_X86_BINARY" "$CLI_X86_BINARY" "$MCP_X86_BINARY")
COMPANION_ARM=("$PTY_DAEMON_ARM_BINARY" "$CLI_ARM_BINARY" "$MCP_ARM_BINARY")

stage_companion() {
    local name="$1" single="$2" x86="$3" arm="$4"
    local staged="$TMP_DIR/$name"

    if [[ "$UNIVERSAL" -eq 1 ]]; then
        [[ -n "$x86" && -n "$arm" ]] || die \
            "--universal requires both x86_64 and aarch64 slices for $name"
        [[ -f "$x86" ]] || die "slice not found: $x86"
        [[ -f "$arm" ]] || die "slice not found: $arm"
        log "Merging universal $name (x86_64 + arm64)"
        lipo -create -output "$staged" "$x86" "$arm"
    else
        [[ -n "$single" ]] || single="$(dirname "$BINARY")/$name"
        [[ -f "$single" ]] || die "companion binary not found: $single
build all four first: cargo build --release -p goble-app -p goble-pty -p goble-cli -p goble-mcp"
        [[ -x "$single" ]] || die "companion binary is not executable: $single"
        cp "$single" "$staged"
    fi
    chmod +x "$staged"
}

for i in "${!COMPANION_NAMES[@]}"; do
    stage_companion "${COMPANION_NAMES[$i]}" "${COMPANION_SINGLE[$i]}" \
        "${COMPANION_X86[$i]}" "${COMPANION_ARM[$i]}"
done

# --- icon ------------------------------------------------------------------
build_icns() {
    local src="$1" dest="$2"
    if [[ "$src" == *.icns ]]; then
        [[ -f "$src" ]] || die "icon not found: $src"
        log "Using prebuilt icon $src"
        cp "$src" "$dest"
        return
    fi
    [[ -f "$src" ]] || die "icon not found: $src"
    need sips
    need iconutil

    local src_w src_h
    read -r src_w src_h <<<"$(sips -g pixelWidth -g pixelHeight "$src" 2>/dev/null \
        | awk '/pixelWidth|pixelHeight/ {print $2}' | paste -sd' ' -)" || true
    [[ -n "$src_w" && -n "$src_h" ]] || die "could not read pixel size of $src"
    if (( src_w < 1024 )); then
        warn "icon source is ${src_w}x${src_h}; sizes above that will be upscaled."
        warn "pass --icon with a 1024x1024 PNG for a crisp Retina icon."
    fi

    local iconset="$TMP_DIR/AppIcon.iconset"
    mkdir -p "$iconset"
    # Standard iconset members. Anything larger than the source is upscaled by
    # sips so iconutil always receives a complete set.
    local spec
    for spec in "16 16x16" "32 16x16@2x" "32 32x32" "64 32x32@2x" \
                "128 128x128" "256 128x128@2x" "256 256x256" "512 256x256@2x" \
                "512 512x512" "1024 512x512@2x"; do
        local px name
        px="${spec%% *}"; name="${spec##* }"
        sips -z "$px" "$px" "$src" --out "$iconset/icon_${name}.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$dest"
}

# --- assemble --------------------------------------------------------------
log "Assembling $BUNDLE"
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "$STAGED_BINARY" "$BUNDLE/Contents/MacOS/$BIN_NAME"
chmod +x "$BUNDLE/Contents/MacOS/$BIN_NAME"

for name in "${COMPANION_NAMES[@]}"; do
    cp "$TMP_DIR/$name" "$BUNDLE/Contents/MacOS/$name"
    chmod +x "$BUNDLE/Contents/MacOS/$name"
done

BUILD_NUMBER="${BUILD_NUMBER:-$DISPLAY_VERSION}"
YEAR="$(date -u +%Y)"
{
    sed -e "s|@APP_NAME@|$(sed_escape "$APP_NAME")|g" \
        -e "s|@BIN_NAME@|$(sed_escape "$BIN_NAME")|g" \
        -e "s|@BUNDLE_ID@|$(sed_escape "$BUNDLE_ID")|g" \
        -e "s|@VERSION@|$(sed_escape "$DISPLAY_VERSION")|g" \
        -e "s|@BUILD_NUMBER@|$(sed_escape "$BUILD_NUMBER")|g" \
        -e "s|@YEAR@|$(sed_escape "$YEAR")|g" \
        "$SCRIPT_DIR/Info.plist"
} > "$BUNDLE/Contents/Info.plist"

if command -v plutil >/dev/null 2>&1; then
    plutil -lint "$BUNDLE/Contents/Info.plist" >/dev/null \
        || die "generated Info.plist is not a valid plist"
fi

printf 'APPL????' > "$BUNDLE/Contents/PkgInfo"

build_icns "$ICON_SRC" "$BUNDLE/Contents/Resources/AppIcon.icns"

# --- sign ------------------------------------------------------------------
# The app and each companion are separate Mach-O files. Sign the companions
# first, then the bundle, so the bundle seal covers already-signed code and
# notarization sees no unsigned executable under Contents/MacOS. --deep is not
# used, because Apple deprecates it for signing and it would hide any nested
# code that needs its own signature.
if [[ -n "$SIGN_IDENTITY" ]]; then
    need codesign
    log "Codesigning bundle with identity: $SIGN_IDENTITY"
    for name in "${COMPANION_NAMES[@]}"; do
        codesign --force --options runtime --timestamp \
            --sign "$SIGN_IDENTITY" "$BUNDLE/Contents/MacOS/$name"
    done
    codesign --force --options runtime --timestamp \
        --entitlements "$ENTITLEMENTS" \
        --sign "$SIGN_IDENTITY" "$BUNDLE"
    codesign --verify --strict --verbose=2 "$BUNDLE"
    log "Signature verified (hardened runtime, entitlements: $(basename "$ENTITLEMENTS"))"
else
    warn "APPLE_SIGNING_IDENTITY is not set: producing an UNSIGNED bundle."
    warn "Gatekeeper will refuse it on download and it cannot be notarized."
fi

log "Bundle ready: $BUNDLE"
if [[ -n "$ARCH_LABEL" ]]; then
    log "Architecture: $ARCH_LABEL"
fi
