# Goble — Reversible execution substrate (the "Shepherd zone" architecture)

This document describes Goble as a **reversible execution substrate**, not "yet another agent". The
structure follows [Shepherd](https://github.com/shepherd-agents/shepherd) (a runtime that turns an
agent's execution into a reversible, Git-like trace), but **with Goble's differentiators added**:
*bring-your-own-harness*, interaction (voice + screen), and per-project observability.

The **security/deploy** document (P2P cluster, identity, mTLS, roles) stays in
[`ARCHITECTURE.md`](ARCHITECTURE.md); here we cover the *reversible execution* property.

---

## 1. Thesis / positioning

> **"An observable and controllable substrate for agentic work, where you see and steer the work
> per-project, in any medium and with voice."**

We do not own the harness. Goble provides a **runtime + interaction layer** onto which anyone attaches
their **own harness** (ours, Claude Code, Codex, or any agent CLI). From this follow the three moats
(differentiators against competitors):

1. **Reversibility** — execution is a Git-like trace: observe, fork, replay, revert.
2. **Interaction** — live voice (STT→TTS) and screen (broadcast + computer-use).
3. **Per-project observability** — for each directory you see what's running and what isn't.

Dogfooding criterion: if the founder uses it daily on his own projects, it's valid.

---

## 2. Structure after Shepherd (concept mapping)

Shepherd structures agentic work around these concepts:

| Shepherd concept | Meaning | How we mirror it in Goble |
|---|---|---|
| **workspace** | a directory turned into a working space | **Project** (directory) + **Session** — the working worktree the work runs on |
| **task** | the function signature is the permission surface | a harness invocation: `Grant`/`GrantMode{ReadOnly,ReadWrite}` on declared repos; the contract lives in `goble-harness-types` and is checked in `goble-harness-protocol` |
| **run** | durable, inspectable execution | `DaemonState` + `goble-replay` (per-turn `TurnRecord` ledger, `ExecutionRecord`/events), surfaced through `DaemonPort` |
| **effect / changeset** | retained output (a proposal), before applying | transcript + `Checkpoint`/snapshot per turn; reviewable before *settlement* (apply/rewind/replay) |
| **scope** | the boundary of a binding | **Medium** (Local / VM / RemoteXrdp / Browser / Container) + named bindings |
| **grant (permission)** | permissions compiled into writable roots | `Grant`/`GrantMode` + `goble-sandbox` (`SandboxProfile`/`SandboxLevel`, Seatbelt/Landlock/seccomp/bwrap jail) |
| **trace** | Git-like trace: fork / replay / revert | `DaemonPort::{rewind,fork,replay,checkpoints}` + `goble-replay`; transcript-only rewind plus optional *env restore* |
| **settlement** | select / apply / release / discard | transcript settlement primitives + the harness's `snapshot`/`restore` reversibility contract |
| **env copy-on-write fork** | COW fork ~5x faster than docker commit | *[target]* — not implemented yet; the optimization / future component path |

The architectural rule we keep from Shepherd: **execution is a reversible, inspectable artifact**, not
a "black box". The positioning difference: Shepherd is coupled to a concrete agent (e.g. Claude);
Goble presents this behavior **for any harness**, through the reversibility seam.

---

## 3. Goble's pluses over Shepherd

| # | Plus | Detail | Status |
|---|---|---|---|
| 1 | **BYOH / harness-agnostic** | Reversibility is injected by the daemon from the event stream any harness already emits — the transcript is reversible for all; *env restore* is optional via `HarnessRuntime::snapshot`/`restore` (default `None`). On a BYOH substrate, task-signature permissions (`ReadOnly`/`ReadWrite` grants per repo) are the only safe path to "the agent controls the medium". | **implemented** |
| 2 | **Interaction — voice** | Streaming STT (client + `StreamingSttEvent`) + TTS, `audio` feature (off by default) so it compiles without native audio deps. Voice is a medium interaction channel (*relayable* through the seam), not a separate feature. | **model implemented**; native backend `[target]` |
| 3 | **Interaction — screen** | Two opposite directions (in a single concept you lose half): **broadcast** (emit your live screen — observability/demo) and **computer-use** (the agent controls input/screen). | **broadcast capture implemented** (`goble-screen-core` + `goble-screen-adapter` macOS `screencapture` + app/ screen UI); SDK + computer-use *control* adapter `[target]` |
| 4 | **Per-project observability** | Per directory: what's running, what's scheduled, which session is active, what finished. This is the **primary** panel, not a secondary one. "Agent" degrades to a run config. | **implemented**: daemon records carry `project_id`/`medium_id`; durable `projects`/`sessions`/`tasks`; app/ `projects` panel |
| 5 | **Normal remote deploy** | **Headless/remote** daemon on a VPS/K8s over mTLS WebSocket + **embedded** daemon in-process in the GUI binary. Two composition roots, one `DaemonPort`. | **implemented** (`goblin-worker` and `DesktopState` both drive `goble-daemon`; headless now gets reversibility + `run_workflow`) |
| 6 | **Medium selector** | An agent-controllable GUI substrate (warp-terminal style); toggle between media (local/VM/xrdp/browser). | **implemented** (app/ medium selector maps the chosen medium onto `HarnessTurn.medium_id`/`project_id`) |
| 7 | **Share credentials / video without downloading** | Share Chrome cookies / open the remote xrdp window so the agent can grab credentials; watch testing videos without local download. | **target** |

---

## 4. Technical architecture (layering)

```
GUI (app/, thick-app on goble-ui wgpu/winit)
        │  thin client
        ▼
   DaemonClient (goble-daemon-client)      ── in-process  or  over mTLS WebSocket
        │
        ▼
   DaemonPort (goble-daemon)               ← reusable, framework-agnostic core
        │  transport seam
        ▼
   goble-harness-protocol (BYOH seam, JSON)  →  HarnessRuntime (internal | cli subprocess | remote)
```

- **Types rule**: `types <- protocol <- runtime` — model types (`goble-harness-types`, `goble-voice`,
  `goble-workflow`, `goble-screen-core`) don't depend on transport; protocol (wire) and runtime
  (trait + registry) follow in turn.
- **Thick-app rule**: a module goes into `crates/` if it doesn't depend on `app/`/`wgpu`. `crates/*`
  never depends on `app/`.
- **Feature flags** (`FeatureFlag` + cargo features) for heavy subsystems (audio, sandbox backends,
  screen platform), keeping the common-path build light.

### Two composition roots, one core

```
   embedded (in-process, in the GUI binary)    headless / remote (goblin-worker on a VPS)
   DesktopState / daemon_state                 AppState / worker.scheduler
        └──────────────┬──────────────────────────────┘
                       ▼
   DaemonPort (goble-daemon): RunHarness · Resume · Cancel · ListHarnesses ·
             Snapshot · Rewind · Fork · Replay · Checkpoints
```

The GUI **doesn't know** whether the daemon runs locally (where the *local* voice and screen live) or
on a VPS; the "medium" is just **which daemon listens**.

---

## 5. Capabilities (crates per concern)

| Crate | Layer | Role | Status |
|---|---|---|---|
| `goble-harness-types` | types | `HarnessId`/`ProjectId`/`SessionId`/`MediumId`, `HarnessTurn`, `Grant`/`GrantMode`, `InteractionHint`, `HarnessCapabilities` (+`reversible` flag), `HarnessSnapshot` | ✅ |
| `goble-harness-protocol` | protocol | BYOH seam: `HarnessClientRequest`/`HarnessServerEvent`/`HarnessMessage` (JSON); `Checkpoint`/`Restore` | ✅ |
| `goble-harness-runtime` | runtime | object-safe `HarnessRuntime` trait (+`snapshot`/`restore` default `None`), `HarnessRegistry`, `MockHarness`, `ReversibleHarness` | ✅ |
| `goble-harness-internal` | runtime | adapter `goble_core::Harness` → `HarnessRuntime` | ✅ |
| `goble-harness-cli` | runtime | BYOH adapter: external subprocess speaking `goble-harness-protocol` | ✅ |
| `goble-foreign-harnesses` | discovery | Claude/Codex/Cursor sessions (read-only, parse-tolerant) | ✅ |
| `goble-voice` | types | `AudioChunk`, `StreamingSttEvent`, `SttClient`/`TtsClient`, `VoiceProvider` trait + `MockProvider`; `audio` feature | ✅ |
| `goble-sandbox` | runtime | `SandboxProfile`/`SandboxLevel`, `Sandbox` trait + `NoopSandbox`, `unix-{seatbelt,landlock,seccomp,bwrap}` backends; real `bwrap` subprocess backend for Hardened profiles (flags enforced, no `unsafe`) | ✅ |
| `goble-workflow` | types | `Workflow`/`Step`/`Trigger`, `WorkflowEngine`, `WorkflowRun`, `WorkflowHostRequest`, `MockStep`; wired into the daemon via `StepExecutor` + `WorkflowHost` + `DaemonPort::run_workflow` | ✅ |
| `goble-replay` | runtime | per-turn transcript ledger + rewind/revert/fork/replay + settlement (`select`/`apply`/`release`/`discard`); wired into the daemon | ✅ |
| `goble-persistence` | runtime | `CheckpointStore` (SQLite) implementing `CheckpointSink`; `projects`/`sessions`/`tasks` entities; seeds via `~/.goble/checkpoints.sqlite` | ✅ |
| `goble-screen-core` | types | `ScreenFrame`, `ScreenCapturer`/`ScreenController`, `ScreenRegistry`, mocks | ✅ |
| `goble-screen-sdk` | runtime | connection + server & harness entry points | `[target]` |
| `goble-screen-adapter` | runtime | local capture: macOS `screencapture` → RGBA8 `ScreenFrame` (via `image`), synthetic/mock elsewhere; factory + `ScreenRegistry` wired into `DesktopState` | ✅ |
| `goble-daemon-protocol` | protocol | GUI↔daemon seam: `DaemonRequest`/`DaemonEvent`/`DaemonMessage` | ✅ |
| `goble-daemon` | runtime | `DaemonPort`, `DaemonState`, harness registry, execution ledger (`project_id`/`medium_id`), `DaemonEventSink`, `CheckpointSink`, `run_workflow` | ✅ |
| `goble-daemon-client` | runtime | `DaemonClient` (in-process `InProcessClient` / over WS `WebSocketClient`) | ✅ |

---

## 6. Domain model (4 primitives)

| Primitive | Meaning |
|---|---|
| **Medium** | which daemon listens (Local / VM / RemoteXrdp / Browser / Container) — the medium toggle |
| **Project** | a directory: the unit of observability and isolation |
| **Session** | a working worktree/session inside a project |
| **Task** | a harness invocation with grants (`ReadOnly`/`ReadWrite`) and a trigger (manual/cron/http/heartbeat) |

"Agent" is **no longer a first-class entity** — a harness configured with scheduled tasks (via
`goble-workflow`) suffices. Per-project observability (<Medium, Project, Session, Task> +
"what's running / what isn't") makes agents redundant.

---

## 7. Reversibility (detail)

- The daemon drives any harness; from the **event stream** it already emits, `goble-replay` keeps a
  **per-turn transcript ledger** (index-aware), so reversibility works with **any harness**, not a
  specific one.
- **Transcript-only rewind** is always available. **Env restore** is optional: if
  `HarnessCapabilities.reversible` is true and `HarnessRuntime::snapshot` returns `Some`, the daemon
  captures the snapshot on settle and applies it on `rewind` via `restore`; otherwise it falls back
  to the transcript. This keeps the contract **harness-agnostic**.
- **Durable persistence**: on settle, the per-turn checkpoint is written through `CheckpointSink` →
  `goble-persistence` (`CheckpointStore`), and on startup the ledgers are seeded from
  `~/.goble/checkpoints.sqlite`, so the reversible history survives a restart.

**Verified** (green tests in `goble-daemon`): `settled_turn_records_environment_snapshot_when_reversible`,
`settled_turn_records_none_when_not_reversible`, `rewind_restores_environment_from_snapshot_when_reversible`,
`rewind_stays_transcript_only_without_a_snapshot`, plus `reverse_a_multi_turn_transcript`.

---

## 8. Current status

**Implemented and green (`cargo check --workspace --all-targets`):**
- The harness seam (types/protocol/runtime) + the internal/cli/foreign adapters.
- The daemon core (`DaemonPort`/`DaemonState`) + full reversibility in `goble-daemon` (snapshot
  capture on settle + `restore` on rewind + transcript-only fallback) + persistence via `CheckpointStore`.
- The model capability crates: `goble-voice`, `goble-sandbox`, `goble-workflow`, `goble-screen-core`.

**Target (the next phases of the plan):** substrate + reversibility (done) → per-project observability
(data + UI) → voice (native backend) → screen (SDK/adapter). Order matters: interaction (voice,
screen) and observability are the unfinished and most expensive parts (2–3× the harness work).

---

## 9. Positioning vs. Grok-workflows

Grok-workflows = **authoring** orchestration at build-time; Goble = **runtime deployed** orchestration.
Shepherd is the orthogonal execution-*representation* layer. Goble sits "in the Shepherd zone", but
its differentiators (BYOH + interaction + per-project observability) are what make it useful.
