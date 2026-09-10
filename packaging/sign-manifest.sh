#!/usr/bin/env bash
#
# Sign a release channel object inside dist/channel_versions.json.
#
# The updater verifies the signature over a fixed text payload, not over JSON:
# JSON has many spellings for the same value, and a signature over a
# serialisation is a signature over whoever wrote the serialiser. The payload is
# defined once, in `goble_update::ChannelRelease::signing_payload`:
#
#   goble-update-v1
#   <version>
#   <os> <arch> <sha256> <size> <url>      # one line per artifact, sorted
#
# with every artifact line lowercased for os/arch/sha256, and the artifact lines
# sorted as plain strings. `crates/goble-update/tests/signer_contract.rs` runs
# this script and verifies its output with the Rust verifier, so the two cannot
# drift apart silently.
#
# The "signature" key is never part of the signed payload. The signature is
# computed first and only then inserted (with --inplace).
#
# Key: $GOBLE_UPDATE_SIGNING_KEY, a hex-encoded 32-byte Ed25519 seed (64 hex
# characters). A 64-byte value (seed || public key) is also accepted; only the
# first 32 bytes are used. Ed25519 is implemented inline with hashlib only, so
# no third-party Python packages are required.
#
# Usage:
#   GOBLE_UPDATE_SIGNING_KEY=<hex> packaging/sign-manifest.sh --manifest dist/channel_versions.json
#
# Exit status is non-zero if the key is missing or malformed: this script is
# only ever called when the caller has decided to sign.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat <<'EOF'
Usage: sign-manifest.sh [options]

Sign one channel object from a channel_versions.json manifest.

Options:
  --manifest PATH   Manifest to read (default: dist/channel_versions.json)
  --channel NAME    Channel to sign (default: stable)
  --out PATH        Write the hex signature here
                    (default: <manifest>.<channel>.sig)
  --inplace         Also insert "signature" into the channel object in the
                    manifest file
  --print-pubkey    Print the derived public key (hex) and exit
  -h, --help        Show this help

Environment:
  GOBLE_UPDATE_SIGNING_KEY   hex Ed25519 seed (32 bytes / 64 hex chars)
EOF
}

log()  { printf '==> %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required tool '$1' not found in PATH"; }

MANIFEST="dist/channel_versions.json"
CHANNEL="stable"
OUT=""
INPLACE=0
PRINT_PUBKEY=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest)     MANIFEST="${2:?--manifest needs a path}"; shift 2 ;;
        --channel)      CHANNEL="${2:?--channel needs a name}"; shift 2 ;;
        --out)          OUT="${2:?--out needs a path}"; shift 2 ;;
        --inplace)      INPLACE=1; shift ;;
        --print-pubkey) PRINT_PUBKEY=1; shift ;;
        -h|--help)      usage; exit 0 ;;
        *)              die "unknown argument: $1 (try --help)" ;;
    esac
done

need python3

if [[ -z "${GOBLE_UPDATE_SIGNING_KEY:-}" && "$PRINT_PUBKEY" -eq 0 ]]; then
    die "GOBLE_UPDATE_SIGNING_KEY is not set; refusing to write a fake signature"
fi
if [[ "$PRINT_PUBKEY" -eq 0 && ! -f "$MANIFEST" ]]; then
    die "manifest not found: $MANIFEST"
fi
if [[ -z "$OUT" ]]; then
    OUT="${MANIFEST}.${CHANNEL}.sig"
fi

GOBLE_UPDATE_SIGNING_KEY="${GOBLE_UPDATE_SIGNING_KEY:-}" \
python3 - "$MANIFEST" "$CHANNEL" "$OUT" "$INPLACE" "$PRINT_PUBKEY" <<'PY'
import hashlib
import json
import os
import sys

# --- minimal Ed25519 (RFC 8032), reference arithmetic ----------------------
Q = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493


def sha512(data):
    return hashlib.sha512(data).digest()


def inv(x):
    return pow(x, Q - 2, Q)


D = (-121665 * inv(121666)) % Q
I = pow(2, (Q - 1) // 4, Q)


def xrecover(y):
    xx = (y * y - 1) * inv(D * y * y + 1)
    x = pow(xx, (Q + 3) // 8, Q)
    if (x * x - xx) % Q != 0:
        x = (x * I) % Q
    if x % 2 != 0:
        x = Q - x
    return x


BY = (4 * inv(5)) % Q
BX = xrecover(BY)
B = (BX % Q, BY % Q)


def edwards(p, q):
    x1, y1 = p
    x2, y2 = q
    x3 = (x1 * y2 + x2 * y1) * inv(1 + D * x1 * x2 * y1 * y2)
    y3 = (y1 * y2 + x1 * x2) * inv(1 - D * x1 * x2 * y1 * y2)
    return (x3 % Q, y3 % Q)


def scalarmult(p, e):
    result = (0, 1)
    while e > 0:
        if e & 1:
            result = edwards(result, p)
        p = edwards(p, p)
        e >>= 1
    return result


def encodepoint(p):
    x, y = p
    # Little-endian y in bits 0..254, sign bit of x in bit 255 (RFC 8032 5.1.2).
    bits = [(y >> i) & 1 for i in range(255)] + [x & 1]
    return bytes(
        sum(bits[i * 8 + k] << k for k in range(8)) for i in range(32)
    )


def secret_expand(seed):
    h = sha512(seed)
    a = 2**254 + sum(2**i * ((h[i // 8] >> (i % 8)) & 1) for i in range(3, 254))
    return a, h[32:64]


def publickey(seed):
    a, _ = secret_expand(seed)
    return encodepoint(scalarmult(B, a))


def sign(message, seed):
    a, prefix = secret_expand(seed)
    pk = publickey(seed)
    r = int.from_bytes(sha512(prefix + message), "little") % L
    big_r = encodepoint(scalarmult(B, r))
    k = int.from_bytes(sha512(big_r + pk + message), "little") % L
    s = (r + k * a) % L
    return big_r + s.to_bytes(32, "little")


# --- arguments -------------------------------------------------------------
manifest_path, channel, out_path, inplace, print_pubkey = sys.argv[1:6]
inplace = inplace == "1"
print_pubkey = print_pubkey == "1"

key_hex = os.environ.get("GOBLE_UPDATE_SIGNING_KEY", "").strip()
if key_hex.startswith("0x"):
    key_hex = key_hex[2:]

seed = None
if key_hex:
    try:
        raw = bytes.fromhex(key_hex)
    except ValueError:
        sys.stderr.write("error: GOBLE_UPDATE_SIGNING_KEY is not valid hex\n")
        raise SystemExit(2)
    if len(raw) == 64:
        raw = raw[:32]  # seed || public key: keep the seed
    if len(raw) != 32:
        sys.stderr.write(
            "error: GOBLE_UPDATE_SIGNING_KEY must be 32 bytes (64 hex chars); "
            "got %d bytes\n" % len(raw)
        )
        raise SystemExit(2)
    seed = raw

if print_pubkey:
    if seed is None:
        sys.stderr.write("error: --print-pubkey needs GOBLE_UPDATE_SIGNING_KEY\n")
        raise SystemExit(2)
    print(publickey(seed).hex())
    raise SystemExit(0)

if seed is None:
    sys.stderr.write("error: GOBLE_UPDATE_SIGNING_KEY is not set\n")
    raise SystemExit(2)

with open(manifest_path, "r", encoding="utf-8") as fh:
    manifest = json.load(fh)

channels = manifest.get("channels", {})
if channel not in channels:
    sys.stderr.write("error: channel '%s' not present in %s\n" % (channel, manifest_path))
    raise SystemExit(2)

channel_obj = channels[channel]
if not isinstance(channel_obj, dict):
    sys.stderr.write("error: channel '%s' is not an object\n" % channel)
    raise SystemExit(2)

# Sign the object without its signature field, so the bytes are stable whether
# or not this script has already run.
signed_obj = {k: v for k, v in channel_obj.items() if k != "signature"}

SIGNING_DOMAIN = "goble-update-v1"


def signing_payload(release):
    version = release.get("version")
    if not isinstance(version, str) or not version:
        sys.stderr.write("error: the channel object has no version\n")
        raise SystemExit(2)

    lines = [SIGNING_DOMAIN, version]
    artifacts = []
    for artifact in release.get("artifacts", []):
        try:
            artifacts.append(
                "%s %s %s %d %s"
                % (
                    str(artifact["os"]).lower(),
                    str(artifact["arch"]).lower(),
                    str(artifact["sha256"]).lower(),
                    int(artifact["size"]),
                    artifact["url"],
                )
            )
        except (KeyError, TypeError, ValueError):
            sys.stderr.write("error: an artifact is missing os/arch/sha256/size/url\n")
            raise SystemExit(2)

    if not artifacts:
        sys.stderr.write("error: the channel object has no artifacts\n")
        raise SystemExit(2)

    artifacts.sort()
    lines.extend(artifacts)
    return "\n".join(lines).encode("utf-8")


payload = signing_payload(signed_obj)
signature = sign(payload, seed)

with open(out_path, "w", encoding="utf-8") as fh:
    fh.write(signature.hex() + "\n")

if inplace:
    channels[channel] = dict(signed_obj)
    channels[channel]["signature"] = signature.hex()
    with open(manifest_path, "w", encoding="utf-8") as fh:
        json.dump(manifest, fh, indent=2, ensure_ascii=False)
        fh.write("\n")

sys.stderr.write("==> Signed channel '%s' (%d payload bytes)\n" % (channel, len(payload)))
sys.stderr.write("==> Public key: %s\n" % publickey(seed).hex())
sys.stderr.write("==> Signature written to %s\n" % out_path)
print(signature.hex())
PY
