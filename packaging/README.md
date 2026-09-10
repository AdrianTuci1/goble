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

All signing is environment-driven. When a variable is missing the scripts
produce an **unsigned** artifact and print a clear warning — they never fail
just because secrets are absent, and they never fabricate a signature.

| Variable | Effect |
|---|---|
| `APPLE_SIGNING_IDENTITY` | `codesign --deep --force --options runtime --timestamp` on the `.app` and the `.dmg` |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | `xcrun notarytool submit --wait` + `stapler staple` on the `.dmg` (requires an identity) |
| `APPLE_CERTIFICATE_P12`, `APPLE_CERTIFICATE_PASSWORD` | CI only: base64 `.p12` imported into a temporary keychain before signing |
| `WINDOWS_SIGN_CERT_PFX` + `WINDOWS_SIGN_CERT_PASSWORD` | Inno `SignTool` via `signtool sign /f …` |
| `WINDOWS_SIGN_CERT_THUMBPRINT` | alternative: `signtool sign /sha1 …` from the user store |
| `WINDOWS_SIGN_TIMESTAMP_URL` | RFC 3161 timestamp URL (default `http://timestamp.digicert.com`) |
| `GOBLE_UPDATE_SIGNING_KEY` | hex Ed25519 seed; signs the manifest channel (see below) |
| `INNO_SETUP_ISCC` | explicit path to `ISCC.exe` |

Both timestamp and hardened-runtime options are always passed when signing.
`make-dmg.sh --no-sign` forces an unsigned build even when secrets are present.

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

* **Notarization without secrets.** `APPLE_ID` / `APPLE_TEAM_ID` /
  `APPLE_APP_PASSWORD` must be present; otherwise the DMG ships unsigned.
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
  (`APPLE_*`, `WINDOWS_SIGN_*`) but is not exercised until those secrets exist.
* **Reproducible builds.** Timestamps and archive metadata are not pinned.
