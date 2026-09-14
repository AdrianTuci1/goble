#!/usr/bin/env bash
#
# Signing policy shared by the packaging scripts. Sourced, not executed.
#
# A release is signed or it is not a release. An unsigned macOS download is
# refused by Gatekeeper, an unsigned Windows installer is flagged by SmartScreen,
# and neither can be checked by the person downloading it. Until now a missing
# credential produced a warning and an unsigned artifact, which is how an
# "official" build ships broken by accident.
#
#   GOBLE_REQUIRE_SIGNING=1   a missing credential is a build failure
#
# scripts/release.sh sets this for every channel except `dev`, where an unsigned
# artifact is the point, and clears it under --allow-unsigned.

# True when this build must be signed.
signing_is_required() {
    [[ "${GOBLE_REQUIRE_SIGNING:-0}" == "1" ]]
}

# Fail-fast message: what is missing, why it matters, and how to proceed
# deliberately rather than by accident.
signing_die() {
    printf 'error: %s\n' "$1" >&2
    shift
    local line
    for line in "$@"; do
        printf '       %s\n' "$line" >&2
    done
    printf '       Set the credential(s) above, or build an unsigned local artifact\n' >&2
    printf '       on purpose with --allow-unsigned (scripts/release.sh) or\n' >&2
    printf '       GOBLE_REQUIRE_SIGNING=0 for a packaging script run directly.\n' >&2
    exit 1
}

# macOS: a Developer ID identity, and for a release also the notary credentials.
require_macos_signing() {
    local identity="$1"
    signing_is_required || return 0

    if [[ -z "$identity" ]]; then
        signing_die "GOBLE_REQUIRE_SIGNING=1 but APPLE_SIGNING_IDENTITY is empty" \
            "macOS refuses an unsigned app on download, so the DMG would be" \
            "unusable for anyone who receives it." \
            "Use a 'Developer ID Application' certificate from the Apple" \
            "Developer Program. See packaging/README.md, section Signing."
    fi

    if [[ -z "${APPLE_ID:-}" || -z "${APPLE_TEAM_ID:-}" || -z "${APPLE_APP_PASSWORD:-}" ]]; then
        local missing=()
        [[ -n "${APPLE_ID:-}" ]] || missing+=(APPLE_ID)
        [[ -n "${APPLE_TEAM_ID:-}" ]] || missing+=(APPLE_TEAM_ID)
        [[ -n "${APPLE_APP_PASSWORD:-}" ]] || missing+=(APPLE_APP_PASSWORD)
        signing_die "GOBLE_REQUIRE_SIGNING=1 but notarization is not configured (missing: ${missing[*]})" \
            "A signed but un-notarized app still trips Gatekeeper on first" \
            "launch, so signing without notarizing does not make the download" \
            "work for a user." \
            "APPLE_APP_PASSWORD is an app-specific password from appleid.apple.com."
    fi
}
