#!/usr/bin/env bash
# Format (or check) the Goble workspace.
#
# `cargo fmt --all` also formats local path-based dependencies, which drags in
# `vendor/alacritty_terminal`: third-party source kept byte-identical to its
# published archive (see THIRD-PARTY-NOTICES.md, and
# `scripts/fetch-alacritty-terminal.sh`, which diffs the tree against that
# archive). Running rustfmt over it would silently break that provenance, so
# this script names the workspace members explicitly and never walks vendor/.
#
# Usage:
#   scripts/fmt.sh            # format in place
#   scripts/fmt.sh --check    # verify formatting, exit non-zero on a diff

set -euo pipefail

cd "$(dirname "$0")/.."

manifests=(app/Cargo.toml)
while IFS= read -r -d '' manifest; do
  manifests+=("$manifest")
done < <(find crates -mindepth 2 -maxdepth 2 -name Cargo.toml -print0 | sort -z)

for manifest in "${manifests[@]}"; do
  cargo fmt --manifest-path "$manifest" -- "$@"
done
