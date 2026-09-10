# 06 — Agent TUI (the harness surface)

**Status:** `[~]` partial — the transcript, the composer and the per-pane harness switch exist; the block-backed agent view, tool-call status and the propose/approve composer do not.
**Owns:** how a conversation is stored and drawn next to the shell — the block list that holds both, tool-call blocks with their status, the composer's propose/edit/variants mode, and the agent-view card that stays behind in the terminal.
**Depends on:** [`terminal-blocks.md`](terminal-blocks.md), [`terminal-emulator.md`](terminal-emulator.md), [`../04-agent-runtime/README.md`](../04-agent-runtime/README.md)

## How to read this doc

Anchors into the reference at `~/Projects/warp-new`, same scheme as [`terminal-blocks.md`](terminal-blocks.md):

- `A/…` = `app/src/terminal/model/…` — the terminal model (blocks, grids, rich content).
- `V/…` = `app/src/terminal/…` *(outside `model/`)* — the view/PTY/input layer.
- `W/…` = `app/src/…` — everything else in the reference app (the AI layer, the pane group).

Our own code is referenced by path, no prefix.

---

## 1. The question this doc settles first

The hypothesis on the table: *"the harness could be a process running in the PTY — that is probably what Warp does, otherwise the conversation could not appear in the terminal."*

**Verified: Warp does not do that.** The agent is an in-process library the app drives (`W/app/src/pane_group/child_agent.rs:5` — `use octomus_cli::agent::Harness;`), and the conversation appears in the terminal because the terminal's own block list holds it:

- Terminal blocks are not all cell grids. A block can carry **rich content** typed by enums: `RichContentType::{AIBlock, EnterAgentView, InlineAgentViewHeader, AgentViewZeroState, …}` (`W/app/src/terminal/model/rich_content.rs:1-27`). `AIBlock` is the conversation; `EnterAgentView` is the card that stands for "there is a conversation here, click to enter" (`is_agent_view_block`, `:19-21`).
- Entering the agent view inserts that card: `Some(RichContentType::EnterAgentView)` at `W/app/src/terminal/view/agent_view.rs:399`.
- Entering is recorded by origin, and the origins are exactly the two gestures we care about (`W/app/src/ai/blocklist/agent_view/controller.rs:99-119`):
  - `AgentViewEntryOrigin::Input { was_prompt_autodetected }` — *"Entered agent view from user input (e.g. /agent or cmd-enter keypress)."*
  - `AgentViewEntryOrigin::AgentViewBlock` — *"Entered agent view by clicking an existing agent view block."*

So the sequence the user described — Cmd+Enter enters, Esc returns, a card with the conversation is left behind in the terminal, clicking it comes back — is not a PTY program. It is a block in the list.

Why this matters for us: we already own a block model (`crates/goble-terminal/src/blocks.rs`) and an emulator. We do **not** need to make the harness a terminal program, do not need a second PTY, and do not need to parse frames to recover a conversation. **The conversation is a block whose body the app renders.**

## 2. One block list, two views

The terminal history and the agent history are **not two storages**. They are two filters over one block list, and the filter is a per-block property.

`A/block.rs:1376` `should_hide_block(agent_view_state)` decides it:

- Agent view **active** for conversation *X* (full-screen) — show only blocks whose visibility names *X*; every shell block that is not associated with *X* is hidden (height 0).
- Agent view **inactive** — the AI blocks are hidden and the shell blocks show.

Visibility is per block and is a small enum (`A/block.rs:406-407`, `:1032-1080`):

| Variant | Meaning |
|---|---|
| `AgentViewVisibility::Terminal { pending_conversation_ids, conversation_ids }` | a shell block that is (or is about to be) visible in the terminal view |
| `AgentViewVisibility::Agent { origin_conversation_id, pending_other_conversation_ids, other_conversation_ids }` | a block that is visible inside those conversations' agent views |

And the association is explicit — `A/blocks.rs:1683` `associate_blocks_with_conversation`: *"Associates the given blocks with a conversation, making them visible in that conversation's agent view."* That is how a command you type **inside** the agent view ends up in that conversation's history while the shell blocks around it stay hidden.

```mermaid
flowchart LR
  subgraph list["one block list (one pane)"]
    s1["shell block"]
    s2["shell block"]
    card["EnterAgentView card<br/>(RichContentType)"]
    ai["AIBlock: conversation<br/>(rendered by the app)"]
    cmd["shell block executed<br/>inside the conversation"]
  end
  card -.->|Cmd+Enter / click| ai
  cmd -.->|associate_blocks_with_conversation| ai
  terminal["terminal view"] -->|filter: Terminal| list
  agent["agent view"] -->|filter: Agent{conversation_id}| list
```

Consequences we inherit for free if we model it this way:

- Leaving the agent view leaves the card in the terminal scrollback at the position where the conversation happened — no separate "restore" path.
- Scrolling the terminal past the card shows the surrounding shell history, because it is the same list.
- The conversation survives a restart with the same persistence story as blocks (B4 in [`terminal-blocks.md`](terminal-blocks.md)).
- A command run from inside the conversation is a block like any other, so it renders with the same [`TerminalBlock`](../../crates/goble-ui/src/elements/terminal_block.rs) element as a shell command.

## 3. What we have today

| Piece | Where | State |
|---|---|---|
| Block model | `crates/goble-terminal/src/blocks.rs` | `Block`/`BlockList`, hook-driven lifecycle, per-block `command`/`output` screens; **block kinds and view visibility are in** (A1) — `BlockKind::{Shell, AgentView}`, `BlockVisibility`, `BlockView::{Terminal, Agent}`, `visible_blocks`/`visible_lines` |
| Real terminal pane | `app/src/ui/terminal.rs`, `app/src/emulator.rs` | emulator behind the pane, cell grid, keys/pointer through the crate |
| Harness switch per pane | `app/src/ui/terminal.rs:436-502`, `PaneControls::harness_mode` (`app/src/state.rs:274`) | Cmd+Enter activates, Esc returns to the shell, per pane |
| Transcript | `app/src/ui/chat.rs`, `goble-ui/src/elements/chat_*` | message bubbles, markdown, inline `TerminalBlock` for tool output, streamed assistant deltas |
| Composer | `crates/goble-ui/src/elements/chat_composer.rs` | draft, pills (model/harness/dir/branch), attach/voice/stop, slash, profile menu |
| Tool calls | `crates/goble-ui/src/elements/chat_content.rs:121`, `chat_message_bubble.rs:39-95` | `ToolCall { name, arguments }` rendered as a **static** card — no status, no result, no pairing with the output row |
| Pending question | `crates/goble-ui/src/elements/ask_user.rs` | single-select quick replies + free text + credential field |

The transcript already exists. What is missing is the block backing, the status, and the approval loop.

## 4. Tool-call status: the data exists, the renderer drops it

The harness already emits the full lifecycle (`crates/goble-core/src/harness.rs:206-245`):

```rust
ToolCallStarted { id, name, arguments }
ToolCallFinished { id, result }
ToolCallError { id, message }
```

and yields them around execution (`crates/goble-core/src/reasoning.rs:632`, `:655`, `:670`). Three hops lose them:

1. **The app never sees the events.** `chat:updated` is emitted for *every* daemon event (`crates/goble-desktop-service/src/state.rs:671`), but the only named events are `chat:ask_user`, `chat:mission`, `chat:turn_finished`, `screen:handoff` (`:674-726`). The native UI reacts to `chat:updated` by **re-reading the store** (`app/src/root_view.rs:134`), so anything not persisted is invisible.
2. **The store keeps the intent, not the outcome.** The assistant row's `tool_calls` column holds the planned calls (name + arguments, `reasoning.rs:591-613`). The outcome is a separate `role="tool"` row whose content is `"<call_id>\n<output>"` — written only *after* the call returns (`reasoning.rs:642-654`, `:662-674`). So there is no running state, and nothing links a call card to its output row except an id embedded in text.
3. **The renderer has no field for it.** `ToolCall { name, arguments }` (`chat_content.rs:121-127`) has nowhere to put a status.

`HarnessEvent::ThinkingModeChanged` and the whole `Reasoning*` family are additionally dropped by the wire adapter (`crates/goble-harness-internal/src/lib.rs:265-268`), so reasoning is not merely unrendered — it never reaches the app.

**The fix is a status carrier, and the design choice is live vs. persisted.** Both are needed: "running" has no durable meaning, while "finished/failed" must survive a restart and a re-read from the store. So:

- **Persisted:** the `tool_calls` column gains `status` and `result` per call, written on start and on finish/error, so a re-read renders the terminal states.
- **Live:** the desktop-service translator gains a structured `chat:tool` event (id, name, status, result) emitted from `ToolCallStarted/Finished/Error`, held in a per-pane in-flight map and overlaid on the persisted rows while the turn runs.

## 5. Approval is a harness change, not a UI change

There is **no approval gate today**. `run_command` expands credential placeholders and calls the runner (`crates/goble-core/src/harness.rs:1358-1375`); the only guard is the runner's allow-list plus optional sandbox (`:167-188`, default `echo, cat, ls, pwd, git, cargo, npm, node, python3, rustc`). "Approve" in this crate currently means only "skip the `ask_user` suspension" (`:371-376`, consumed at `crates/goble-core/src/reasoning.rs:448`).

So a composer that offers *run / edit / pick another candidate* cannot be built in the renderer alone. The turn has to **suspend before executing**, the way `ask_user` already suspends:

```
ToolCallStarted
  → is it a command tool? and not auto-approved?
      → emit CommandProposed { id, candidates: Vec<String>, cwd }   [new]
      → wait for the decision
          Approve(chosen)  → run `chosen` verbatim
          Edit(text)       → run `text`
          Reject(reason)   → ToolCallError("rejected by user")
  → otherwise run immediately (today's path)
```

Candidate generation ("give me three ways to do this") is a model call, not a tool call; it belongs to the harness, and the composer only renders what it is handed. The existing `auto_approve` flag is the switch that skips the suspension, so the two features share one bit.

## 6. The pieces, and the order we build them

Each row is meant to be one turn of work — small, fully defined, verifiable on its own.

| # | Item | Where | Verified by |
|---|---|---|---|
| A1 | ~~**Block kinds + view visibility.**~~ **Done.** `BlockKind { Shell, AgentView }`, a per-block visibility set, `BlockList::{push_agent_view_block, associate_with_conversation, set_visibility}`, `visible_blocks(view)` | `crates/goble-terminal/src/blocks.rs` | ✅ `cargo test -p goble-terminal` — an agent-view block coexists with shell blocks without becoming active, each view filter sees exactly its own, and association makes a block visible in a conversation |
| A2 | **Tool-call status carrier.** `status`/`result` on the `tool_calls` column, written on start and finish | `crates/goble-core/src/reasoning.rs`, persistence | tests: a finished call is readable from the store with its status |
| A3 | **Live tool events to the app.** `chat:tool` from `ToolCallStarted/Finished/Error` | `crates/goble-desktop-service/src/state.rs`, `app/src/root_view.rs` | test: the in-flight map gains and clears an entry per call |
| A4 | **Tool-call block with status.** `ToolCall { id, name, arguments, status, result }`, per-status icon/colour, collapsible body, output grouped under its call | `crates/goble-ui/src/elements/chat_content.rs`, `chat_message_bubble.rs` | element tests + the app's mapping tests |
| A5 | **Composer proposal mode.** candidate list, in-place edit, approve/reject, keyboard cycling between candidates | `crates/goble-ui/src/elements/chat_composer.rs` | element tests: selection, edit, submit payload |
| A6 | **Harness approval suspension.** `CommandProposed` + resume on the decision, sharing the `auto_approve` bit | `crates/goble-core/src/harness.rs`, `reasoning.rs` | harness tests: suspend, approve-runs-chosen, edit-runs-edited, reject-errors |
| A7 | **Agent view over the block list.** Cmd+Enter pushes the block and switches the pane's filter; Esc returns; the card renders in the terminal and is clickable | `app/src/ui/terminal.rs`, `app/src/state.rs` | app tests: enter/leave/click round-trip |
| A8 | **Conversation card element.** The `EnterAgentView` card: conversation title, status, last activity, click handler | `crates/goble-ui/src/elements/` | element tests |

A1 blocks A7 and A8; A2–A3 block A4; A5 and A6 are one feature split at the wire and can land in either order. Nothing here blocks on the remote/worker path.

## 7. What this does not cover

- **Reasoning display.** `Reasoning*` events are dropped on the wire; showing the model's thinking needs the adapter change first, then a block. Separate item.
- **The product pages** (agents, executions, logs) and the per-message author/avatar header and status footer, which are the remaining warp-new parity items in [`../TRACKER.md`](../TRACKER.md).
- **Persistence of the conversation as a block** — that is B4 in [`terminal-blocks.md`](terminal-blocks.md); this doc reuses it rather than redefining it.
- **Voice and computer use.** Different surfaces; see [`../04-agent-runtime/README.md`](../04-agent-runtime/README.md).

## 8. Verification

- Crate items (A1, A2, A6, A8): `cargo test -p <crate>` with the test named in the row above.
- App items (A3, A7): `cargo test -p goble-app`.
- Element items (A4, A5): `cargo test -p goble-ui`.
- No step here builds or runs the `goble-app` binary; the renderer is exercised through the app's element tests, and anything that genuinely needs a window is recorded as unverified rather than claimed.
