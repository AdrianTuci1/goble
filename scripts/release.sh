#!/usr/bin/env bash
#
# Goble release driver for the native desktop app.
#
#   version resolution:  --version > $GOBLE_VERSION > `git describe --tags`
#                        > packaging/version.env VERSION
#   build:               cargo build --release -p goble-app
#   package:             the packaging script for the host OS
#   outputs:             dist/<installers>, dist/SHA256SUMS, and optionally
#                        dist/channel_versions.json for the updater
#
# This script never touches Tauri, npm or the legacy crates/goble-desktop tree:
# the product shell is `app/` (package goble-app, binary goble-app).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging"

# shellcheck source=../packaging/version.env
source "$PKG_DIR/version.env"

APP_NAME="${APP_NAME:-Goble}"
PKG_NAME="${PKG_NAME:-goble-app}"
BIN_NAME="${BIN_NAME:-goble-app}"
FALLBACK_VERSION="${VERSION:-0.0.0}"

usage() {
    cat <<EOF
Usage: scripts/release.sh [options]

Build and package $APP_NAME for the host platform, then write checksums and
(optionally) the updater manifest.

Options:
  --version VERSION     Override the release version
  --channel NAME        Manifest channel name (default: stable)
  --manifest            Also write dist/channel_versions.json
  --require-signing     Refuse to produce an unsigned release (default for every
                        channel except dev; also settable with
                        GOBLE_REQUIRE_SIGNING=1)
  --allow-unsigned      Build unsigned on purpose, for a local test release
  --dry-run             Print the plan without building or packaging
  --skip-build          Reuse an existing target/release/$BIN_NAME
  --skip-package        Do not run a packaging script; only (re)write
                        checksums and, with --manifest, the manifest. Used by
                        CI on a directory of artifacts from other runners.
  --outdir DIR          Output directory (default: <repo>/dist)
  --download-base URL   Base URL for manifest artifact links
                        (default: \$GOBLE_DOWNLOAD_BASE or the GitHub release tag)
  --universal           macOS only: build and bundle a universal (x86_64+arm64) app
  --linux-format FMT    Linux only; repeatable: appimage | deb | tarball | all
                        (default: appimage)
  -h, --help            Show this help

Environment:
  GOBLE_VERSION                version, used when --version is absent
  GOBLE_DOWNLOAD_BASE          base URL for the manifest
  GOBLE_UPDATE_SIGNING_KEY     hex Ed25519 seed; signs the manifest channel
  GOBLE_REQUIRE_SIGNING        1 = a missing signing credential fails the build
  APPLE_SIGNING_IDENTITY       macOS codesign identity
  APPLE_ID, APPLE_TEAM_ID, APPLE_APP_PASSWORD   macOS notarization
  WINDOWS_SIGN_CERT_PFX, WINDOWS_SIGN_CERT_PASSWORD,
  WINDOWS_SIGN_CERT_THUMBPRINT, WINDOWS_SIGN_TIMESTAMP_URL   Windows signing

Examples:
  scripts/release.sh --version 0.2.0 --manifest
  GOBLE_UPDATE_SIGNING_KEY=\$(cat key.hex) scripts/release.sh --manifest
  scripts/release.sh --dry-run
  scripts/release.sh --allow-unsigned   # local test build, never a release
EOF
}

log()  { printf '==> %s\n' "$*" >&2; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required tool '$1' not found in PATH"; }

ARG_VERSION=""
CHANNEL="stable"
WRITE_MANIFEST=0
DRY_RUN=0
SKIP_BUILD=0
SKIP_PACKAGE=0
SIGNING_MODE=""   # "", "require" or "allow"
OUTDIR="$ROOT_DIR/dist"
DOWNLOAD_BASE="${GOBLE_DOWNLOAD_BASE:-}"
UNIVERSAL=0
LINUX_FORMATS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)       ARG_VERSION="${2:?--version needs a value}"; shift 2 ;;
        --channel)       CHANNEL="${2:?--channel needs a name}"; shift 2 ;;
        --manifest)      WRITE_MANIFEST=1; shift ;;
        --require-signing) SIGNING_MODE="require"; shift ;;
        --allow-unsigned)  SIGNING_MODE="allow"; shift ;;
        --dry-run)       DRY_RUN=1; shift ;;
        --skip-build)    SKIP_BUILD=1; shift ;;
        --skip-package)  SKIP_PACKAGE=1; shift ;;
        --outdir)        OUTDIR="${2:?--outdir needs a path}"; shift 2 ;;
        --download-base) DOWNLOAD_BASE="${2:?--download-base needs a URL}"; shift 2 ;;
        --universal)     UNIVERSAL=1; shift ;;
        --linux-format)
            case "${2:?--linux-format needs a value}" in
                all) LINUX_FORMATS=(appimage deb tarball) ;;
                appimage|deb|tarball) LINUX_FORMATS+=("$2") ;;
                *) die "--linux-format must be appimage, deb, tarball or all" ;;
            esac
            shift 2 ;;
        -h|--help)       usage; exit 0 ;;
        *)               die "unknown argument: $1 (try --help)" ;;
    esac
done

if [[ ${#LINUX_FORMATS[@]} -eq 0 ]]; then
    LINUX_FORMATS=(appimage)
fi

# --- version ---------------------------------------------------------------
resolve_version() {
    if [[ -n "$ARG_VERSION" ]]; then printf '%s\n' "$ARG_VERSION"; return; fi
    if [[ -n "${GOBLE_VERSION:-}" ]]; then printf '%s\n' "$GOBLE_VERSION"; return; fi

    local tag=""
    if command -v git >/dev/null 2>&1; then
        tag="$(git -C "$ROOT_DIR" describe --tags --abbrev=0 2>/dev/null || true)"
    fi
    if [[ -n "$tag" ]]; then
        printf '%s\n' "${tag#v}"
        return
    fi

    printf '%s\n' "$FALLBACK_VERSION"
}
VERSION="$(resolve_version)"
[[ -n "$VERSION" ]] || die "could not resolve a version"

# --- host ------------------------------------------------------------------
HOST_OS="$(uname -s)"
case "$HOST_OS" in
    Darwin) PLATFORM="macos" ;;
    Linux)  PLATFORM="linux" ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT) PLATFORM="windows" ;;
    *)      PLATFORM="" ;;
esac

if [[ -z "$PLATFORM" ]]; then
    die "no packaging path for host OS '$HOST_OS'.
Supported hosts: macOS (uname Darwin), Linux, Windows (MSYS/MinGW/Cygwin).
Cross-compilation is not driven from this script; use the CI matrix in
.github/workflows/release.yml instead."
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
if [[ "$PLATFORM" == "windows" ]]; then
    HOST_BINARY="$TARGET_DIR/release/$BIN_NAME.exe"
else
    HOST_BINARY="$TARGET_DIR/release/$BIN_NAME"
fi

# --- signing policy ---------------------------------------------------------
# A release is signed, or it is not a release: Gatekeeper refuses an unsigned
# macOS download and SmartScreen flags an unsigned Windows installer. So signing
# is required by default for every channel a user can be on, and `dev` — which
# by definition means a developer's own machine — is the one that may be
# unsigned. `--allow-unsigned` is the deliberate way out for a local test build.
resolve_signing_requirement() {
    case "$SIGNING_MODE" in
        require) REQUIRE_SIGNING=1; return ;;
        allow)   REQUIRE_SIGNING=0; return ;;
    esac
    if [[ -n "${GOBLE_REQUIRE_SIGNING:-}" ]]; then
        [[ "$GOBLE_REQUIRE_SIGNING" == "1" ]] && REQUIRE_SIGNING=1 || REQUIRE_SIGNING=0
        return
    fi
    [[ "$CHANNEL" == "dev" ]] && REQUIRE_SIGNING=0 || REQUIRE_SIGNING=1
}
REQUIRE_SIGNING=0
resolve_signing_requirement

# Check before building anything: discovering a missing certificate after a
# ten-minute release build helps nobody.
check_signing_requirements() {
    [[ "$REQUIRE_SIGNING" -eq 1 ]] || return 0

    local missing=()

    # --skip-package writes checksums (and the manifest) over artifacts another
    # runner built, so the platform signing credentials belong to that runner.
    # The manifest key still applies here, because this step is what signs it.
    if [[ "$SKIP_PACKAGE" -eq 0 ]]; then
        case "$PLATFORM" in
            macos)
                [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]] || missing+=("APPLE_SIGNING_IDENTITY (Developer ID Application certificate)")
                [[ -n "${APPLE_ID:-}" ]] || missing+=("APPLE_ID")
                [[ -n "${APPLE_TEAM_ID:-}" ]] || missing+=("APPLE_TEAM_ID")
                [[ -n "${APPLE_APP_PASSWORD:-}" ]] || missing+=("APPLE_APP_PASSWORD (app-specific password)")
                ;;
            windows)
                if [[ -n "${WINDOWS_SIGN_CERT_THUMBPRINT:-}" ]]; then
                    :
                elif [[ -n "${WINDOWS_SIGN_CERT_PFX:-}" && -n "${WINDOWS_SIGN_CERT_PASSWORD:-}" ]]; then
                    [[ -f "${WINDOWS_SIGN_CERT_PFX}" ]] || die "WINDOWS_SIGN_CERT_PFX points at a missing file: $WINDOWS_SIGN_CERT_PFX"
                else
                    missing+=("WINDOWS_SIGN_CERT_PFX + WINDOWS_SIGN_CERT_PASSWORD, or WINDOWS_SIGN_CERT_THUMBPRINT")
                fi
                ;;
        esac
    fi

    # Linux has no artifact signature of its own: the ed25519 signature over the
    # update manifest is what authenticates a Linux download, so it is required
    # on every platform whenever a manifest is written.
    if [[ "$WRITE_MANIFEST" -eq 1 && -z "${GOBLE_UPDATE_SIGNING_KEY:-}" ]]; then
        missing+=("GOBLE_UPDATE_SIGNING_KEY (signs the updater manifest; unauthenticated updates on every platform)")
    fi

    if [[ ${#missing[@]} -gt 0 ]]; then
        printf 'error: channel %s must be signed, but the following are not set:\n' "$CHANNEL" >&2
        local item
        for item in "${missing[@]}"; do
            printf '       - %s\n' "$item" >&2
        done
        printf '       See packaging/README.md, section Signing, for how to obtain each one.\n' >&2
        printf '       To build an unsigned artifact on purpose (a test build, never a\n' >&2
        printf '       release), rerun with --allow-unsigned.\n' >&2
        if [[ "$DRY_RUN" -eq 1 ]]; then
            warn "dry run: reporting the missing credentials instead of failing"
            return 0
        fi
        exit 1
    fi
}

check_signing_requirements
if [[ "$REQUIRE_SIGNING" -eq 1 ]]; then
    export GOBLE_REQUIRE_SIGNING=1
    log "Signing: required (channel $CHANNEL; --allow-unsigned to override)"
else
    unset GOBLE_REQUIRE_SIGNING
    if [[ "$CHANNEL" == "dev" ]]; then
        log "Signing: not required (channel dev)"
    else
        log "Signing: not required (--allow-unsigned): this is NOT a release artifact"
    fi
fi

mkdir -p "$OUTDIR"
OUTDIR="$(cd "$OUTDIR" && pwd)"

log "Version:  $VERSION"
log "Host:     $HOST_OS ($PLATFORM)"
log "Output:   $OUTDIR"
if [[ "$WRITE_MANIFEST" -eq 1 ]]; then
    log "Manifest: dist/channel_versions.json (channel: $CHANNEL)"
fi

run() {
    if [[ "$DRY_RUN" -eq 1 ]]; then
        printf '[dry-run] %s\n' "$*" >&2
        return 0
    fi
    "$@"
}

# --- build -----------------------------------------------------------------
build_binary() {
    if [[ "$SKIP_BUILD" -eq 1 ]]; then
        log "Skipping cargo build (--skip-build)"
        [[ -f "$HOST_BINARY" ]] || warn "expected binary not found: $HOST_BINARY"
        return 0
    fi
    need cargo
    log "Building $PKG_NAME (release, channel: $CHANNEL, version: $VERSION)"
    # GOBLE_CHANNEL and GOBLE_RELEASE_VERSION are baked in with `option_env!`: a
    # released build reports crashes, accepts updates, and knows its own version;
    # a developer's build does none of that and claims no version it is not.
    run env GOBLE_CHANNEL="$CHANNEL" GOBLE_RELEASE_VERSION="$VERSION" \
        cargo build --release -p "$PKG_NAME"
}

# --- packaging -------------------------------------------------------------
package_macos() {
    local bundle_args=(--binary "$HOST_BINARY" --outdir "$OUTDIR" --version "$VERSION")
    if [[ "$UNIVERSAL" -eq 1 ]]; then
        log "Universal build requested: compiling both macOS slices"
        run cargo build --release -p "$PKG_NAME" --target aarch64-apple-darwin
        run cargo build --release -p "$PKG_NAME" --target x86_64-apple-darwin
        bundle_args+=(--universal
            --aarch64-binary "$TARGET_DIR/aarch64-apple-darwin/release/$BIN_NAME"
            --x86_64-binary "$TARGET_DIR/x86_64-apple-darwin/release/$BIN_NAME")
    fi
    run "$PKG_DIR/macos/make-bundle.sh" "${bundle_args[@]}"
    run "$PKG_DIR/macos/make-dmg.sh" \
        --app "$OUTDIR/$APP_NAME.app" --outdir "$OUTDIR" --version "$VERSION"
}

package_linux() {
    local format
    for format in "${LINUX_FORMATS[@]}"; do
        case "$format" in
            appimage) run "$PKG_DIR/linux/make-appimage.sh" \
                          --binary "$HOST_BINARY" --outdir "$OUTDIR" --version "$VERSION" ;;
            deb)      run "$PKG_DIR/linux/make-deb.sh" \
                          --binary "$HOST_BINARY" --outdir "$OUTDIR" --version "$VERSION" ;;
            tarball)  run "$PKG_DIR/linux/make-tarball.sh" \
                          --binary "$HOST_BINARY" --outdir "$OUTDIR" --version "$VERSION" ;;
        esac
    done
}

package_windows() {
    local ps
    ps="$(command -v pwsh || command -v powershell || true)"
    [[ -n "$ps" ]] || die "PowerShell (pwsh or powershell) not found; run packaging/windows/build-installer.ps1 directly"
    run "$ps" -NoProfile -ExecutionPolicy Bypass -File "$PKG_DIR/windows/build-installer.ps1" \
        -Version "$VERSION" -SourceBinary "$HOST_BINARY" -OutDir "$OUTDIR"
}

build_binary
if [[ "$SKIP_PACKAGE" -eq 1 ]]; then
    log "Skipping packaging (--skip-package)"
else
    case "$PLATFORM" in
        macos)   package_macos ;;
        linux)   package_linux ;;
        windows) package_windows ;;
    esac
fi

# --- sha256 -----------------------------------------------------------------
hash_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

# Release artifacts only: skip the checksum file itself, signature sidecars and
# the generated manifest, which are metadata rather than downloads.
list_artifacts() {
    find "$OUTDIR" -maxdepth 1 -type f \
        ! -name 'SHA256SUMS' \
        ! -name '*.sig' \
        ! -name 'channel_versions.json' \
        -print | sort
}

write_sha256sums() {
    local sums="$OUTDIR/SHA256SUMS"
    : > "$sums"
    local artifact
    while IFS= read -r artifact; do
        printf '%s  %s\n' "$(hash_file "$artifact")" "$(basename "$artifact")" >> "$sums"
    done < <(list_artifacts)
    log "Wrote $sums"
}

# --- updater manifest -------------------------------------------------------
# Maps artifact file names to the os/arch pairs the updater understands. The
# first matching pattern per (os, arch) wins, so a machine that produced both
# an AppImage and a .deb advertises the AppImage.
pick_artifact() {
    local pattern candidate
    # Globbing with nullglob avoids relying on `find -quit`, which is not
    # portable to the BSD find shipped with macOS.
    shopt -s nullglob
    for pattern in "$@"; do
        for candidate in "$OUTDIR"/$pattern; do
            if [[ -f "$candidate" ]]; then
                basename "$candidate"
                shopt -u nullglob
                return 0
            fi
        done
    done
    shopt -u nullglob
    return 0
}

json_escape() { printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'; }

write_manifest() {
    [[ -n "$DOWNLOAD_BASE" ]] || DOWNLOAD_BASE="${RELEASE_BASE_URL}/v${VERSION}"
    DOWNLOAD_BASE="${DOWNLOAD_BASE%/}"

    local entries=()
    local os arch file
    while IFS='|' read -r os arch; do
        [[ -n "$os" ]] || continue
        # shellcheck disable=SC2086
        case "$os:$arch" in
            macos:aarch64) file="$(pick_artifact "Goble-${VERSION}-arm64.dmg" "Goble-${VERSION}-universal.dmg")" ;;
            macos:x86_64)  file="$(pick_artifact "Goble-${VERSION}-x64.dmg" "Goble-${VERSION}-universal.dmg")" ;;
            linux:x86_64)  file="$(pick_artifact "Goble-${VERSION}-x86_64.AppImage" "goble_${VERSION}_amd64.deb" "goble-${VERSION}-linux-x86_64.tar.gz")" ;;
            linux:aarch64) file="$(pick_artifact "Goble-${VERSION}-aarch64.AppImage" "goble_${VERSION}_arm64.deb" "goble-${VERSION}-linux-aarch64.tar.gz")" ;;
            windows:x86_64)  file="$(pick_artifact "Goble-${VERSION}-win-x64-setup.exe")" ;;
            windows:aarch64) file="$(pick_artifact "Goble-${VERSION}-win-arm64-setup.exe")" ;;
        esac
        [[ -n "$file" ]] || continue
        local path="$OUTDIR/$file"
        entries+=("        {\"os\": \"$os\", \"arch\": \"$arch\", \"url\": \"$(json_escape "$DOWNLOAD_BASE/$file")\", \"sha256\": \"$(hash_file "$path")\", \"size\": $(wc -c < "$path" | tr -d ' ')}")
    done <<'PAIRS'
macos|aarch64
macos|x86_64
linux|x86_64
linux|aarch64
windows|x86_64
windows|aarch64
PAIRS

    if [[ ${#entries[@]} -eq 0 ]]; then
        warn "no recognized artifacts found in $OUTDIR; writing an empty artifact list"
    fi

    local manifest="$OUTDIR/channel_versions.json"
    {
        echo '{'
        echo '  "schema": 1,'
        echo '  "channels": {'
        echo "    \"$(json_escape "$CHANNEL")\": {"
        echo "      \"version\": \"$(json_escape "$VERSION")\","
        echo '      "artifacts": ['
        if [[ ${#entries[@]} -gt 0 ]]; then
            local i
            for i in "${!entries[@]}"; do
                if [[ "$i" -lt $(( ${#entries[@]} - 1 )) ]]; then
                    printf '%s,\n' "${entries[$i]}"
                else
                    printf '%s\n' "${entries[$i]}"
                fi
            done
        fi
        echo '      ]'
        echo '    }'
        echo '  }'
        echo '}'
    } > "$manifest"
    log "Wrote $manifest"

    if [[ -n "${GOBLE_UPDATE_SIGNING_KEY:-}" ]]; then
        log "Signing manifest channel '$CHANNEL'"
        GOBLE_UPDATE_SIGNING_KEY="$GOBLE_UPDATE_SIGNING_KEY" \
            "$PKG_DIR/sign-manifest.sh" --manifest "$manifest" \
            --channel "$CHANNEL" --inplace >/dev/null
    else
        warn "GOBLE_UPDATE_SIGNING_KEY is not set: shipping an UNSIGNED manifest."
    fi
}

if [[ "$DRY_RUN" -eq 1 ]]; then
    log "Dry run: no build, no packaging, no checksums written."
    exit 0
fi

write_sha256sums

if [[ "$WRITE_MANIFEST" -eq 1 ]]; then
    write_manifest
fi

log "Release complete. Artifacts in $OUTDIR:"
ls -lh "$OUTDIR" >&2
