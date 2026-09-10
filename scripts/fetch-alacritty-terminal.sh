#!/usr/bin/env bash
# Fetch the published alacritty_terminal crate and check it against our vendored copy.
#
#   scripts/fetch-alacritty-terminal.sh                 verify vendor/ against the archive
#   scripts/fetch-alacritty-terminal.sh --install       restore vendor/ from the archive
#   scripts/fetch-alacritty-terminal.sh <destination>   leave the extracted crate there
#
# The archive is pinned by SHA-256. Verification downloads
# alacritty_terminal-0.26.0.crate, checks that digest, and diffs the extraction
# against vendor/alacritty_terminal, so a local edit to the vendored tree cannot
# go unnoticed. `--install` runs the same checks and then replaces the vendored
# tree, which is how a fresh clone (or CI) obtains it. A destination directory
# leaves the extracted crate untouched, which is how the upstream test suite can
# be run as a differential oracle:
#   cargo test --manifest-path <destination>/alacritty_terminal-0.26.0/Cargo.toml

set -euo pipefail

version="0.26.0"
sha256="bda177466b9524d59f1b12f0dd30b68696788e9992a7e959021c4a0ed96fcf59"
url="https://static.crates.io/crates/alacritty_terminal/alacritty_terminal-${version}.crate"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
vendored="${root}/vendor/alacritty_terminal"

mode="verify"
if [[ "${1:-}" == "--install" ]]; then
    mode="install"
    destination="$(mktemp -d)"
    trap 'rm -rf "${destination}"' EXIT
elif [[ -n "${1:-}" ]]; then
    mode="extract"
    destination="${1}"
else
    destination="$(mktemp -d)"
    trap 'rm -rf "${destination}"' EXIT
fi

# `shasum` ships with macOS and the Git-for-Windows shell, `sha256sum` with the
# Linux runners. Support both rather than making the script host-specific.
digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

archive="${destination}/alacritty_terminal-${version}.crate"

mkdir -p "${destination}"
echo "fetching ${url}"
curl -sSfL -o "${archive}" "${url}"

actual="$(digest "${archive}")"
if [[ "${actual}" != "${sha256}" ]]; then
    echo "checksum mismatch for ${archive}" >&2
    echo "  expected ${sha256}" >&2
    echo "  actual   ${actual}" >&2
    exit 1
fi
echo "checksum ok"

tar -xzf "${archive}" -C "${destination}"
extracted="${destination}/alacritty_terminal-${version}"

# Deliberate differences from the published archive: no Cargo.lock (we build it
# in our workspace) and no tests/ (46 MB of fixtures that never run here).
rm -f "${extracted}/Cargo.lock"
rm -rf "${extracted}/tests"

if [[ "${mode}" == "extract" ]]; then
    echo "extracted to ${extracted}"
    exit 0
fi

if [[ "${mode}" == "install" ]]; then
    rm -rf "${vendored}"
    mkdir -p "$(dirname "${vendored}")"
    mv "${extracted}" "${vendored}"
    echo "installed ${version} into ${vendored}"
    exit 0
fi

echo "comparing ${extracted} with ${vendored}"
if diff -r "${extracted}" "${vendored}"; then
    echo "vendored source is identical to the published crate"
else
    echo "vendored source differs from the published crate (see above)" >&2
    exit 1
fi
