# Goble — Project structure

This document describes the file structure of the Goble workspace. It reflects the **current** state
and marks the **target additions** (new crates and domain modules in `app/`) according to the
grok-build architectural approach (small crates per concern) + warp-new (thick-app).

> Conventions: `[target]` = doesn't exist yet, to be created. `[refactor]` = exists, but is being
> extracted/restructured.

---

## Workspace layout (recap)

```
goble/
├─ Cargo.toml                 # workspace: members = ["app", "crates/*"]; exclude goble-desktop
├─ app/                       # GUIPRODUCT shell (goble-app, native wgpu/winit) → thin client over daemon
├─ crates/                    # composable crates; the daemon core is framework-agnostic
├─ deploy/                    # goblin Dockerfile + goblin-cluster Helm chart
├─ scripts/                   # dev-ui.sh, install-goblin.sh, release.sh
├─ docs/                      # ARCHITECTURE.md, user-guide, plans/, design/, PROJECT_STRUCTURE.md
├─ CHANGELOG.md / TEST_REPORT.md / README.md / LICENSE
```

---

## `app/` — GUI (thick-app, presentation only)

The GUI is a **thin client**: view snapshots, actions, element tree. It contains no execution
semantics. It talks to the daemon across a boundary (`DaemonClient`).

| File | Role |
|---|---|
| `main.rs` | entry point: builds `App`, keeps it on the tokio runtime, opens `RootView` |
| `lib.rs` | re-export / wiring |
| `root_view.rs` | scene root; owns `UiState`, `AiState`, `desktop` (daemon), `event_bus`; each frame: `drain_events()` → `UiSnapshot`/`AiSnapshot` → `build_ui` |
| `runtime.rs` | runtime orchestration: decides local/remote and drives `DaemonModel.run_turn` |
| `features.rs` | runtime feature flags (`FeatureFlag`, `init_feature_flags`, `feature_enabled`, `with_enabled`) |
| `daemon/` | daemon domain: `DaemonModel` (the app→daemon seam: local embedded or remote client) |
| `state.rs` | `UiState` (the model owned by app): `from_desktop`, `refresh_*`, `mock` |
| `actions.rs` | `make_actions(state, desktop) -> UiActions` (`Rc<RefCell<dyn FnMut..>>` closures) |
| `ai/` | AI domain: `state.rs` (`AiState`), `actions.rs` (`make_ai_actions`), `mod.rs` — **domain template** |
| `ui/mod.rs` | `UiSnapshot`, `AiSnapshot`, `UiActions`, `AiActions`, `enum AppTab { Threads, Chat, Settings }`, `build_ui(...)` |
| `ui/shell.rs` | `build_topbar`, `build_main`, `build_settings_disabled`, `SidebarLayout` |
| `ui/sidebar.rs` | `build_sidebar` (search + conversation list + plugins footer) |
| `ui/chat.rs` | `build_agent_chat`, `build_agent_header`, right chat-sidebar (Computer Use + Routines) |
| `ui/crons.rs` | `build_crons_drawer`, `build_cron_row` — **template for "what's running"** |
| `ui/connectors.rs` | `build_connectors_sheet`, `build_install_drawer` — **domain panel template** |
| `ui/vault.rs`, `ui/model_form.rs` | sheets/dialogs |
| `integration_testing/` | headless UI tests (not browser): `ui_render`, `chat_flow`, `cron_flow`, `vault_flow`, `connector_flow`, `first_run_flow`, `harness_flow`, `projects_flow`, `screen_flow`, `common` |

### New domains in `app/` (warp-new pattern: one domain = `app/src/<domain>/`)

```
app/src/projects/    # [implemented] per-directory observability: projects + sessions + "what's running"
app/src/media/       # [implemented] medium selector (Local / VM / RemoteXrdp / Browser / Container)
app/src/screen/      # [implemented] broadcast window + computer-use mode
app/src/voice/       # [model-only] microphone + live STT→TTS transcript (kept on hold)
```

Each follows the `ai/` model: `state.rs` + `actions.rs` + `mod.rs`; in `ui/mod.rs` add `XSnapshot` +
`XActions`; in `root_view.rs` wire up `x_state` + an arm in `drain_events`.

---

## `crates/` — the core (daemon + shared types)

Thick-app rule: a module goes into `crates/` if it **does not depend on `app/`/wgpu** and is reusable
(or the daemon uses it). `crates/` never depends on `app/`.

### Existing

| Crate | Role |
|---|---|
| `goble-core` | the central engine: `agent`, `harness`, `reasoning`, `execution`, `store`, `workflow`, `protocol`, `mcp_*`, `identity`, `cluster_key`, `provision`, `snapshot`, `vault`, `secret`, `worker`, `worker_pool`, `workspace`, `thread`, `agent_memory`, `app_home`, `crypto`, `tls`, `config`, `task`, `llm`, `isolate`, `audit`, `principal`, `user`, `device_transfer`, `docs` |
| `goblin-worker` | the **remote/headless** daemon (composition root): drives agent execution through `goble-daemon` (`DaemonPort`/`DaemonState` + a registered `gooble-harness-internal::InternalHarness`), so it inherits reversibility (`rewind`/`fork`/`replay`/`checkpoints`/`select`/`apply`/`release`/`discard`) + `run_workflow` + the harness registry; keeps its operational shell: `websocket` (mTLS WS), `state`, `task_store`/`scheduler`, `pairing`, `snapshot_runner`, `llm_factory`, `mcp`, `leader`, `file_vault`, `agent_memory` |
| `goble-cli` | CLI utility for worker operations |
| `goble-desktop-service` | **embedded daemon** `DesktopState` + `event_bus` + `thread_store` + `worker_manager` + `ssh_installer` → `[refactor]` the core is extracted into `goble-daemon`; `DesktopState` is now the composition root driving a `DaemonClient` |
| `goble-desktop-native` | backend-integration reference (state_api + views); **not** the product shell |
| `goble-desktop` | the **legacy Tauri/React/Rust** frontend (`exclude`d from the workspace) — being migrated |
| `goble-ui` | the wgpu/winit widget library (the WarpUI equivalent): `elements/` (67 widgets), `views/`, `platform/` (`app`, `mac`, `linux`, `windows`, `wgpu_render_engine`, `text_atlas`, `icon_atlas`), `theme`, `style`, `render`, `scene`, `event`, `geometry` |
| `goble-harness-types` | `[new]` model-facing types for `HarnessTurn`/`Grant`/`InteractionHint`/`Schedule`/`HarnessSnapshot` + `HarnessCapabilities.reversible` flag (no transport) |
| `goble-harness-protocol` | `[new]` wire types (BYOH seam): `HarnessClientRequest`/`HarnessServerEvent`/`HarnessMessage`; reversible: `Checkpoint`/`Restore` requests + `Checkpoint` event |
| `goble-harness-runtime` | `[new]` execution contract: `HarnessRuntime` trait (object-safe) + `HarnessRegistry` + `MockHarness`; reversible: `snapshot()`/`restore()` (default `None`, opt-in) |
| `goble-daemon-protocol` | `[new]` GUI↔daemon seam: `DaemonRequest`/`DaemonEvent`/`DaemonMessage` (wire types, no transport) |
| `goble-daemon` | `[new]` the daemon core: `DaemonPort` (trait), `DaemonState`, harness registry, execution ledger, event sink, `CheckpointSink` (durable persistence); full reversibility: snapshot capture on settle + `restore` on `rewind` (transcript-only fallback) |
| `goble-daemon-client` | `[new]` the facade the GUI uses: `DaemonClient` (in-process `InProcessClient` or over WS `WebSocketClient`) |
| `goble-harness-internal` | `[new]` adapter: `goble_core::Harness` → `HarnessRuntime` |
| `goble-harness-cli` | `[new]` BYOH adapter: external subprocess speaking `goble-harness-protocol` (driven through `HarnessRuntime`) |
| `goble-foreign-harnesses` | `[new]` read-only discovery of Claude/Codex/Cursor sessions (jsonl, parse-tolerant) |
| `goble-replay` | `[new]` reversibility substrate: per-turn transcript ledger + rewind/revert + fork + replay + **settlement** (`select`/`apply`/`release`/`discard`); **wired into `goble-daemon`** via `DaemonPort` (`rewind`/`fork`/`replay`/`checkpoints`) + `DaemonRequest` in `goble-daemon-protocol`; persistable via `CheckpointSink` → `goble-persistence` |
| `goble-persistence` | `[new]` durable store (SQLite/rusqlite) for replay checkpoints + project entities: `CheckpointStore` (`save`/`load`/`list`/`delete`) implementing `CheckpointSink` + `projects`/`sessions`/`tasks` entities (upsert/list/load/delete); wired into `DesktopState` (runs on `~/.goble/checkpoints.sqlite`) |
| `goble-voice` | `[new]` voice capability (`types` layer): `AudioChunk`, `StreamingSttEvent`, `SttClient`/`TtsClient`, `VoiceProvider` trait + `MockProvider`; `audio` feature (off by default) |
| `goble-sandbox` | `[new]` profile-driven isolation: `SandboxProfile`/`SandboxLevel`, `Sandbox` trait + `NoopSandbox` (default-safe); `unix-{seatbelt,landlock,seccomp,bwrap}` backends; real `bwrap` subprocess backend for Hardened profiles (flags enforced, no `unsafe`) |
| `goble-workflow` | `[new]` scripted engine: `Workflow`/`Step`/`Trigger`, `WorkflowEngine`, `WorkflowRun`, `WorkflowHostRequest`, `MockStep`; framework-agnostic; wired into the daemon via `StepExecutor` + `WorkflowHost` + `DaemonPort::run_workflow` |
| `goble-screen-core` | `[new]` screen abstractions (`types` layer): `ScreenFrame`, `ScreenCapturer`/`ScreenController`, `ScreenRegistry`, mocks; `platform` feature |
| `goble-screen-adapter` | `[new]` local capture adapter: macOS `screencapture` → RGBA8 `ScreenFrame` (via workspace `image` crate), synthetic/mock elsewhere; factory + `ScreenRegistry` wired into `DesktopState` |
| `goble-screen-sdk` | `[new]` **remote** screen source (`runtime` layer, `rdp` feature gate): IronRDP-backed `RdpRemoteSource::connect` decodes a real xrdp/RDP desktop into an RGBA8 `ScreenFrame` and drives input through a `ScreenController` (`click`/`type_text`/`scroll`); registers into the same `ScreenRegistry`, so a remote source is handled identically to the local adapter |

### Screen handoff (agent → live desktop)

```
open_screen tool (goble-core) → HarnessServerEvent::ScreenHandoff (goble-harness-protocol)
  → DaemonEvent::ScreenHandoff (goble-daemon → GUI)  → DesktopState::open_remote_screen (goble-desktop-service, `remote-screen` feature)
  → goble-screen-sdk::RdpRemoteSource::connect registers capturer+controller in the screen registry
  → app renders the live stream (`FrameView`) and routes input back through the controller
```

---

## Daemon: two composition roots, one core `[refactor]`

```
            GUI (app/)                     ──thin client──►  DaemonClient (goble-daemon-client)
                 │                                         (in-process OR over mTLS WX)
                 │                                          DaemonPort (goble-daemon)
                 ▼
      ┌─────────────────────────── Daemon ---------------------------------─┐
      │  goble-daemon (framework-agnostic):                                 │
      │    state · harness-registry · scheduler · executions                │
      │    + capability crates (voice/screen/replay/workflow/sandbox)       │
      └─────────────────────────────────────────────────────────────────────┘
        │ embedded (in-process, in the GUI binary)   │ headless/remote (goblin-worker on a VPS)
```

- **`goble-daemon`** = the reusable core, without `wgpu`/`winit`. Compiled in BOTH:
  - daemon **embedded** — runs in-process in the GUI binary; the **local** functions live here
    (voice STT/TTS, local machine screen capture).
  - daemon **headless/remote** — `goblin-worker` on a VPS, over mTLS WebSocket.
- **The seam** = `DaemonPort`: the same set of commands/events, realized in-process or over WS.
  The GUI doesn't know whether the daemon is local or on a VPS — the "medium" is just which daemon listens.
- **What we unified**: `DesktopState` (embedded) and `goblin-worker`'s `AppState` (remote) both sit
  behind `DaemonPort`. Execution logic lives in `goble-daemon`; `goblin-worker` routes agent runs
  through it (gaining reversibility, `run_workflow`, and the harness registry) and keeps only its
  operational shell (transport, pairing, vault, scheduler, snapshot). `harness_runner` no longer
  re-implements the daemon run loop.

---

## `deploy/` — daemon distribution

```
deploy/goblin/
├─ Dockerfile
└─ charts/goblin-cluster/
   ├─ Chart.yaml
   ├─ values.yaml
   └─ templates/ (_helpers.tpl, secrets.yaml, service.yaml, serviceaccount.yaml, statefulset.yaml)
```

## `scripts/`

| File | Role |
|---|---|
| `dev-ui.sh` | build + run `goble-app` (with `cargo-watch` if present) |
| `install-goblin.sh` | worker install / provision |
| `release.sh` | release build |

## `docs/`

| File | Role |
|---|---|
| `PROJECT_STRUCTURE.md` | this document |
| `substrate-architecture.md` | reversible execution substrate (Shepherd structure + Goble's pluses) |
| `ARCHITECTURE.md` | P2P cluster, identity, mTLS, roles |
| `agent-runtime.md` | the original minimal worker runtime |
| `local-remote-agent-runtime.md` | the "brain local, hands remote" architecture |
| `local-remote-agent-graph.md` | graph-of-runtime (local/remote) |
| `agent-orchestration-ui.md` | agent orchestration UI |
| `ui-library.md` | UI primitive catalog |
| `plans/` | work plans (see the plan names) |
| `design/` | design tokens (goble-tokens.json) + generation scripts |

> The user-guide ships with `goble-core/assets/user-guide/` (18 topical files).
