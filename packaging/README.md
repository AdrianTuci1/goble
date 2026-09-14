# Packaging

Build scripts for the shipping artifacts of **Goble**, the native desktop app
(`app/` → package `goble-app`, binary `goble-app`). Nothing here builds Rust:
every script takes an already-built release binary.

The one command that does everything for the host platform is
[`../scripts/release.sh`](../scripts/release.sh).

```
packaging/
  version.env          single source of truth: names, bundle id, version fallback
  EULA.txt             licence page shown by the Windows installer
  README.md            this file
  sign-manifest.sh     Ed25519-signs a channel in channel_versions.json
  macos/               Info.plist, make-bundle.sh, make-dmg.sh, dmg-background.png
  linux/               goble.desktop, make-appimage.sh, make-deb.sh, make-tarball.sh
  windows/             goble.iss, build-installer.ps1
```

## Quick start

```bash
# Build once, then package for this host (unsigned unless secrets are set)
cargo build --release -p goble-app
scripts/release.sh                      # installers + dist/SHA256SUMS
scripts/release.sh --manifest           # ... plus dist/channel_versions.json
scripts/release.sh --dry-run            # print the plan, build nothing
scripts/release.sh --universal          # macOS: lipo-merged universal .app/.dmg
scripts/release.sh --linux-format all   # Linux: AppImage + .deb + .tar.gz
```

Individual steps, if you want them separately:

```bash
packaging/macos/make-bundle.sh --binary target/release/goble-app --outdir dist
packaging/macos/make-dmg.sh    --app dist/Goble.app --outdir dist

packaging/linux/make-appimage.sh --binary target/release/goble-app --outdir dist
packaging/linux/make-deb.sh      --binary target/release/goble-app --outdir dist
packaging/linux/make-tarball.sh  --binary target/release/goble-app --outdir dist

# Windows, from PowerShell
./packaging/windows/build-installer.ps1 -Version 0.1.0 `
    -SourceBinary target/release/goble-app.exe -OutDir dist
```

Every script takes `--help`.

## Prerequisites

| Platform | Required | Notes |
|---|---|---|
| macOS | Xcode Command Line Tools (`hdiutil`, `codesign`, `sips`, `iconutil`, `lipo`, `osascript`) | ships with CLT; `xcrun notarytool` also needs the CLT |
| Linux (`AppImage`) | `linuxdeploy` **or** `appimagetool` on `PATH` | self-contained AppImages; see below |
| Linux (`.deb`) | `dpkg-deb` (`dpkg-dev`) | |
| Linux (RPM) | `rpmbuild` (`rpm`) | prerequisite for the roadmap only — no `.spec` exists yet, see *Not covered yet* |
| Linux (built from source) | `pkg-config`, `libxkbcommon-dev`, `libwayland-dev`, `libx11-dev`, `libxcb1-dev`, `libvulkan-dev`, `libgl1-mesa-dev`, `libfontconfig1-dev`, `libfreetype-dev` | compile-time only |
| Windows | Inno Setup 6 (`ISCC.exe`) | `choco install innosetup --version=6.3.3`, or set `INNO_SETUP_ISCC` |
| All | Rust ≥ 1.86, `python3` (only for `sign-manifest.sh`) | |

`appimagetool` is not in most distro repos; download the AppImage, `chmod +x`
it, and put it on `PATH`. On hosts without `/dev/fuse` (containers, most CI
runners) `make-appimage.sh` automatically sets `APPIMAGE_EXTRACT_AND_RUN=1`.

`rpmbuild` is a listed prerequisite for the roadmap only — see *Not covered
yet* below.

### macOS icons

`make-bundle.sh` defaults to the in-repo 256×256 `icon.png` and will **upscale**
it to fill the `.icns` sizes, warning as it does so. Pass a 1024×1024 source for
a crisp Retina icon:

```bash
packaging/macos/make-bundle.sh --binary target/release/goble-app \
    --icon path/to/icon-1024.png --outdir dist
# or: APP_ICON=path/to/icon-1024.png scripts/release.sh
```

## Signing and notarization

**A release is signed, or it is not a release.** `GOBLE_REQUIRE_SIGNING=1` turns a
missing credential into a build failure instead of a warning, and
`scripts/release.sh` sets it for every channel except `dev`, where an unsigned
artifact is the point. Without it, the failure mode is an "official" download
that Gatekeeper refuses on macOS or SmartScreen flags on Windows — which is
exactly what happened before this existed.

`scripts/release.sh --allow-unsigned` is the deliberate way out for a local test
build. Running a packaging script directly, set `GOBLE_REQUIRE_SIGNING=0`. The
scripts never fabricate a signature.

### What each credential buys

| Variable | Effect |
|---|---|
| `APPLE_SIGNING_IDENTITY` | `codesign --force --options runtime --timestamp --entitlements macos/entitlements.plist` on the `.app` and the `.dmg`, then `codesign --verify --strict` |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | `xcrun notarytool submit --wait`, `stapler staple`, `stapler validate`, then a `spctl --assess` on both the DMG and the app — the check that answers "would Gatekeeper accept this?" |
| `APPLE_CERTIFICATE_P12`, `APPLE_CERTIFICATE_PASSWORD` | CI only: base64 `.p12` imported into a temporary keychain before signing |
| `WINDOWS_SIGN_CERT_PFX` + `WINDOWS_SIGN_CERT_PASSWORD` | `signtool sign /f … /fd sha256 /tr … /td sha256` |
| `WINDOWS_SIGN_CERT_THUMBPRINT` | alternative: `signtool sign /sha1 …` from the user store |
| `WINDOWS_SIGN_TIMESTAMP_URL` | RFC 3161 timestamp URL (default `http://timestamp.digicert.com`) |
| `GOBLE_UPDATE_SIGNING_KEY` | hex Ed25519 seed; signs the manifest channel (see below) |
| `INNO_SETUP_ISCC` | explicit path to `ISCC.exe` |

The app executable is signed **before** it is packaged on Windows, and the
result is checked with `signtool verify /pa`; the setup engine and uninstaller
are signed by Inno through the `SignTool` directive and verified the same way.
The hardened runtime is enabled on macOS with the entitlements in
`macos/entitlements.plist`, which is deliberately empty and explains why; there
is no `--deep`, because Apple deprecates it for signing.

### Obtaining the credentials

Both certificates cost money and are tied to a verified identity; there is no
free path to a trusted signature.

| Platform | What to get | Cost |
|---|---|---|
| macOS | [Apple Developer Program](https://developer.apple.com/programs/) membership, then a **Developer ID Application** certificate (Xcode → Settings → Accounts → Manage Certificates, or `developer.apple.com` → Certificates). The identity string looks like `Developer ID Application: Example Ltd (ABCDE12345)`. `APPLE_TEAM_ID` is the parenthesised part. `APPLE_APP_PASSWORD` is an app-specific password from [appleid.apple.com](https://appleid.apple.com) — not the account password. | 99 USD/year |
| Windows | An **OV or EV code-signing certificate** from a CA (DigiCert, Sectigo, SSL.com…). Since June 2023 a private key must live on a FIPS 140-2 Level 2 / Common Criteria hardware token or a cloud HSM, so `WINDOWS_SIGN_CERT_THUMBPRINT` against a token in the store is often easier than a `.pfx`. | ~200–500 USD/year |

CI reads them from repository secrets with exactly the names in the table above.
For `APPLE_CERTIFICATE_P12`, export the certificate and key from Keychain Access
as a `.p12` and base64 it:

```sh
base64 -i certificate.p12 | pbcopy      # paste into the APPLE_CERTIFICATE_P12 secret
```

Export the `.p12` with the legacy algorithms if you generated it with OpenSSL 3:
macOS `security import` cannot read the default PBES2/AES-256 format.

### Verifying a download

Exactly what a user can do, and what a reviewer should do before publishing:

```sh
# macOS
codesign --verify --strict --verbose=2 /Applications/Goble.app
codesign -dv --verbose=4 /Applications/Goble.app      # expect: Developer ID Application, flags=runtime
spctl --assess --type exec --verbose=4 /Applications/Goble.app   # expect: accepted, source=Developer ID
xcrun stapler validate Goble-<v>-arm64.dmg
```

```powershell
# Windows
Get-AuthenticodeSignature .\GobleSetup.exe | Format-List Status, SignerCertificate
signtool verify /pa /v .\GobleSetup.exe
```

```sh
# Linux: there is no per-artifact signature. The ed25519 signature over
# channel_versions.json is what authenticates the AppImage/.deb, because the
# signed payload contains each artifact's sha256.
```

`make-dmg.sh --no-sign` forces an unsigned build even when secrets are present,
and is refused when `GOBLE_REQUIRE_SIGNING=1`.

### Testing the signing path without a certificate

The signing code can be exercised on any Mac with an ad-hoc signature, which
applies `--options runtime` and the entitlements exactly like a real identity
does. Only Apple's timestamp service cannot be reached, so shim `codesign` to
drop `--timestamp`:

```sh
mkdir -p /tmp/shim
printf '#!/bin/sh\nargs=""; for a in "$@"; do [ "$a" = "--timestamp" ] && continue; args="$args \\"$a\\""; done\neval exec /usr/bin/codesign $args\n' > /tmp/shim/codesign
chmod +x /tmp/shim/codesign
PATH="/tmp/shim:$PATH" packaging/macos/make-bundle.sh --binary /path/to/binary --sign-identity -
codesign -dv --verbose=4 dist/Goble.app | grep flags     # expect flags=0x10002(adhoc,runtime)
```

`spctl` will still reject the result — an ad-hoc signature is not a trusted
source — which is the reason `GOBLE_REQUIRE_SIGNING` exists.

## Artifact naming — the updater contract

The updater resolves artifacts from `channel_versions.json` by **exact URL**, so
the file names below are part of the interface. `<v>` is the version without a
leading `v` (tag `v0.1.0` → `0.1.0`).

| `os` | `arch` | File name |
|---|---|---|
| `macos` | `aarch64` | `Goble-<v>-arm64.dmg` |
| `macos` | `x86_64` | `Goble-<v>-x64.dmg` |
| `macos` | either | `Goble-<v>-universal.dmg` (used for both entries when per-arch DMGs are absent) |
| `linux` | `x86_64` | `Goble-<v>-x86_64.AppImage` \| `goble_<v>_amd64.deb` \| `goble-<v>-linux-x86_64.tar.gz` |
| `linux` | `aarch64` | `Goble-<v>-aarch64.AppImage` \| `goble_<v>_arm64.deb` \| `goble-<v>-linux-aarch64.tar.gz` |
| `windows` | `x86_64` | `Goble-<v>-win-x64-setup.exe` |
| `windows` | `aarch64` | `Goble-<v>-win-arm64-setup.exe` |

When several formats exist for one `(os, arch)` pair, the manifest advertises
the first match in this order: macOS `.dmg`; Linux `.AppImage` > `.deb` >
`.tar.gz`; Windows `.exe`.

`dist/SHA256SUMS` lists every released file as `<sha256>  <filename>`
(two spaces, GNU coreutils format), excluding `SHA256SUMS` itself, the manifest,
and `*.sig` sidecars. The DMG volume name is `Goble <version>`; within it the app
sits at x≈180 and the `/Applications` symlink at x≈480 on a 660×400 background.

## `channel_versions.json` and manifest signing

`scripts/release.sh --manifest` writes `dist/channel_versions.json` with exactly
this shape (no extra keys):

```json
{
  "schema": 1,
  "channels": {
    "stable": {
      "version": "0.1.0",
      "artifacts": [
        {"os": "macos", "arch": "aarch64", "url": "https://…/Goble-0.1.0-arm64.dmg", "sha256": "…", "size": 12345}
      ]
    }
  }
}
```

Artifact URLs default to
`https://github.com/AdrianTuci1/goble/releases/download/v<version>/<file>`;
override with `--download-base` or `GOBLE_DOWNLOAD_BASE`.

With `GOBLE_UPDATE_SIGNING_KEY` set, a `"signature"` field is added to the
channel object and the hex signature is also written to
`dist/channel_versions.json.<channel>.sig`. Without it the field is omitted
entirely — never faked.

```bash
# sign the channel object of an existing manifest
GOBLE_UPDATE_SIGNING_KEY=<64 hex chars> packaging/sign-manifest.sh \
    --manifest dist/channel_versions.json --channel stable --inplace
# derive the public key to ship in the verifier
GOBLE_UPDATE_SIGNING_KEY=<64 hex chars> packaging/sign-manifest.sh --print-pubkey
```

The signature is over a fixed text payload, not over JSON — JSON has many
spellings for the same value, and a signature over a serialisation is a
signature over whoever wrote the serialiser:

```
goble-update-v1
<version>
<os> <arch> <sha256> <size> <url>      # one line per artifact, sorted
```

`os`, `arch` and `sha256` are lowercased, the artifact lines are sorted as plain
strings, and the `signature` field itself is never part of the payload. The
definition lives in `goble_update::ChannelRelease::signing_payload`;
`crates/goble-update/tests/signer_contract.rs` runs this script on a throwaway
manifest and verifies the signature with the Rust verifier, so the signer and
the client cannot drift apart without a failing test.

Ed25519 is implemented inline in `sign-manifest.sh` using only `hashlib`, so no
third-party Python package is needed. The implementation is checked against the
RFC 8032 test vectors.

## Windows installer notes

* Per-user install (`PrivilegesRequired=lowest`); `{autopf}` resolves to
  `%LocalAppData%\Programs`, so no UAC prompt.
* `LicenseFile=EULA.txt` — `build-installer.ps1` stages the `.iss` next to a
  rendered copy of `EULA.txt`, substituting `@VERSION@` and `@DATE@`.
* `MinVersion=10.0.18362` (Windows 10 version 1903). The terminal opens a
  pseudo-console through `portable-pty`, which is ConPTY on Windows; Inno's own
  default would let the installer run on Windows 7, where the terminal cannot
  start at all.
* Single-instance contract: `AppMutex=Goble-App-SingleInstance`. The app must
  create a mutex with that exact name for the installer/updater to detect a
  running instance.
* Auto-updater invocation, documented in the `goble.iss` header:

  ```
  Goble-<v>-win-x64-setup.exe /SILENT /NORESTART /NOCLOSEAPPLICATIONS \
      /SUPPRESSMSGBOXES /DIR="<install dir>" /update=1
  ```

  `/update=1` is read by `[Code]` to skip the licence page and the post-install
  launch checkbox. The install directory is recorded under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{AppId}_is1`.

## Not covered yet

* **No certificate has ever been used.** The signing code is exercised with an
  ad-hoc signature (see *Testing the signing path without a certificate*), and
  the DMG pipeline runs end to end locally, but a real Developer ID identity,
  a real notarization round trip and a real Authenticode certificate have not
  been tried: neither credential exists yet. Everything downstream of the
  certificate is therefore unverified — in particular, `xcrun notarytool`
  rejections and the `spctl` assessment.
* **MSI / WiX.** Only the Inno Setup `.exe` is produced.
* **RPM.** `rpmbuild` is not wired up even though packages list it as a
  prerequisite for future work.
* **Flatpak and Snap.** No manifests exist for either.
* **Homebrew cask / winget / Scoop manifests.** The artifacts are produced, but
  no package-manager definitions or tap updates are generated.
* **macOS PKG installer.** Only the DMG drag-to-Applications flow.
* **ARM64 Linux and Windows CI builds.** The scripts accept `--arch` for both,
  but the workflow matrix builds Linux x86_64 and Windows x64 only.
* **Code signing in the release workflow** is wired to repository secrets
  (`APPLE_*`, `WINDOWS_SIGN_*`) and now fails the build when they are absent,
  rather than publishing an unsigned artifact; it is not exercised until those
  secrets exist.
* **Reproducible builds.** Timestamps and archive metadata are not pinned.
