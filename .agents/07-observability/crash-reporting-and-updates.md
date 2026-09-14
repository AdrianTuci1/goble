# Crash reporting, updates, and installers

**Status:** `[~]` partial: the three layers are implemented and tested; the collector host, the
signing key, and the UI surfaces are not deployed
**Owns:** crash capture, telemetry upload, release manifests and verified updates, installer scripts
**Depends on:** [`../06-renderer/README.md`](../06-renderer/README.md), [`logs.md`](logs.md),
[`executions-and-trace.md`](executions-and-trace.md)

## Why this exists

Until this work, Goble was a binary you compile yourself. That is a project, not a product: a user who
hits a bug has no way to tell us anything, and every improvement has to be re-obtained by hand. The
three pieces below are the minimum a desktop product needs — a way to hear about failures, a way to
deliver fixes, and a way to install without a toolchain.

The constraint that shapes all of it: Goble is open source, and a user may never sign in. So nothing
here may depend on an account, and everything that leaves the machine must be something the user
could have refused.

## 1. Crash reporting and telemetry — `crates/goble-telemetry`

| Module | Responsibility |
|---|---|
| `config.rs` | `TelemetryConfig`, `Channel`, consent resolution, the `~/.goble` paths, the on-disk state (install id, user choice) |
| `redact.rs` | The scrubber applied to every free-text field: known secrets, credential-shaped literals, bearer headers, long opaque runs, emails, and the home directory |
| `breadcrumbs.rs` | A bounded ring buffer of recent log lines, fed by wrapping the app's logger |
| `report.rs` | `CrashReport`, `TelemetryEvent`, `QueuedItem`, the crash fingerprint, and the human-readable archive |
| `queue.rs` | The on-disk queue: one file per item, attempts persisted, pruned by count and age |
| `upload.rs` | `Uploader` (HTTP and test doubles), the drain policy, and the background worker thread |
| `panic.rs` | The panic hook: capture first, then run whatever hook was installed before |

Decisions worth keeping:

- **Local capture is always on; uploading is the consent-gated half.** A panic writes a report to
  `~/.goble/crashes/` before anything is sent, so the file exists even when uploads are off, the
  network is down, or the collector is not deployed. That file is what a user attaches to an issue.
- **Precedence is `DO_NOT_TRACK` > `GOBLE_TELEMETRY` > the stored user choice > the channel default.**
  `DO_NOT_TRACK=0` is not an opt-in. A released channel reports by default, a dev build does not.
  Warp has no `DO_NOT_TRACK` support at all; this is deliberately better than the reference.
- **A report carries a fixed field set.** Version, channel, platform, an anonymous install id, a random
  session id, a fingerprint, the panic text, a location, a backtrace, and the log tail. No user
  content, no command lines, no file paths other than the panic's own source location. Redaction is
  the second net, not the first.
- **The fingerprint is over the message, with digits folded to `N`.** Grouping by location would split
  one bug into one group per build; grouping by raw message would split "the len is 3" from "the len
  is 7".
- **Nothing is lost when the collector is unreachable.** Items wait in `~/.goble/telemetry-queue/`,
  retry with backoff, and survive a restart. A permanent rejection (a 4xx that is not a timeout or a
  rate limit) deletes the item, because retrying cannot change the answer.
- **Release builds keep line tables** (`[profile.release] debug = 1`, `strip = false`). Without them a
  backtrace is a list of addresses and a crash report is not actionable.

## 2. The collector — `crates/goble-telemetry-server`

`POST /v1/crashes`, `POST /v1/events`, `GET /v1/summary`, `GET /health`. It stores one file per report
under `crashes/<fingerprint>/<id>.json` and appends events to `events/<date>.jsonl`, which makes
"how many people hit this, and on which version" a directory listing.

It deliberately does not log IP addresses, set cookies, or call a third party. It refuses an unknown
`schema`, bounds every body (`MAX_BODY`), and refuses a fingerprint that would escape the store
directory. Setting `GOBLE_TELEMETRY_TOKEN` makes every ingest and summary request require the token —
which it must be, the moment it is reachable from the internet.

## 3. Update delivery — `crates/goble-update`

One manifest per release, per channel, listing artifacts per platform with their digests:

```json
{"schema": 1, "channels": {"stable": {"version": "0.2.0", "artifacts": [
  {"os": "macos", "arch": "aarch64", "url": "https://…", "sha256": "…", "size": 12345678}
], "signature": "…"}}}
```

Four steps, in order: fetch and validate the manifest; compare versions with semver; download into a
staging directory while hashing, and refuse a digest or size mismatch; verify the manifest's ed25519
signature. Then — and only then — plan an install.

- **Verification is not optional.** With no configured public key, a release is reported as unverified
  and staging refuses it. This is the one place where the reference implementation is weaker than us
  on purpose: it checks the macOS code signature but no manifest signature, so whoever controls the
  release host decides what every installation runs.
- **The signed payload is a fixed text form**, not the JSON document
  (`goble_update::ChannelRelease::signing_payload`), because a signature over a serialisation is a
  signature over whoever wrote the serialiser. `crates/goble-update/tests/signer_contract.rs` runs
  `packaging/sign-manifest.sh` and verifies its output with the client verifier, so the signer and the
  verifier cannot drift apart without a failing test.
- **Install plans are pure decisions**, so "this is a Homebrew install", "this copy is not inside an
  app bundle", and "the package manager owns this file" are testable without touching a real
  installation. `plan_install` produces one of: replace the app bundle, replace the AppImage, run the
  vendor installer, or hand the user a command.
- **A `.deb` or `.rpm` install is handed back to the package manager.** Overwriting a file the package
  manager owns produces a machine that lies about what is installed.

Known limitation: the macOS swap is two renames, not an atomic `renamex_np(RENAME_SWAP)`, which needs
`unsafe` and the workspace denies `unsafe_code`. The window is small and the swap script logs what it
did, but it is a real gap.

## 4. Installers — `packaging/` and `.github/workflows/release.yml`

| Platform | Artifacts | Notes |
|---|---|---|
| macOS | `Goble-<v>-arm64.dmg`, `Goble-<v>-x64.dmg`, `…-universal.dmg` | `hdiutil` disk image with a custom background image and a fixed icon layout; Developer ID signing, hardened runtime with `macos/entitlements.plist`, notarisation, stapling, and a Gatekeeper assessment |
| Linux | `Goble-<v>-x86_64.AppImage`, `goble_<v>_amd64.deb`, `Goble-<v>-linux-x86_64.tar.gz` | AppImage, Debian package, and a tarball fallback; integrity comes from the signed manifest, since Linux artifacts carry no signature of their own |
| Windows | `Goble-<v>-win-x64-setup.exe` | Inno Setup, per-user install, EULA page, a Windows 10 1903 floor (below it ConPTY, and therefore the terminal, cannot start), the app executable signed before packaging and the setup engine and uninstaller signed by Inno, both verified with `signtool`, silent flags documented for the updater |

`scripts/release.sh` builds the app for the host, runs the host's packaging script, writes
`dist/SHA256SUMS`, and with `--manifest` writes `dist/channel_versions.json`. It sets
`GOBLE_CHANNEL` and `GOBLE_RELEASE_VERSION` at build time, because the app reads both with
`option_env!`: they decide whether a build reports crashes, accepts updates, and which version it
claims to be.

Signing is environment-driven (`APPLE_SIGNING_IDENTITY`, `APPLE_*` for notarisation,
`WINDOWS_SIGN_*`, `GOBLE_UPDATE_SIGNING_KEY`). It is also **required**: `GOBLE_REQUIRE_SIGNING=1`
makes a missing credential a build failure, and `scripts/release.sh` sets it for every channel except
`dev`, so a release cannot be published unsigned by accident. A signed release is checked rather than
assumed — `codesign --verify --strict`, `spctl --assess` on both the DMG and the app,
`stapler validate`, and `signtool verify` on the Windows app executable and installer. The macOS
bundle is signed with the hardened runtime and `macos/entitlements.plist`, which is deliberately
empty and says why; `--deep` is gone, because Apple deprecates it for signing. `--allow-unsigned` is
the explicit way to build a local test artifact. See `packaging/README.md`, section Signing, for what
the two certificates cost and how to obtain them.

## Reference mapping (warp-new)

`warp-new` is AGPL-3.0-only and is read as a **read-only architectural reference**. No code from it is
copied, linked, or vendored here.

| Idea taken from the reference | What Goble does differently |
|---|---|
| A per-channel version manifest fetched at a stable URL | Same shape, plus an ed25519 signature over a fixed text payload |
| Sentinel/manifest-driven crash reporting with the DSN in channel config | We run our own collector, so there is no third-party processor in the path and no account |
| An anonymous install id, stored under a non-telemetry-looking key | Same idea, stored as `install_id` in `~/.goble/telemetry.json`; honesty is cheaper than obscurity for an open-source app |
| Unconditional regex secret redaction on outbound payloads | Same idea, implemented in `redact.rs`, with the same rule: do not log secrets in the first place |
| A hand-rolled updater per platform, `codesign` verification of the DMG | Same platform split, plus manifest signature verification and a refusal to install what it cannot authenticate |
| Silently downloaded updates, a banner when one is ready | Not implemented yet; the app does not check for updates at all right now |
| ~2,900 lines of packaging: `cargo bundle` + the pinned `create-dmg`, Inno Setup, and `linuxdeploy` plus deb/rpm/Arch templates | Same four installer targets, built with the tool each platform already has (`hdiutil`, Inno Setup, `appimagetool`/`dpkg-deb`, a tarball). We add what that tree turned out to be missing: Developer ID signing and notarisation, an actual EULA page on the installer, and a background image whose path exists |

The reference's packaging has dead ends of its own, which is why it was not copied: no notarisation
anywhere in the tree, no release code signing outside the dev runner, a DMG background path pointing
at a file that no longer exists, and a main release workflow that is referenced but absent. Its Sentry
integration is a dead end too — the dependencies have been deleted and the feature is an empty shell,
so there is nothing to learn from it beyond the tag and breadcrumb taxonomy.

## Remaining work

- [ ] **Obtain the two signing certificates** — an Apple Developer Program membership with a
      Developer ID Application certificate (99 USD/year) and a Windows OV/EV code-signing certificate
      — and add them to the repository secrets listed in `packaging/README.md`. Until then no
      official build can be produced at all, because a release refuses to be unsigned.
- [ ] **Run one release with the certificates** and confirm the notarisation round trip, the `spctl`
      assessment, and `signtool verify` on a real artifact. The signing code path is exercised with an
      ad-hoc signature and the DMG pipeline runs end to end, but nothing downstream of a real
      certificate has been tried.
- [ ] Deploy a collector host, set `GOBLE_TELEMETRY_ENDPOINT` for release builds, and confirm an
      end-to-end upload (staging → collector → summary) against the real host.
- [ ] Generate the release signing key pair, publish the public key, and make `--manifest` sign in CI.
- [ ] Surface an available update in the UI: a banner or a status-bar entry with "Restart and update",
      plus a manual "Check for updates" action. Today the check is off and log-only.
- [ ] Wire `plan_install().apply()` into that flow, with the restart/relaunch handoff.
- [ ] A first-run telemetry notice. `TelemetryState.notice_shown` exists and nothing sets it.
- [ ] A settings surface for the telemetry switch, writing `~/.goble/telemetry.json`.
- [ ] Minidump capture on Linux and Windows. A panic hook catches Rust panics; a segfault or a GPU
      driver crash produces nothing today.
- [ ] Split debug info and upload symbols per release, so a backtrace from a stripped CI artifact
      still resolves.
- [ ] Consider an atomic bundle swap on macOS behind a small `unsafe` opt-in (see the limitation above).
- [ ] Run the packaging scripts for real once: no DMG has been produced, no `.deb` installed, no Inno
      Setup compile executed. All of it is reviewed and syntax-checked but unexercised.
- [ ] Fix or drop the release workflow's formatting gate. `./scripts/fmt.sh --check` reports roughly
      200 hunks across 127 files in committed source, none of them from this work, so `publish-release`
      cannot reach the end. (`cargo fmt --all` is not the answer either: `--all` formats path
      dependencies and would rewrite the vendored emulator.)
- [ ] Track `vendor/alacritty_terminal` in git. It is a path dependency of `goble-terminal`, so a
      checkout without it fails every cargo command in the workspace, not just the terminal's; a fresh
      clone and every CI job are broken until it is restored. The workflow now recovers with
      `scripts/fetch-alacritty-terminal.sh --install`.
- [ ] Add RPM, Flatpak, Homebrew cask, and winget coverage, and arm64 Linux/Windows CI runners.
- [ ] Generate `THIRD-PARTY-NOTICES.md` from the dependency graph (the reference runs `cargo about`
      with a template at bundle time) instead of maintaining the table by hand, so it cannot fall
      behind the lockfile.
- [ ] Decide whether to ship our own ConPTY (`conpty.dll` + `OpenConsole.exe`, as the reference does)
      so Windows 10 1809 is supported; today we depend on the OS one and refuse to install below 1903.

## How to verify

```sh
cargo test -p goble-telemetry           # consent, redaction, queue, upload policy, panic hook
cargo test -p goble-telemetry-server    # ingest, auth, schema refusal, store layout
cargo test -p goble-update              # manifest, signature, digest, install plans
cargo test -p goble-update --test signer_contract   # the packaging signer vs the client verifier
cargo clippy -p goble-telemetry -p goble-telemetry-server -p goble-update --all-targets -- -D warnings
./scripts/fmt.sh --check                # scoped to workspace members; never rewrites vendor/
```

A report can also be posted by hand, which is how the collector is exercised without a deploy:

```sh
cargo run -p goble-telemetry-server            # listens on 127.0.0.1:8787 by default
curl -X POST --data @report.json http://127.0.0.1:8787/v1/crashes
```
