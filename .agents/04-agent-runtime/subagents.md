# 04 — Sub-agents (routines)

**Status:** `[~]` designed, not implemented: the model, the surfaces and the bounds are settled here; wave 12 of the agent-UI rollout builds them.
**Owns:** spawning short-lived agents to run routines without blocking the main chat.
**Depends on:** [`README.md`](README.md); the lifecycle reaches the app through the same wire pattern as [`../06-renderer/agent-tui.md`](../06-renderer/agent-tui.md) §5, and the chrome that shows it is that doc's §11.

## Problem

The user keeps typing while the agent works. If the harness blocks the main chat loop to run a long task, the conversation stalls. Routines (a review, a search, a build) should run as **sub-agents** that report back, so the main chat stays alive.

## Model

- **Main agent** = the persona you talk to; owns the conversation.
- **Sub-agent** = a disposable agent spawned for one routine. It gets its own prompt/context (derived from the main agent), its own CWD, runs to completion, and returns a result to the main agent.
- The main agent can spawn several sub-agents. Depth is **bounded**: past the limit the spawn tool is simply not in the child's tool set, so a runaway tree is impossible by construction rather than by counting.

This is the specification the `SubAgent` model item implements (`crates/goble-core/src/subagent.rs`):

```rust
struct SubAgentSpec {
    id: SubAgentId,
    parent_chat_id: ChatId,          // the conversation that spawned it
    parent_call_id: String,          // the tool call that asked for it
    description: String,             // one line, shown in the row and the overlay
    subagent_type: String,           // resolves the prompt/tool set (see Reuse)
    prompt: String,
    cwd: PathBuf,                    // a subdir of the workspace root, per child
    run_in_background: bool,
    depth: u32,
    budget: SubAgentBudget,
}

struct SubAgentBudget { max_turns: u32, max_tool_calls: u32, max_tokens: u64 }

enum SubAgentStatus {
    Initializing,
    Running { turns: u32, tool_calls: u32, tokens: u64, activity: String },
    Completed { output: String, duration_ms: u64, turns: u32, tool_calls: u32, tokens: u64 },
    Failed  { error: String },
    Cancelled { reason: String },
}

struct SubAgentRecord { spec: SubAgentSpec, status: SubAgentStatus, started_at, finished_at }
```

The budget's charge API is the load-bearing part: exhaustion is a **terminal outcome**, never a loop and never a hang. A child that runs out of turns, calls or tokens ends `Failed` with the reason, exactly as if a tool had errored.

## Why sub-agents (not just tools)

- They have their **own context/memory** for the routine, so a long task doesn't bloat the main transcript.
- They run **concurrently**, so the main chat keeps accepting input.
- They are **visible**: a row in the parent transcript with a status and an elapsed time, a full-screen view of the child's own transcript, and a place in the work overlay — never a silent background task.

## Concurrency and cancellation

- A **foreground** spawn (`run_in_background: false`) is the model's own request for the result: the tool call awaits the child and returns its output. The parent's turn is parked, and the footer says `Waiting on subagent…`.
- A **background** spawn returns to the model immediately with the child's id while the child keeps going as a `tokio` task. Whether a finished background child may wake the parent is a policy decision made at completion, not a promise: if auto-wake is not implemented, the record is still there to be read and the overlay still shows it. Say which was built rather than implying the other.
- Cancellation is real and reaches every child: the parent's existing `Harness` cancel bit stops the children, and a single child can be killed on its own from the overlay. A child that panics becomes `Failed`, never a hang.

## Surfaces

All four live in the renderer; the first three are this doc's items, the fourth is [`../06-renderer/agent-tui.md`](../06-renderer/agent-tui.md) §11.

- **The transcript row** (S5): one ruler row — a status bullet, the description, and while running the activity and elapsed time — folded by default like every other tool call, so a sub-agent is a quiet row first. Its data comes from the live record, not from the tool's arguments.
- **The child view** (S6): the row's click opens the child's *own* conversation in the pane, through the same block-list/filter mechanism the agent view already uses (A7) — the child's card in the list, the pane's filter on the child — under a framed title naming the child's type, description, status and elapsed time; Esc returns to the conversation the row was clicked in. No composer: the input belongs to the parent.
- **The work overlay** (S7): everything in flight, grouped by kind, with a per-sub-agent kill.
- **The footer and the topbar indicator** (C2/C3): while a foreground child runs, the footer's activity is `Waiting on subagent…`; the counts the footer and the indicator show include the running children.

## The wire

The lifecycle must cross the daemon boundary the same way every other harness event does. `CommandProposed` is the worked template (six hops, listed in [`../06-renderer/agent-tui.md`](../06-renderer/agent-tui.md) §5): `HarnessEvent` → `map_event` in `goble-harness-internal` → `HarnessServerEvent` in `goble-harness-protocol` → the daemon's mapper → `DaemonEvent` in `goble-daemon-protocol` → `translate_daemon_event` in the desktop service, which emits `chat:subagent_spawned` / `chat:subagent_progress` / `chat:subagent_finished`. The payloads mirror the record's own fields; the protocol crate carries plain data and never depends on `goble-core`.

## Storage

- **A child owns a conversation.** `chats` has no parent column; the migration idiom in this tree is the probe-then-`ALTER TABLE` block that added `workspace_routing` (`crates/goble-core/src/store.rs:286-300`). `parent_chat_id` follows it, the child's chat id is its `SubAgentId`, and the child's messages are persisted under that id — never mixed into the parent's conversation. This is what makes the child view a view over real data.
- **A child owns a directory.** The child's working directory is a subdirectory of the workspace root derived from its id, so two children never share one (the other half of the `sandbox-and-cwd.md` tracker row).
- **Secrets and config** are inherited from the workspace, per Boundaries below.

## Reuse

- `xai-grok-subagent-resolution` (grok-build) is the **reference for the design, not a dependency**: its two-phase split — a pure resolution library (name → effective config, prompts, tool policy, with a documented precedence *explicit override > role > persona > parent*) plus host adapters — is the idea worth copying, along with capability mode as an ordered lattice intersected rather than last-write-wins, tool filtering by *kind* rather than by name, identity-validated resume with the model treated as a pin, and a coordinator whose foreground await has a deadline after which the child auto-backgrounds. Everything it actually contains is welded to grok-build's session, storage, worktree and prompt machinery; a different product reimplements it behind its own equivalents. Copy no file, add no dependency.
- **`goblin-worker::agent_runtime` is not reusable and this doc previously said it was.** `crates/goblin-worker/src/agent_runtime.rs` is one line — `pub use goble_core::agent_runtime::{ChecklistItem, RuntimeState};` — and the sibling `crates/goblin-worker/src/agent_runtime/` directory (`runtime.rs`, `state.rs`, `tools.rs`, an `AgentRuntime` with its own step loop) is not compiled, because `goblin-worker/src/lib.rs` declares only `pub mod agent_runtime;` and there is no `mod.rs`. Nothing references `AgentRuntime`. Do not build on it; the sub-agent loop is new code.
- The harness already has what a child needs: the tool registry and `execute_tool_call` (`crates/goble-core/src/harness.rs:1230-1292`), the model provider the turn loop uses (`crates/goble-core/src/reasoning.rs:321`), the per-run `workspace_dir` (`:488`, `with_workspace_dir` `:548-551`), and `LlmToolCall`/`Message` (`crates/goble-core/src/llm.rs:8-21`).

## Boundaries

- Sub-agents are scoped to the workspace and inherit its secrets/TOML (same rules as [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)).
- They are **not** "the agent you talk to" — they don't own the long-lived persona state.
- They are bounded in depth *and* in budget; neither bound may be enforced by hoping.

## Tasks

- [x] **S1 — the model, budget and lifecycle.** `SubAgentSpec`/`SubAgentBudget`/`SubAgentStatus`/`SubAgentRecord` in `crates/goble-core/src/subagent.rs`; exhaustion terminal by construction; a depth predicate; the per-child CWD rule.
- [x] **S2 — spawn a real child.** A `spawn_subagent` harness tool that runs a bounded child on its own conversation (`parent_chat_id`), with the child's own tool set (no `spawn_subagent` at the depth limit) and its output returned as the tool result.
- [x] **S3 — background, cancellation and the registry.** `run_in_background` returns at once with the child's id; the harness owns the records; completion, failure, cancellation and exhaustion all reach a terminal status; cancel reaches every child and any one child.
- [x] **S4 — the lifecycle on the wire.** `SubAgentSpawned`/`Progress`/`Finished` across the six hops, emitted as `chat:subagent_*`. The events ride the record's own fields as plain data; the registry emits each transition to the sink the live turn attaches, and a child that outlives its turn keeps updating the record, which stays the source.
- [x] **S5 — the transcript row.** The four status lines, the fold, the elapsed time, the click that opens the child view. **Done:** `SubAgentRow` (`crates/goble-ui/src/elements/chat_content.rs`) is the row's data — status, activity, elapsed, counters, outcome — fed from S4's live events held by the app (`PaneRuntime::apply_subagent_*` → `UiState::sub_agent_rows` re-keyed by `parent_call_id`), never from the tool's arguments; `spawn_subagent` draws one ruler row with a status bullet (`◐` running / `●` completed / `◆` failed / `◇` cancelled), the description and, while running, the activity and the elapsed time, folded by A4's three states like every other shape; a click on the row fires `ChatAction::OpenSubAgent(child_id)`, which S6 turns into the child view.
- [x] **S6 — the child view.** Enter shows the child's own conversation; Esc returns. **Done:** the row's click is `UiState::open_sub_agent(pane_id, child_id, desktop)` → A7's `enter_agent_view` aimed at the child — the child's card in the pane's block list, the pane's filter on the child's conversation — and `app/src/ui/chat.rs::build_sub_agent_child_view` draws the pane's body as that child's own transcript: rows read from the store under the child's chat id (`SubAgentChildView::refresh` → `list_chat_messages` → the same `MessageParseCache`), so a child's tool output is drawn by the one `terminal_block`/`ChatMessageBubble` renderer, with no bespoke path. The framed title above it carries the record's type, description, status and elapsed time (`{type} · {description}` and `status_line()`, the bullet from the shared `SubAgentRow::status_affordance()`), plus the `Esc to return` cue. There is **no composer** (`ChatView::without_composer`): the input belongs to the parent. Esc is handled by the child view itself (`ChatView::with_on_escape` → `UiActions.on_close_sub_agent(pane_id)` → `UiState::close_sub_agent_view`), which restores the view the pane came from — the shell, or the parent's own agent view when the row was clicked from inside it, remembered in `returned_to` — because A7 filters a pane to one conversation at a time and entering the child necessarily switches it. Nothing is stranded: both cards stay in the block list, so either conversation is reachable again by its own card. The mounted child is pane-scoped: closing the pane or its space, or binding the pane to another conversation (`UiState::bind_active_pane_conversation`), unmounts it. Enter-on-row is **not** added — transcript rows have no per-row keyboard focus; the row's own hit target (like S5's `open child`) is the way in.
- [ ] **S7 — the work overlay.** Grouped by kind, with the kill, opening from the chrome's cue and its own binding.

S1 → S2 → S3 and S1 → S2 → S4 are the hard edges; S5 → S6 and S4 → S7 the rest.

## Verification

- Core items (S1–S3): `cargo test -p goble-core`, with a stub `LlmProvider` so no network is touched.
- The wire (S4): `cargo test -p goble-harness-internal -p goble-daemon -p goble-desktop-service`, one adapter test per variant in the style of the existing `CommandProposed` mapping test.
- The surfaces (S5–S7): `cargo test -p goble-ui -p goble-app`.
- Nothing here builds or runs the `goble-app` binary. A real child against a real model, and what a sub-agent row feels like in a live session, are recorded as unverified rather than claimed.
