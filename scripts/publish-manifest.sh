#!/usr/bin/env bash
#
# Publish a signed channel manifest to the downloads worker.
#
# The worker stores exactly one document per channel in KV and serves it at
# `GET /channel_versions.json`, so this POST is what makes a release
# installable. It refuses a document that is not schema 2, has no signature, or
# names more than one channel, and it reports the worker's own refusal (a stale
# epoch answers 409) instead of pretending the publish happened.
#
# Usage:
#   GOBLE_WORKER_URL=https://downloads.goble.dev \
#   GOBLE_WORKER_ADMIN_TOKEN=… \
#   scripts/publish-manifest.sh --manifest dist/channel_versions.json
#
# Options:
#   --manifest PATH   Manifest to publish (default: dist/channel_versions.json)
#   --channel NAME    Channel to publish; by default the one the document names
#   --dry-run         Print the request without sending it
#
# Environment:
#   GOBLE_WORKER_URL          base URL of the worker (no trailing slash needed)
#   GOBLE_WORKER_ADMIN_TOKEN  bearer token for /admin/publish
#
# With neither variable set the script exits 0 and says publishing is off: the
# release workflow calls it unconditionally, and a fork without a worker must
# still be able to build a release.

set -euo pipefail

MANIFEST="dist/channel_versions.json"
CHANNEL=""
DRY_RUN=0

log() { printf '==> %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required tool '$1' not found in PATH"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest) MANIFEST="${2:?--manifest needs a path}"; shift 2 ;;
        --channel)  CHANNEL="${2:?--channel needs a name}"; shift 2 ;;
        --dry-run)  DRY_RUN=1; shift ;;
        -h|--help)  sed -n '3,30p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *)          die "unknown argument: $1 (try --help)" ;;
    esac
done

WORKER_URL="${GOBLE_WORKER_URL:-}"
ADMIN_TOKEN="${GOBLE_WORKER_ADMIN_TOKEN:-}"

if [[ -z "$WORKER_URL" || -z "$ADMIN_TOKEN" ]]; then
    log "GOBLE_WORKER_URL or GOBLE_WORKER_ADMIN_TOKEN is not set: nothing was published."
    log "The manifest is still attached to the release; publish it by hand with this script."
    exit 0
fi

[[ -f "$MANIFEST" ]] || die "manifest not found: $MANIFEST"
need python3

WORKER_URL="${WORKER_URL%/}"
if [[ ! "$WORKER_URL" =~ ^https?:// ]]; then
    die "GOBLE_WORKER_URL must be an http(s) URL, got '$WORKER_URL'"
fi

# The body is { "channel": …, "manifest": "<json string>" }: the worker stores
# the document verbatim, so the signature it verifies is the one that was made.
BODY="$(python3 - "$MANIFEST" "$CHANNEL" <<'PY'
import json
import sys

path, channel = sys.argv[1], sys.argv[2]
with open(path, "r", encoding="utf-8") as fh:
    document = json.load(fh)

schema = document.get("schema")
if schema != 2:
    sys.stderr.write("error: %s is schema %r; the worker accepts schema 2 only\n" % (path, schema))
    raise SystemExit(2)

channels = document.get("channels")
if not isinstance(channels, dict) or not channels:
    sys.stderr.write("error: %s has no channels\n" % path)
    raise SystemExit(2)

if not channel:
    if len(channels) != 1:
        sys.stderr.write(
            "error: %s names %d channels (%s); pass --channel\n"
            % (path, len(channels), ", ".join(sorted(channels)))
        )
        raise SystemExit(2)
    channel = next(iter(channels))

release = channels.get(channel)
if not isinstance(release, dict):
    sys.stderr.write("error: %s has no '%s' channel\n" % (path, channel))
    raise SystemExit(2)
signature = release.get("signature")
if not isinstance(signature, str) or not signature.strip():
    sys.stderr.write("error: channel '%s' in %s is not signed\n" % (channel, path))
    raise SystemExit(2)
epoch = release.get("epoch")
if not isinstance(epoch, int) or isinstance(epoch, bool) or epoch < 0:
    sys.stderr.write("error: channel '%s' has no valid epoch\n" % channel)
    raise SystemExit(2)

body = {"channel": channel, "manifest": json.dumps(document, separators=(",", ":"))}
sys.stderr.write("==> Publishing channel '%s' (epoch %d)\n" % (channel, epoch))
json.dump(body, sys.stdout)
PY
)"

if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '%s\n' "$BODY" >&2
    log "Dry run: nothing was posted to $WORKER_URL/admin/publish"
    exit 0
fi

need curl

# The token goes in a header from the environment, never in a URL or in the
# body, and the manifest never contains it.
resp_file="$(mktemp)"
trap 'rm -f "$resp_file"' EXIT
status="$(
    printf '%s' "$BODY" | curl --silent --show-error \
        --output "$resp_file" --write-out '%{http_code}' \
        --header 'content-type: application/json' \
        --header "authorization: Bearer ${ADMIN_TOKEN}" \
        --data-binary @- \
        "${WORKER_URL}/admin/publish"
)"

cat "$resp_file" >&2
printf '\n' >&2

case "$status" in
    200) log "Published to ${WORKER_URL}" ;;
    409) die "the worker refused the manifest: its stored epoch is newer (HTTP 409). Publish with a higher GOBLE_RELEASE_EPOCH." ;;
    *)   die "the worker answered HTTP ${status}" ;;
esac
