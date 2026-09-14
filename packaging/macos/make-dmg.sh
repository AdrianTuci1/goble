#!/usr/bin/env bash
#
# Build a distributable macOS .dmg from an assembled .app bundle.
#
# Approach (no create-dmg, no third-party tools): create a writable HFS+ image
# with hdiutil, mount it, copy the .app and an /Applications symlink, ask Finder
# via osascript to record the window/icon layout into the volume's .DS_Store,
# detach, then convert to a compressed read-only UDZO image.
#
# The mount point is always detached by a trap, so a failure mid-layout never
# leaves a volume mounted.
#
# Signing / notarization:
#   APPLE_SIGNING_IDENTITY  sign the .app and the .dmg
#   APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD  notarize + staple the .dmg
# Missing secrets degrade to an unsigned image with an explicit warning.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGING_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$PACKAGING_DIR/.." && pwd)"

# shellcheck source=../version.env
source "$PACKAGING_DIR/version.env"
# shellcheck source=../signing.sh
source "$PACKAGING_DIR/signing.sh"

APP_NAME="${APP_NAME:-Goble}"
VERSION="${VERSION:-0.0.0}"

DEFAULT_BG="$SCRIPT_DIR/dmg-background.png"
WINDOW_W=660
WINDOW_H=400
APP_X=180
APP_Y=200
LINK_X=480
LINK_Y=200

usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Build a .dmg from an already-assembled ${APP_NAME}.app.

Options:
  --app PATH            .app bundle (default: <repo>/dist/${APP_NAME}.app)
  --outdir DIR          Output directory (default: <repo>/dist)
  --version VERSION     Version used in the file name (default: ${VERSION})
  --arch LABEL          arm64 | x64 | universal (default: host arch)
  --volume-name NAME    Mounted volume name (default: "${APP_NAME} <version>")
  --background PATH     Background PNG (default: ${DEFAULT_BG})
  --codesign-identity ID
                        codesign identity (default: \$APPLE_SIGNING_IDENTITY)
  --entitlements PATH   Entitlements plist (default: macos/entitlements.plist)
  --no-sign             Never sign or notarize, even if secrets are present
  --keep-rw             Keep the intermediate read/write image for debugging
  -h, --help            Show this help

Environment:
  APPLE_SIGNING_IDENTITY   Developer ID Application identity
  APPLE_ID                 Apple account for notarization
  APPLE_TEAM_ID            Team ID for notarization
  APPLE_APP_PASSWORD       App-specific password for notarization
EOF
}

log()  { printf '==> %s\n' "$*" >&2; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required tool '$1' not found in PATH"; }

APP_PATH=""
OUTDIR="$ROOT_DIR/dist"
DISPLAY_VERSION="$VERSION"
ARCH_LABEL=""
VOLUME_NAME=""
BACKGROUND="$DEFAULT_BG"
SIGN_IDENTITY="${APPLE_SIGNING_IDENTITY:-}"
ENTITLEMENTS="$SCRIPT_DIR/entitlements.plist"
DO_SIGN=1
KEEP_RW=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --app)               APP_PATH="${2:?--app needs a path}"; shift 2 ;;
        --outdir)            OUTDIR="${2:?--outdir needs a path}"; shift 2 ;;
        --version)           DISPLAY_VERSION="${2:?--version needs a value}"; shift 2 ;;
        --arch)              ARCH_LABEL="${2:?--arch needs a value}"; shift 2 ;;
        --volume-name)       VOLUME_NAME="${2:?--volume-name needs a value}"; shift 2 ;;
        --background)        BACKGROUND="${2:?--background needs a path}"; shift 2 ;;
        --codesign-identity) SIGN_IDENTITY="${2:?--codesign-identity needs a value}"; shift 2 ;;
        --entitlements)      ENTITLEMENTS="${2:?--entitlements needs a path}"; shift 2 ;;
        --no-sign)           DO_SIGN=0; shift ;;
        --keep-rw)           KEEP_RW=1; shift ;;
        -h|--help)           usage; exit 0 ;;
        *)                   die "unknown argument: $1 (try --help)" ;;
    esac
done

need hdiutil
[[ "$(uname -s)" == "Darwin" ]] || die "make-dmg.sh only runs on macOS"

# A signed-but-un-notarized DMG is still refused by Gatekeeper on first launch,
# so for a release both halves are required, and the check happens before any
# image is built. --no-sign is the deliberate way out.
if [[ "$DO_SIGN" -eq 0 && "${GOBLE_REQUIRE_SIGNING:-0}" == "1" ]]; then
    signing_die "--no-sign was passed while GOBLE_REQUIRE_SIGNING=1" \
        "A release DMG must be signed and notarized; refusing to build one" \
        "that users cannot open without a Gatekeeper override."
fi
if [[ "$DO_SIGN" -eq 1 ]]; then
    require_macos_signing "$SIGN_IDENTITY"
fi

APP_PATH="${APP_PATH:-$OUTDIR/$APP_NAME.app}"
[[ -d "$APP_PATH" ]] || die ".app bundle not found: $APP_PATH
run packaging/macos/make-bundle.sh first"
[[ -x "$APP_PATH/Contents/MacOS/$BIN_NAME" ]] \
    || die "$APP_PATH is missing Contents/MacOS/$BIN_NAME"

if [[ -z "$ARCH_LABEL" ]]; then
    # `lipo -archs` prints "arm64" or "x86_64 arm64" for a fat binary.
    ARCH_LABEL="$(lipo -archs "$APP_PATH/Contents/MacOS/$BIN_NAME" 2>/dev/null || true)"
    if [[ -z "$ARCH_LABEL" ]]; then
        ARCH_LABEL="$(uname -m)"
    elif [[ "$(printf '%s' "$ARCH_LABEL" | wc -w | tr -d ' ')" -gt 1 ]]; then
        ARCH_LABEL="universal"
    fi
fi
case "$ARCH_LABEL" in
    aarch64|arm64) SUFFIX="arm64" ;;
    x86_64|x64)    SUFFIX="x64" ;;
    universal)     SUFFIX="universal" ;;
    *)             SUFFIX="$ARCH_LABEL" ;;
esac

[[ -f "$BACKGROUND" ]] || die "background image not found: $BACKGROUND"

mkdir -p "$OUTDIR"
OUT_DMG="$OUTDIR/${APP_NAME}-${DISPLAY_VERSION}-${SUFFIX}.dmg"

# hdiutil refuses volume names longer than 27 characters.
if [[ -z "$VOLUME_NAME" ]]; then
    VOLUME_NAME="${APP_NAME} ${DISPLAY_VERSION}"
fi
VOLUME_NAME="${VOLUME_NAME:0:27}"

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goble-dmg.XXXXXX")"
RW_DMG="$TMP_DIR/rw.dmg"
MOUNT_POINT="$TMP_DIR/mnt"
MOUNTED=0

cleanup() {
    local status=$?
    if [[ "$MOUNTED" -eq 1 ]]; then
        hdiutil detach "$MOUNT_POINT" -force >/dev/null 2>&1 \
            || hdiutil detach "$VOLUME_NAME" -force >/dev/null 2>&1 \
            || true
    fi
    if [[ "$KEEP_RW" -eq 1 && -f "$RW_DMG" ]]; then
        local keep="$OUTDIR/$(basename "$RW_DMG")"
        mv -f "$RW_DMG" "$keep" 2>/dev/null || true
        log "Kept read/write image: $keep"
    fi
    rm -rf "$TMP_DIR"
    return "$status"
}
trap cleanup EXIT INT TERM

# --- sign the app before it is sealed into the image -----------------------
# Same treatment as make-bundle.sh: hardened runtime, explicit entitlements, no
# --deep. This runs even when the bundle was already signed, because the bundle
# gets copied and it costs nothing to seal it with the flags this script knows.
if [[ "$DO_SIGN" -eq 1 && -n "$SIGN_IDENTITY" ]]; then
    need codesign
    [[ -f "$ENTITLEMENTS" ]] || die "entitlements file not found: $ENTITLEMENTS"
    log "Codesigning ${APP_NAME}.app"
    codesign --force --options runtime --timestamp \
        --entitlements "$ENTITLEMENTS" \
        --sign "$SIGN_IDENTITY" "$APP_PATH"
    codesign --verify --strict --verbose=2 "$APP_PATH"
fi

# --- create and mount the read/write image ---------------------------------
# Size the image from the bundle plus a fixed allowance for the Finder metadata
# and the /Applications symlink.
APP_KB="$(du -sk "$APP_PATH" | awk '{print $1}')"
SIZE_KB=$(( APP_KB + 65536 ))
SIZE_MB=$(( (SIZE_KB + 1023) / 1024 ))

log "Creating ${SIZE_MB}MB read/write image (volume: $VOLUME_NAME)"
hdiutil create -quiet -size "${SIZE_MB}m" -fs HFS+ -volname "$VOLUME_NAME" \
    -type UDIF -ov "$RW_DMG"

log "Mounting image"
hdiutil attach "$RW_DMG" -mountpoint "$MOUNT_POINT" -nobrowse -noautoopen >/dev/null
MOUNTED=1

# --- populate ---------------------------------------------------------------
log "Copying ${APP_NAME}.app and /Applications symlink"
cp -R "$APP_PATH" "$MOUNT_POINT/"
ln -s /Applications "$MOUNT_POINT/Applications"

mkdir -p "$MOUNT_POINT/.background"
cp "$BACKGROUND" "$MOUNT_POINT/.background/dmg-background.png"
if command -v chflags >/dev/null 2>&1; then
    chflags hidden "$MOUNT_POINT/.background" 2>/dev/null || true
fi

# --- record the Finder window/icon layout into .DS_Store --------------------
# Finder automation can fail in headless sessions; the image is still valid
# without a stored layout, so treat this as best-effort and warn.
log "Setting Finder layout (window ${WINDOW_W}x${WINDOW_H}, app x=$APP_X, link x=$LINK_X)"
if command -v osascript >/dev/null 2>&1; then
    if ! osascript <<APPLESCRIPT >/dev/null 2>&1
tell application "Finder"
    tell disk "$VOLUME_NAME"
        open
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        set the bounds of container window to {400, 100, $((400 + WINDOW_W)), $((100 + WINDOW_H))}
        set viewOptions to the icon view options of container window
        set arrangement of viewOptions to not arranged
        set icon size of viewOptions to 128
        set background picture of viewOptions to file ".background:dmg-background.png"
        set position of item "${APP_NAME}.app" of container window to {$APP_X, $APP_Y}
        set position of item "Applications" of container window to {$LINK_X, $LINK_Y}
        close
        open
        update without registering applications
        delay 2
    end tell
end tell
APPLESCRIPT
    then
        warn "Finder layout could not be applied (no GUI session?); building DMG without stored icon positions."
    fi
else
    warn "osascript not available; DMG will use Finder's default layout."
fi

# Give Finder a moment to flush .DS_Store before unmounting.
sync
sleep 1

log "Unmounting image"
hdiutil detach "$MOUNT_POINT" >/dev/null
MOUNTED=0

# --- convert to compressed read-only ----------------------------------------
log "Converting to UDZO ($OUT_DMG)"
rm -f "$OUT_DMG"
hdiutil convert "$RW_DMG" -format UDZO -imagekey zlib-level=9 -o "$OUT_DMG" >/dev/null

# --- sign the image ---------------------------------------------------------
SIGNED=0
if [[ "$DO_SIGN" -eq 1 && -n "$SIGN_IDENTITY" ]]; then
    need codesign
    log "Codesigning DMG"
    codesign --force --sign "$SIGN_IDENTITY" --timestamp "$OUT_DMG"
    codesign --verify --verbose=2 "$OUT_DMG"
    SIGNED=1
fi

# --- notarize ---------------------------------------------------------------
if [[ "$SIGNED" -eq 1 && -n "${APPLE_ID:-}" && -n "${APPLE_TEAM_ID:-}" \
      && -n "${APPLE_APP_PASSWORD:-}" ]]; then
    need xcrun
    log "Submitting to Apple notary service (this can take several minutes)"
    xcrun notarytool submit "$OUT_DMG" \
        --apple-id "$APPLE_ID" \
        --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD" \
        --wait
    xcrun stapler staple "$OUT_DMG"
    xcrun stapler validate "$OUT_DMG"

    # The only check that answers the question a user cares about: would
    # Gatekeeper let this through? codesign and stapler both pass on artifacts
    # that Gatekeeper still refuses, so ask Gatekeeper itself.
    log "Assessing with Gatekeeper (spctl)"
    if ! spctl --assess --type install --verbose=4 "$OUT_DMG"; then
        die "Gatekeeper rejected $OUT_DMG even after notarization and stapling"
    fi
    if ! spctl --assess --type exec --verbose=4 "$APP_PATH"; then
        die "Gatekeeper rejected the signed app bundle $APP_PATH"
    fi
    log "Notarization, stapling and Gatekeeper assessment complete"
else
    warn "Shipping DMG UNSIGNED/UNNOTARIZED."
    if [[ "$DO_SIGN" -eq 0 ]]; then
        warn "  reason: --no-sign was passed."
    elif [[ -z "$SIGN_IDENTITY" ]]; then
        warn "  reason: APPLE_SIGNING_IDENTITY is not set."
    else
        warn "  reason: APPLE_ID / APPLE_TEAM_ID / APPLE_APP_PASSWORD not all set."
    fi
    warn "  macOS will show a Gatekeeper warning on first launch."
fi

log "DMG ready: $OUT_DMG"
