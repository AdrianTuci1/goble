<p align="center">
  <img src="assets/header.svg" alt="goble — a reversible execution substrate for agentic work" width="820">
  <br>
  A reversible execution substrate for agentic work.
  <br><br>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.86%2B-orange.svg" alt="Rust 1.86 or newer">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg" alt="Platforms: macOS, Linux, Windows">
  <img src="https://img.shields.io/badge/telemetry-opt--out-success.svg" alt="Telemetry is opt-out and can be disabled">
</p>

## About

Goble is a runtime and interaction layer for agentic work. It turns an agent's execution into a
reversible, Git-like trace that can be observed, forked, replayed, and reverted, and it presents that
trace per project rather than per process.

The problem it addresses is that agent work is currently opaque and irreversible. An agent runs,
produces a result, and leaves behind little more than a log. You cannot inspect a single step, undo
it, or try a different branch from the same point. Goble makes the execution the artifact: every
turn is a durable record you can inspect and act on.

Goble does not ship a single agent. It attaches to the agent you already use through a transport
seam, so the same reversibility, observability, and interaction features apply regardless of which
harness is underneath. This is the bring-your-own-harness (BYOH) model. An adapter decides whether a
harness runs in-process, as a CLI subprocess, or on a remote daemon.

Three properties make Goble distinct:

1. **Reversibility.** Execution is a Git-like trace. From the event stream any harness already emits,
   the daemon keeps a per-turn ledger, so you can fork from a turn, replay it, or revert it. The
   transcript is always reversible; a harness can opt in to environment snapshots and restore as well.
2. **Interaction.** Live voice (speech to text and text to speech) and screen. Screen works in both
   directions: broadcasting your screen for observation, and computer-use, where the agent drives
   input.
3. **Per-project observability.** For each directory you can see what is running, what is scheduled,
   and what has finished. The project, not the agent, is the unit of attention.

| | |
|---|---|
| **It is** | a runtime and interaction layer for agentic work, with pluggable harnesses |
| **It is not** | yet another agent, and it is not a proprietary harness |

### Domain concepts

| Primitive | Meaning |
|---|---|
| **Medium** | which daemon listens: Local / VM / RemoteXrdp / Browser / Container |
| **Project** | a directory; the unit of observability and isolation |
| **Session** | a worktree or working session inside a project |
| **Task** | a harness invocation with grants (`ReadOnly` / `ReadWrite`) and a trigger (manual / cron / http / heartbeat) |

"Agent" is not a first-class entity in this model. A harness configured with scheduled tasks covers
the same ground, and per-project observability makes the agent wrapper unnecessary.

### Architecture at a glance

```
GUI (app/, thick app on goble-ui)
      │ thin client
      ▼
DaemonClient (goble-daemon-client)   ── in-process or over mTLS WebSocket
      │
      ▼
DaemonPort (goble-daemon)            ← reusable, framework-agnostic core
      │ transport seam
      ▼
goble-harness-protocol (BYOH seam) → HarnessRuntime (internal | cli subprocess | remote)
```

The layering rule is `types ← protocol ← runtime`. There are two composition roots over one core: the
daemon runs **embedded** in-process inside the GUI binary, where the local voice and screen code
lives, or **headless** as `goblin-worker` on a remote host. The GUI does not know or care which one it
is talking to; the medium is simply which daemon listens. Reversibility is backed by `goble-replay`
(a per-turn ledger) and `goble-persistence` (a SQLite checkpoint store), so the history survives a
restart.

For the full architecture, see [`docs/substrate-architecture.md`](docs/substrate-architecture.md),
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) (identity, mTLS, and roles), and
[`docs/PROJECT_STRUCTURE.md`](docs/PROJECT_STRUCTURE.md) (workspace file layout).

## Installation

Goble is distributed as native installers. For anyone who is not working on the code, the released
binaries are the supported path; building from source is a developer workflow described further down.

### macOS

Download the `.dmg` from the releases page. Separate builds are published for Intel (x86_64) and
Apple Silicon (aarch64); pick the one matching your machine. Official builds are signed with a
Developer ID certificate and notarized by Apple, so they open without a Gatekeeper override. A build
you compile yourself is not signed, and that one needs the quarantine attribute cleared:

```sh
xattr -dr com.apple.quarantine /Applications/Goble.app
```

### Verifying a download

Releases are signed, and you can check it rather than trust it:

```sh
# macOS — expect "Developer ID Application: …", flags=runtime, and source=Developer ID
codesign -dv --verbose=4 /Applications/Goble.app
spctl --assess --type exec --verbose=4 /Applications/Goble.app
xcrun stapler validate Goble-<version>-arm64.dmg
```

```powershell
# Windows — expect Status: Valid and a signer name
Get-AuthenticodeSignature .\GobleSetup.exe | Format-List Status, SignerCertificate
```

Linux artifacts carry no signature of their own; the updater manifest is signed with ed25519 and
contains each artifact's SHA-256, which is where integrity is checked. `dist/SHA256SUMS` covers the
same files.

Release builds refuse to be produced unsigned: if the signing credentials are missing, the build
fails instead of publishing something users would have to work around. Local test builds are the
exception, and are made explicitly with `--allow-unsigned`. See
[`packaging/README.md`](packaging/README.md#signing-and-notarization).

### Linux

Three artifacts are published:

- **AppImage** — mark it executable, then run it:

  ```sh
  chmod +x Goble-*.AppImage
  ./Goble-*.AppImage
  ```

- **`.deb`** — install it with your package manager, for example `sudo apt install ./goble_*.deb`.
- **tarball** — unpack it and run the binary from the extracted directory.

### Windows

Download and run `GobleSetup.exe`. It is an Inno Setup installer that installs per user, so it does
not require administrator rights, and it shows the end-user license agreement during setup. It
requires Windows 10 version 1903 or newer: the terminal opens a pseudo-console, which is ConPTY on
Windows, and ConPTY is not dependable below that release.

### Updates

Releases are published with a manifest per channel. A build that is allowed to update itself fetches
the manifest, compares its own version, downloads the artifact for its platform, checks the SHA-256
against the manifest, and verifies the manifest's ed25519 signature before anything is installed.
macOS swaps the app bundle and relaunches, Linux replaces the AppImage in place, and Windows runs the
signed installer. A `.deb` or `.rpm` installation is handed back to the package manager instead,
because that installation is not ours to overwrite.

The update check is off in shipping builds today, because the release host and the signing key do not
exist yet: there is no public manifest URL, so no build checks for updates. `crates/goble-update` and
`packaging/sign-manifest.sh` are the two halves of the mechanism, and
`cargo test -p goble-update` runs the packaging signer against the client verifier so the two cannot
disagree about what a signature covers.

### Build from source

The repository is a single Cargo workspace. Install the toolchain with [rustup](https://rustup.rs/)
and build the product shell, `goble-app`:

```sh
rustup toolchain install stable
cargo build --release -p goble-app
```

The workspace's minimum supported Rust version is 1.86 (`rust-version` in `Cargo.toml`).

The terminal emulator core is vendored under `vendor/alacritty_terminal` and is a path dependency of
`goble-terminal`, so no `cargo` command in this workspace works without it. It is committed to the
repository; if your checkout lacks it, restore the pinned archive with:

```sh
./scripts/fetch-alacritty-terminal.sh --install
```

For day-to-day development, run the native UI through the dev script, which builds and launches the
app and, when `cargo-watch` is installed, rebuilds and restarts it on changes under `app/` and
`crates/`:

```sh
./scripts/dev-ui.sh
```

There is no hot reload: editing any `app` or `crates` source requires a rebuild, which `dev-ui.sh`
handles for you. To produce installers yourself, see the packaging recipes in `packaging/`
([`packaging/README.md`](packaging/README.md)).

## Contributions overview

### Where things live

`app/` is the product shell (`goble-app`, native wgpu 25 and winit 0.30). Everything else is a crate
under `crates/`. The crates fall into a few groups:

| Group | Crates | Role |
|---|---|---|
| Harness seam | `goble-harness-types`, `goble-harness-protocol`, `goble-harness-runtime`, `goble-harness-internal`, `goble-harness-cli`, `goble-foreign-harnesses` | The BYOH contract and its adapters |
| Daemon | `goble-daemon`, `goble-daemon-protocol`, `goble-daemon-client`, `goble-desktop-service`, `goblin-worker`, `goble-cli` | The core execution seam, the embedded root, and the headless root |
| Reversibility and persistence | `goble-replay`, `goble-persistence` | Per-turn ledger, fork/replay/revert/settlement, and the SQLite checkpoint store |
| UI | `goble-ui` (widget library), `app/` (product shell), `goble-desktop-native` (backend-integration reference, not the product shell) | Rendering, windowing, and the application views |
| Voice, sandbox, workflow, screen | `goble-voice`, `goble-sandbox`, `goble-workflow`, `goble-screen-core`, `goble-screen-adapter`, `goble-screen-sdk` | Capability crates behind the daemon |
| Terminal | `goble-terminal` plus the vendored `vendor/alacritty_terminal` | The VT emulator and its parser |
| Core | `goble-core`, `goble-telemetry`, `goble-telemetry-server`, `goble-update` | Store, identity, protocol, MCP, provisioning, crash reporting, the collector that receives it, and update delivery |

Three rules shape where new code goes:

- **`types ← protocol ← runtime`.** Model types carry no transport dependency; wire types come next;
  the runtime trait and registry come last. A new capability crate starts at the `types` layer.
- **`crates/` never depends on `app/`.** A module belongs in `crates/` if it does not depend on
  `app/` or on wgpu and is reusable by the daemon.
- **The daemon core is framework-agnostic.** `goble-daemon` holds execution logic; the composition
  roots (`app/` and `goblin-worker`) hold transport and process concerns.

### Registering a harness

Every harness becomes a `HarnessRuntime` (an object-safe trait) and is registered in a
`HarnessRegistry`, both in `goble-harness-runtime`. There are three paths:

- **Internal** — `goble-harness-internal` adapts `goble_core::Harness` to `HarnessRuntime`.
- **CLI subprocess** — `goble-harness-cli` drives an external process that speaks
  `goble-harness-protocol`. This is the path a third-party harness takes.
- **Remote** — over the headless daemon, through `goble-daemon-client`.

If a harness reports `HarnessCapabilities.reversible` and implements `snapshot` / `restore`, the
daemon captures a snapshot on settle and restores it on rewind. Otherwise it falls back to the
transcript, so reversibility still works.

### Getting oriented

- `docs/` holds the architecture documents: the reversible substrate
  ([`docs/substrate-architecture.md`](docs/substrate-architecture.md)), the security and deployment
  model ([`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)), and the file layout
  ([`docs/PROJECT_STRUCTURE.md`](docs/PROJECT_STRUCTURE.md)).
- `.agents/` is the design-docs tree: a numbered subsystem map where each folder's `README.md` is a
  short surface document and the leaf documents carry the detail.
  [`.agents/00-README.md`](.agents/00-README.md) is the index, [`.agents/TRACKER.md`](.agents/TRACKER.md)
  is the aggregated backlog, and `.agents/GUIDE.md` and `.agents/RESOLVER.md` describe the working
  contract.

### Good first issues

- **A harness adapter.** Implement `HarnessRuntime` for a new agent CLI, using `goble-harness-cli` and
  its fixture binary as the reference.
- **A UI widget.** Add a primitive to `goble-ui`, following the existing elements under
  `crates/goble-ui/src/elements/` and the preview example in `crates/goble-ui/examples/`.
- **A screen backend.** Add a capturer or controller behind the `ScreenCapturer` / `ScreenController`
  traits in `goble-screen-core`, with the macOS adapter in `goble-screen-adapter` as a model.
- **A voice backend.** Implement the `VoiceProvider` trait in `goble-voice`; the crate ships a mock
  provider and an `audio` feature that is off by default.

## Automate development

The full local gate is:

```sh
./scripts/fmt.sh
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets
./scripts/dev-ui.sh
```

Use `scripts/fmt.sh`, not `cargo fmt --all`. The `--all` flag also formats local
path-based dependencies, which pulls in `vendor/alacritty_terminal`; rustfmt would rewrite that
third-party source and break the byte-identical provenance that `scripts/fetch-alacritty-terminal.sh`
checks. The script names the workspace members explicitly so `vendor/` is never touched.

`dev-ui.sh` is the only step that launches a window; the rest are non-interactive and safe to run
from a hook. A `pre-commit` hook that runs the checks looks like this:

```sh
#!/usr/bin/env sh
set -eu

./scripts/fmt.sh
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Save it as `.git/hooks/pre-commit` and mark it executable with `chmod +x .git/hooks/pre-commit`.

CI lives in [`.github/workflows/release.yml`](.github/workflows/release.yml). It is triggered
manually, and it builds and packages the release artifacts for the headless worker on both
`x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`. The same job runs the formatting, check,
and test gates (`./scripts/fmt.sh --check`, `cargo check --workspace --all-targets`,
`cargo test --workspace`) before publishing a release.

The terminal emulator core is vendored under `vendor/alacritty_terminal` and excluded from the
workspace. To refresh or verify it, run:

```sh
./scripts/fetch-alacritty-terminal.sh
```

With no argument the script downloads the published crate, checks its SHA-256 against the recorded
value, diffs it against `vendor/`, and reports any difference. Passing a destination directory leaves
the extracted crate there so its upstream test suite can be run as a differential oracle, and
`--install` restores `vendor/` from the verified archive.

## Licensing

Goble is licensed under the MIT license. The full text is in [`LICENSE`](LICENSE).

One third-party component is vendored as source: `alacritty_terminal` 0.26.0, the VT emulator core,
under the Apache-2.0 license. It lives in `vendor/alacritty_terminal/`, is excluded from the
workspace, and is used by `goble-terminal`. Its provenance, the checksum that pins it, and the
changes made to the published archive are documented in
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md), along with the notice text the Apache-2.0 license
requires us to reproduce.

To refresh the vendored source, re-extract the new archive into `vendor/alacritty_terminal`, update
the version and SHA-256 in `THIRD-PARTY-NOTICES.md`, and run
`./scripts/fetch-alacritty-terminal.sh` followed by `cargo test -p goble-terminal`. The dependency
notice list itself is maintained by hand from the dependency tree; `Cargo.lock` is the authoritative
list of everything that is linked.

## Open source and contributing

Contributions are welcome. The flow is the usual one:

1. Fork the repository and create a branch from `main`.
2. Make a focused change. Keep unrelated refactors out of the same pull request.
3. Run the local gate described under [Automate development](#automate-development).
4. Open a pull request against `main`.

Commit subjects follow the conventional-commit style already used in the history, for example
`feat(native): wire settings view with real state`, `fix(native): correct text rasterization`,
`docs(agents): realign the native UI roadmap`, and `chore: fix cargo clippy warnings`. Prefixes in
use include `feat`, `fix`, `docs`, `refactor`, `chore`, and `test`.

Pull requests are reviewed for scope and for whether the change respects the layering rules above.
The formatting, check, and test gates must pass. New behaviour should come with a test; the workspace
has unit tests throughout and integration tests in `app/src/integration_testing/`.

Goble's own code is MIT. Some projects we read for reference are not. In particular, `warp-new` is
AGPL-3.0-only and is used as a read-only architectural reference: no code from it is copied, linked,
or vendored anywhere in this repository. This applies to any other AGPL-licensed source as well. When
implementing behaviour studied from such a project, write the implementation yourself and cite the
mapping in the relevant design document.

## Support and questions

- **Bugs and feature requests** — open an issue on GitHub.
- **Questions and open-ended discussion** — use GitHub Discussions.

### Reporting a crash

Goble writes local crash reports under `~/.goble/crashes/`, one JSON file per crash. The directory is
kept whether or not uploading is enabled, so a report can always be attached manually. To report a
crash, attach the relevant file from that directory to an issue, together with what you were doing
when it happened.

A development build never uploads anything; a released build reports by default. Either way, turning
it off takes one of these:

- `DO_NOT_TRACK=1` — the cross-project opt-out; it takes precedence over everything else.
- `GOBLE_TELEMETRY=0` — disables telemetry uploads for the process.
- The stored setting — set `"upload": false` in `~/.goble/telemetry.json`.

The public telemetry collector (`https://telemetry.goble.dev`) is not deployed yet. Until it is,
uploads are queued on disk rather than delivered. This is why attaching a crash report to an issue is
the reliable path today.

## Open source dependencies

Goble builds on the following projects. Each entry lists the upstream license and the reason the
dependency is present.

| Dependency | License | Why it is here |
|---|---|---|
| [`wgpu`](https://crates.io/crates/wgpu) 25 | MIT OR Apache-2.0 | Cross-platform GPU rendering for the UI and terminal |
| [`winit`](https://crates.io/crates/winit) 0.30 | MIT OR Apache-2.0 | Window creation and the event loop |
| `alacritty_terminal` 0.26.0 (vendored) | Apache-2.0 | The VT emulator core and escape-sequence parser; see [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) |
| [`tokio`](https://crates.io/crates/tokio) 1 | MIT | Async runtime across the daemon, worker, and client |
| [`axum`](https://crates.io/crates/axum) 0.8 | MIT | HTTP server for the daemon and worker |
| [`tower-http`](https://crates.io/crates/tower-http) 0.6 | MIT | CORS and tracing middleware for the HTTP layer |
| [`serde`](https://crates.io/crates/serde) / [`serde_json`](https://crates.io/crates/serde_json) | MIT OR Apache-2.0 | Serialization for the wire protocols, config, and persisted state |
| [`rusqlite`](https://crates.io/crates/rusqlite) 0.34 | MIT | SQLite access for checkpoints, projects, sessions, and tasks |
| [`reqwest`](https://crates.io/crates/reqwest) 0.12 + [`rustls`](https://crates.io/crates/rustls) | MIT OR Apache-2.0 (reqwest); Apache-2.0 OR ISC OR MIT (rustls) | Outbound HTTPS without a system TLS dependency |
| [`regex`](https://crates.io/crates/regex) 1.11 | MIT OR Apache-2.0 | Text matching in the parser and redaction paths |
| [`ring`](https://crates.io/crates/ring) 0.17 | Apache-2.0 OR ISC | Ed25519 verification of release manifests, so an update is only installed if it was signed |
| [`sha2`](https://crates.io/crates/sha2) / [`hex`](https://crates.io/crates/hex) | MIT OR Apache-2.0 | Artifact digests and crash report fingerprints |
| [`semver`](https://crates.io/crates/semver) 1 | MIT OR Apache-2.0 | Comparing a running version against a released one |
| [`uuid`](https://crates.io/crates/uuid) 1 | Apache-2.0 OR MIT | Identifiers for projects, sessions, tasks, and installations |
| [`chrono`](https://crates.io/crates/chrono) 0.4 | MIT OR Apache-2.0 | Timestamps in records and the checkpoint store |

The complete, exact list of linked crates and their versions is `Cargo.lock`. [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md)
carries the notice text that the licenses require us to reproduce.
