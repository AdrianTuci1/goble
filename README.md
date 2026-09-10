# Goble — Reversible execution substrate

Goble is a **runtime substrate** for agentic work, in the
[Shepherd](https://github.com/shepherd-agents/shepherd) zone: it turns an agent's execution into a
**reversible, Git-like trace** that meta-agents can observe, fork, replay, and revert. We are not
building "yet another agent" — we build a runtime + interaction layer onto which you attach your
**own harness** (*bring-your-own-harness*): ours, Claude Code, Codex, or any agent CLI.

The three moats (differentiators against competitors):

1. **Reversibility** — execution is a Git-like trace: observe, fork, replay, revert.
2. **Interaction** — live voice (STT→TTS) and screen (broadcast + computer-use).
3. **Per-project observability** — for each directory you see what's running and what isn't.

Dogfooding criterion: if the founder uses it daily on his own projects, it's valid.

---

## What Goble is

| | |
|---|---|
| **It is not** | "yet another agent" / a proprietary harness |
| **It is** | a runtime + interaction layer for agentic work, with pluggable harnesses |
| **Structure** | follows Shepherd: workspace → task (signature = permission surface) → run (durable, inspectable trace) → effect/changeset → scope → grant → trace (fork/replay/revert) → settlement |
| **Goble's pluses** | BYOH/harness-agnostic, voice + screen, per-project observability, normal remote deploy, medium selector |

The positioning difference from Shepherd: Shepherd is coupled to a specific agent; Goble presents
reversibility **for any harness**, via the transport seam (`goble-harness-protocol`) and the
harness's reversibility contract (`snapshot`/`restore`, opt-in).

### Domain concepts

| Primitive | Meaning |
|---|---|
| **Medium** | which daemon listens — Local / VM / RemoteXrdp / Browser / Container (toggling between media) |
| **Project** | a directory: the unit of observability and isolation |
| **Session** | a worktree / working session inside a project |
| **Task** | a harness invocation with grants (`ReadOnly`/`ReadWrite`) and a trigger (manual/cron/http/heartbeat) |

"Agent" is no longer a first-class entity — a harness configured with scheduled tasks suffices;
per-project observability makes agents redundant.

---

## Quickstart

The repo is a single Cargo workspace. Build + tests:

```bash
# Format and check the whole workspace
cargo fmt --all
cargo check --workspace --all-targets

# Run tests
cargo test --workspace
```

**Run the native UI** (the product shell, `app/` = `goble-app`, on `goble-ui` wgpu/winit):

```bash
./scripts/dev-ui.sh
```

`dev-ui.sh` builds and runs `goble-app`; with `cargo-watch` installed it rebuilds and restarts the app
on any change in `app/` or `crates/`. There's no live hot-reload — editing any `app` or `crates`
source requires a normal rebuild.

### Attach a harness

Any harness becomes a `HarnessRuntime` (object-safe) and is registered in `HarnessRegistry`:

- **internal** — `goble-harness-internal` wraps `goble_core::Harness`.
- **CLI / BYOH** — `goble-harness-cli` drives an external subprocess speaking `goble-harness-protocol`.
- **remote** — over the headless/remote `goble-daemon-client`.

If the harness is reversible (`HarnessCapabilities.reversible` + `snapshot`/`restore`), the daemon
captures the snapshot on settle and restores it on `rewind`; otherwise it falls back to
transcript-only. So reversibility works with **any** harness.

---

## Architecture (short)

```
GUI (app/, thick-app on goble-ui)
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

- **Types rule**: `types ← protocol ← runtime`.
- **Two composition roots, one core**: daemon **embedded** (in-process, in the GUI binary — where the
  local voice and screen actually run) and daemon **headless/remote** (`goblin-worker` on a VPS, over
  mTLS WS). The GUI doesn't know where the daemon runs; the "medium" is which daemon listens.
- **Reversibility + persistence**: `goble-replay` (per-turn ledger) + `goble-persistence`
  (`CheckpointStore`, SQLite) — the reversible history survives a restart.

Architecture docs:

| Doc | What it covers |
|---|---|
| [`docs/substrate-architecture.md`](docs/substrate-architecture.md) | reversible execution substrate (Shepherd structure + Goble's pluses) |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | P2P security/deploy: identity, mTLS, roles |
| [`docs/PROJECT_STRUCTURE.md`](docs/PROJECT_STRUCTURE.md) | workspace file structure |

---

## Project layout

```
app/                       GUIPRODUCT shell (goble-app, native wgpu/winit) → thin client over daemon
crates/
  goble-harness-*          BYOH seam: types ← protocol ← runtime (+ internal/cli/foreign)
  goble-daemon-*           core + seam: goble-daemon, goble-daemon-protocol, goble-daemon-client
  goble-replay             reversibility substrate (ledger + fork/replay/revert)
  goble-persistence        durable store (SQLite) for checkpoints
  goble-voice              voice: STT (StreamingSttEvent) + TTS, `audio` feature
  goble-sandbox            profile-driven isolation (Seatbelt/Landlock/seccomp/bwrap)
  goble-workflow           scripted engine + journaled WorkflowHostRequest
  goble-screen-core        screen abstractions: capture + control (broadcast / computer-use)
  goble-core               existing core (store, identity, protocol, mcp, provision, worker…)
  goblin-worker            headless/remote daemon (composition root on a VPS)
  goble-cli                CLI utility for worker operations
  goble-ui                 wgpu/winit widget library (WarpUI-like)
deploy/                    goblin Dockerfile + goblin-cluster Helm chart
scripts/                   dev-ui.sh, install-goblin.sh, release.sh
```

> `crates/goble-desktop` (Tauri/React/Rust) is **legacy** — `exclude`d from the workspace, being
> migrated; the product shell is `app/`.

---

## Development

```bash
# Build + tests
cargo check --workspace --all-targets
cargo test --workspace

# Native UI
./scripts/dev-ui.sh
```

## License

MIT — see `LICENSE`.
