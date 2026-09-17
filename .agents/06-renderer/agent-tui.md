# 06 — Agent TUI (the harness surface)

**Status:** `[~]` partial — the transcript, the composer (including its propose/approve mode), the per-pane harness switch, the block-backed agent view and the conversation-card element exist; tool-call status does not.
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
| A5 | ~~**Composer proposal mode.**~~ **Done.** `CommandProposalUi` and a card with the candidate list, in-place edit through the composer's own editor, approve/reject buttons and ↑/↓ keyboard cycling, emitting the harness's `CommandDecision` with the proposal id | `crates/goble-ui/src/elements/chat_composer.rs` | ✅ `cargo test -p goble-ui` — the card draws its candidates with the selected one accent-marked, ↑/↓ cycle and load the draft, typing then Enter submits `Edit`, Enter on an untouched candidate submits `Approve`, Esc submits `Reject` |
| A6 | ~~**Harness approval suspension.**~~ **Done.** `CommandProposed { id, candidates, cwd }` + `CommandDecision { Approve, Edit, Reject }`, `Harness::resume_command`, a persisted `pending_commands` proposal, sharing the `auto_approve` bit | `crates/goble-core/src/harness.rs`, `reasoning.rs`, `store.rs` | ✅ `cargo test -p goble-core` — a `run_command` call suspends with a proposal (candidate line + cwd) and nothing runs; approve runs the chosen text, edit runs the edited text, reject yields a `ToolCallError` without running |
| A7 | ~~**Agent view over the block list.**~~ **Done.** Cmd+Enter pushes the conversation's card into the pane's block list and points the pane's filter at that conversation's agent view; Esc returns the filter to the terminal and the card stays behind; a click on the card reopens the view; the agent view draws the conversation's blocks through the one terminal-block renderer | `app/src/ui/terminal.rs`, `app/src/state.rs`, `app/src/actions.rs` | ✅ `cargo test -p goble-app` — Cmd+Enter enters, Esc leaves, and a click on the card enters again (`ui::terminal::tests::cmd_enter_esc_and_a_card_click_round_trip_the_agent_view`) |
| A8 | ~~**Conversation card element.**~~ **Done.** `ConversationCard::from_block(&Block)` reads the A7 card block, with `with_status`/`with_last_activity`/`with_on_click`, drawn in the pager idiom (a status-coloured rail, no border) | `crates/goble-ui/src/elements/conversation_card.rs` | ✅ `cargo test -p goble-ui` — the card reads the block's conversation id and label (id as the title when unnamed, `None` for a shell block), paints a square status rail with no border, shows the title/status/last activity, and a click fires its handler |

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

## 9. Follow-ups found by the first rollout

The first end-to-end rollout implemented A2–A8 and Q1–Q11; each item passed its own acceptance command, and the run's integration pass then found what per-item verification cannot see. They are tracked as R1–R6 in [`../TRACKER.md`](../TRACKER.md); three land here.

- **R1 — the approval gate made commands un-runnable.** A6 suspends any `run_command` when `auto_approve` is off, and that is the default (`crates/goble-core/src/harness.rs:515`, `app/src/state.rs:815`, `:1786`). It yields `CommandProposed` and returns; `crates/goble-harness-internal/src/lib.rs:255` maps that event to `Vec::new()` and `Harness::resume_command` (`harness.rs:662` → `reasoning.rs:856`) has no caller outside tests. Under the default configuration a command never runs and the turn never closes, where before this work it ran through the sandbox. §5's design is unchanged; what is missing is the path back, and A5's composer is what should answer it.
- **R6 — the cards A5 and A8 promised did not arrive card-free (fixed).** A5's proposal was a `Container` with `Border::all(1.0)` and `corner_radius(8)` (`chat_composer.rs:620-629`), and the app drew its own bordered card in `app/src/ui/terminal.rs::build_card` while A8's card-free `ConversationCard` element was exported and never constructed. The proposal is now a full-width band of rows with no border and no radius, and `build_card` builds A8's `ConversationCard` from the block identity the pane's view carries (`ConversationCard::new`), so the app draws the real element. Tests: the composer's proposal test asserts zero borders, and `ui::terminal::tests::the_conversation_card_is_card_free` asserts the app's card draws the element's square status rail and no border.
- **R5 — the status is inferred, not read (fixed).** `app/src/ui/terminal.rs:562-566` rendered a still-running block as `Success` from the boolean `block.failed`, which is the same defect as the `ERROR: ` prefix in [`agent-parser.md`](agent-parser.md) §10: A4's row reads its status, the surfaces around it still guessed. `VisibleBlock` now carries the block's `BlockState` and `exit_code` (both read off the `Block` in `app/src/emulator.rs`), and the agent view maps them — a block that is not done draws `Running`, never `Success`; a failed one is its non-zero exit code. The `ERROR: ` prefix is gone from `reasoning.rs` and the tool-result row's status is resolved from the persisted call record instead. Test: `ui::terminal::tests::the_agent_view_never_draws_a_running_block_as_success`.

The lesson worth keeping: an item's acceptance test proves the item, not the feature. The approval gate passed its own test — suspend, approve, edit, reject all behaved — while the default path through it dead-ended at a wire boundary no single item owned.

## 10. Follow-ups found by the second rollout

A4 was the one item of this plan that was never implemented; it now is, and the reason it mattered showed up as soon as the transcript had bodies worth folding: the reasoning rows fold (Q11 gave them an app-owned collapsed/expanded map) and a tool call did not — `build_tool_call_rows` (`crates/goble-ui/src/elements/chat_message_bubble.rs:102`) drew the whole body on every frame. The owner's requirement is explicit: **the fold must behave exactly as grok-build's does.**

- **A4 — the tool call folds like grok-build's (implemented).** grok-build keeps a per-entry `DisplayMode::{Collapsed, Truncated, Expanded}` (`scrollback/types.rs:54`) with a per-block default set when the entry is created (`entry.rs:188`), and a `toggle_fold` (`entry.rs:258`) driven by `e` (fold), `h`/`l` (collapse/expand), `E` (all) and double-click (`app/agent_view/selection.rs:1091`). The per-block defaults are the substance, not the keys: a read starts collapsed and shows the path alone, `e` gives the first 5 and last 3 lines, `l` gives the whole file (`blocks/tool/read.rs:17-18`); an edit starts collapsed with a `+N/-M` diffstat in the header (`blocks/tool/edit.rs:996-1007`); an agent's command starts collapsed and the user's own `!` command starts truncated at first 2 + last 3 (`blocks/tool/execute.rs:518`, `config.rs:693-694`). Adopted here: `ToolDisplayMode` in `chat_content.rs`, every tool shape starts `Collapsed`, a header click or `e` advances `Collapsed -> Truncated -> Expanded -> Collapsed`, and the app-owned `tool_fold` map (`app/src/state.rs`, next to `reasoning_expanded`) holds the state across the rebuild. Our own single key plus click stand in for grok-build's `e`/`h`/`l`/`E`/double-click; the defaults and the three states match. The point stands: a folded row is the *quiet* form and it is what you see first.
- **R9 — four surfaces are still bordered cards.** `AskUserCard` (`ask_user.rs:297`) wraps the agent's pending question in a `Border::all(1.0)` container with a corner radius while the sibling proposal is a borderless band, and `chat_view.rs` still draws three more (`:755` queued prompt, `:806` and `:851` handoffs). The rule has no exception for these.
- **R10 — two loose ends from R1 (fixed).** `resume_command_chat_turn` (`crates/goble-desktop-service/src/state.rs:2733`) issued the resume before subscribing to the bus, so a fast turn could finish before anyone was listening; and the "never leaves the session busy" acceptance clause rode on assertions inside the approve/reject tests instead of being a test in its own right. The listener now subscribes before `resume_command`, so the broadcast receiver exists before the resumed turn can emit `TraceFinished` and the handle cannot miss it. Test: `state::tests::a_command_turn_frees_the_session_on_every_exit_path` drives the proposal through approved, rejected and approved-but-failing and asserts the session is live while suspended and freed once the joined listener sees the terminal event.

### Found by the second rollout's integration pass

The fold (A4) and the four card surfaces (R9) are in. Two things the rule still catches, both found by reading the diff rather than by a failing test:

- **R13 — the error notice is the last bordered card.** `build_agent_error` (`app/src/ui/chat.rs:408-413`) wraps the agent's failure in `Container::with_background(Surface).with_border(ColorToken::Error).with_corner_radius(...)`, drawn inline in the transcript through `build_agent_chat`'s `with_notice` (`app/src/ui/chat.rs:160` → `views/chat_view.rs:216`, painted at `:694-695`). After R9 it is the only `with_border` left anywhere in the agent surface, which makes it conspicuous: a failure is exactly the message that should look like the rest of the pager — an accent rail or a full-width band, not a red-outlined box.
- **H5 — the markdown boxes are boxes.** A fenced code block and a blockquote are wrapped in a rounded `SurfaceRaised` container (`crates/goble-ui/src/elements/chat_message_bubble.rs:991-1007`; in the group view `crates/goble-ui/src/elements/group_chat_message.rs:234-243`, `:326-330`, `:404-413`). No border, so R9's grep would not have caught them, but the rule says "bordered card **or box**" and grok-build draws both as full-width bands — a fenced block sits on a panel band, a quote indents. Convert them.
- **R12 — the environment menu had no way to set a default.** R11 left the "+ ▾" menu's environment rows doing two jobs by accident: a click opened a space on the environment, and the environment that new spaces start in was only ever whatever the last click left active. warp-new settles both in one place — hovering an item in its new-session dropdown raises an *action sidecar* beside that row — so the tray is now the mechanism, in `PopupMenu` rather than in the app: `with_hover_tray(width, build)` builds it for the hovered item's index, hangs it from that row's top edge just past the panel's trailing edge (or to the panel's left when the available width says it would leave the window), and `dispatch_event` routes an event inside the tray's bounds to the tray, consuming it so a tray click neither closes the menu nor falls through to the outside-the-panel close. Hover is read at paint time, where this framework reads hover, and the picked row is app-owned (`with_hover_index`, `UiState::env_selector_hover`) for the same reason the open flag is: the tree is rebuilt every frame. `CursorMoved` requests a frame of its own, so a hover-only surface does not wait for the idle heartbeat. The app's tray (`build_environment_tray`, `app/src/ui/shell/topbar.rs`) names the environment and offers "Make default", which is `MediaActions::on_select_medium` — the pick "+" uses — plus closing the menu; clicking the row itself still opens a space on the environment, and the row's button is inert, muted and tooltipped once it already is the default.
- **R11 — the workspace had no surface listing its own chords, its menu could not launch a terminal, and picking an environment only switched it.** The instruction strip carries the composer's five gestures and nothing else; the global chords (`⌘K`, `⌘Space`, `⌘⇧D`, `⌘⇧T`, `⌘⇧W`, `⌘W`, the pane arrows) were discoverable only by reading `app/src/root_view/element.rs` or the palette. `Ctrl+.` — grok-build's own cheatsheet binding — now toggles `app/src/ui/shortcuts_help.rs`, a centered panel of sections (**Workspace**, **Input**, **Palette**), one row per gesture, each drawing the strip's own key caps in a fixed key column (`ShortcutHint::caps`). The rows are this app's bindings and only those: a row the app does not answer is not drawn, and the test that proves it fails on any label it cannot dispatch. Nothing advertises the chord yet (no palette entry, no hint row, no topbar control). In the same pass the topbar's "+ ▾" environment menu gained its first entry, **New terminal**, wired to `on_add_space` — the menu listed environments only, so a terminal could not be launched from it. Its environment rows no longer merely switch the active environment: choosing one calls `on_add_space_with_medium`, so the pick opens a new space running on that environment, its tab appended to the strip exactly where "+" puts one (the chosen environment also becomes the active one, because the new space runs there).

## 11. The live chrome: what is running, while it runs

Everything the transcript shows is *after the fact*: a tool row appears when the store is re-read, and the composer's Stop button (`crates/goble-ui/src/elements/chat_composer.rs:44`, drawn `:925-941`, fed by `agent_busy` in `app/src/ui/chat.rs:193`) is the only thing on screen that says a turn is live. There is no footer, no topbar indicator and no overlay for work in flight — and, until wave 12, no sub-agents to show. This section is the design for the chrome; [`../04-agent-runtime/subagents.md`](../04-agent-runtime/subagents.md) is the design for what it will eventually show most of.

### What exists, and what does not

- **A footer does not exist.** No `footer.rs`, no `status_bar`/`status_line`/`status_row` element, and no row reserved for one: the chat column's last two children are the divider and the composer (`crates/goble-ui/src/views/chat_view.rs:1058-1059`). `RunningIndicator` (`crates/goble-ui/src/elements/running_indicator.rs`) was meant to be the spinner and is dead: its `paint` records its origin and draws nothing (`:47-49`), and it is constructed only in its own test (`:68`).
- **The topbar has no status slot.** The shipping topbar is the app's own `build_topbar` (`app/src/ui/shell.rs:56-123`), built at `app/src/ui/mod.rs:1414` and painted in the root stack (`:1527-1539`); its root row is `SpaceBetween` with exactly two children — `left` (sidebar toggle, workspace chips, the `+ ▾` environment control) and the settings icon (`:90-108`). `goble-ui`'s own `Topbar` element (`crates/goble-ui/src/elements/topbar.rs:39-171`) is used only by `ShellView`, never by the app; this is not the moment to switch.
- **The data is mostly already flowing, and dropped.** `RootView::drain_events` (`app/src/root_view.rs:120-308`) polls `bus.take_events()` (`:123`) against a fixed list of names (`:131-291`) and matches no `agent:*` event at all, although the desktop service emits `agent:log` (`crates/goble-desktop-service/src/state.rs:1553`), `agent:started` (`:1584`), `agent:finished` (`:1613`), `agent:state_update` (`:1623`) and `agent:tool_result` (`:1705`). The service holds the live records (`ExecutionInfo` at `:259-267`, the map at `:378`, `list_executions` at `:2034-2036`; the per-chat `tool_in_flight` map at `:405`), and the app already tracks per pane what the transcript needs (`PaneRuntime::in_flight_tools` `app/src/state.rs:576`, `busy` `:566`, `pending_command` `:561`, `pending_ask` `:558`, `UiState::agent_busy` `:741`). C1 closed this: the `agent:*` names are drained into `UiState`'s live execution records and `UiState::live_work()` is the one accessor. (The old `pane_in_flight_tool_count`, documented "test observability; not read by the app", was deleted in its favour.)
- **An overlay has nowhere to list sub-agents.** There is no sub-agent record to show (see `subagents.md`), so the overlay is built last, on the records wave 12 produces.

### The design

- **C1 — one live source.** Drain the `agent:*` names into app state and expose a single accessor: the running executions (agent, worker, started at), the active pane's in-flight tool calls and busy flag, whether an approval or a question is pending, and the current turn's start time when one can genuinely be observed. Do not invent a token count or a clock the app does not have — a field the events do not carry is omitted, not faked. C1 renders nothing; it is the only item the other two depend on. It also delivers the `07 — Observability` tracker row for the same drain. **Done.** `RootView::drain_events` handles `agent:started` / `agent:finished` / `agent:state_update` / `agent:tool_result` / `agent:log`; the accessor is `UiState::live_work()` (`LiveWork { executions, tool_calls, turn_busy, pending_approval, pending_question, turn_started_at }`). A run is tracked from `agent:started` (which now carries the service's own `started_at`, added to the event so the app reads it rather than guessing from arrival) until `agent:finished`; `agent:state_update` stores the run's runtime state and `agent:tool_result`/`agent:log` its last activity. `turn_started_at` is recorded by `UiState::begin_turn` where the app issues the turn and cleared by `finish_turn`; a turn whose start was not observed reports `None` instead of an invented time. `pane_in_flight_tool_count` was deleted in favour of `live_work()`. Verified by `cargo test -p goble-app` (`root_view::tests::drain_events_leaves_the_live_accessor_reporting_what_is_in_flight`).
- **C2 — the footer.** One row, the pager idiom, placed in the chat column directly above the composer's divider and wired the way the composer's flags already are (a `ChatView` builder field, compare `with_composer_stop_visible`, `chat_view.rs:62,350-352,956`). Three states and no others: **busy** — the activity actually under way (a running tool drawn as its command or name, `Waiting on approval…` while a proposal is pending, `Waiting on a question…` while one is, otherwise `Thinking…`/`Responding…`) plus the elapsed time, with a spinner advanced from a real clock or frame counter; **idle with work in flight** — `{n} {kind} still running`, naming each kind that is really running, as one hit target that opens the work overlay; **nothing in flight** — zero height. The composer keeps the only Stop control; the footer does not add a second one.
- **C3 — the topbar indicator.** A live indicator between the workspace strip and the settings button in the app's own `build_topbar`: a spinner plus `◆ N` from C1's state when work is in flight, inert at zero, firing the same overlay action as C2's cue. `TOPBAR_HEIGHT` (36 px, 40 on macOS) and the workspace controls are untouched, and the app keeps its own topbar.
- **The work overlay** is a wave-12 item (S7) because it needs sub-agent records to be worth opening; it uses the app's established overlay pattern (`Sheet`/`Dialog` at `app/src/ui/mod.rs:1479-1514`, `:1520-1524`), groups by kind, and carries a kill per sub-agent. Until it lands, the cue and the indicator fire an action with no visible surface — stated plainly rather than left as a surprise.

### Verification

- C1: `cargo test -p goble-app` — a scripted event sequence leaves the live accessor reporting exactly what is still in flight.
- C2: `cargo test -p goble-ui -p goble-app` — the element draws each of the three states, takes zero height when idle and empty, and its cue fires the overlay action.
- C3: `cargo test -p goble-app` — the indicator reflects the count, is inert at zero, and its click fires the overlay action.
- Nothing here builds or runs the `goble-app` binary: the chrome is exercised through element and app tests. The *feel* of a live footer — whether the spinner reads as motion, whether the row's appearance is jarring on a slow turn — is not something these tests can answer and is recorded as unverified until the app is run by its owner.

## 12. Follow-ups found by the remote rollout's integration pass

The remote rollout moved the pane's surface decision to `PaneControls::harness_mode` (the per-pane flag Cmd+Enter flips) while several readers still ask the pane's *view* what surface it is drawing. The two answers agree everywhere except in the one state where a caller returns a pane to the shell.

- [x] **U1 — a pane that goes back to the shell really goes back.** `app/src/ui/terminal/build.rs` decides the surface from `controls.harness_mode`, but `UiState::bind_active_pane_conversation` (`app/src/state/ui_state/session.rs`) — whose own comment says the pane goes back to the shell — only resets `view = BlockView::Terminal` and never clears `harness_mode`. The pane then keeps drawing the agent surface while every reader of `pane_view` sees `Terminal`, and both consequences are live for a user: `app/src/root_view/element.rs`'s `matches!(state.pane_view(pane_id), BlockView::Agent { .. })` test goes false, so the root declines the Cmd/Ctrl+F chord and the shell's own handler is not mounted — the transcript filter is dead while that transcript is on screen, which it was not at HEAD — and `app/src/actions/make_actions.rs`'s `!cmd` path matches `BlockView::Terminal => None`, so a command typed into the agent surface runs with no conversation owner. Reachable from a sidebar conversation switch or an agent delete while a sub-agent child view is open. Make one flag the source of truth for "which surface is this pane drawing", clear it in the caller that returns a pane to the shell, and audit every other reader of `pane_view`/`pane_controls(..).view` so no third reader disagrees. Prove it: after a conversation switch that returns a pane to the shell, the pane draws the shell surface, the filter chord is handled, and a `!cmd` carries the pane's conversation owner — and the same holds after an agent delete. **Done (2026-09-17):** which surface a pane draws is one decision, taken from one flag — `PaneControls::surface` (`app/src/state/types.rs`) answers `Self::view`'s agent view while `harness_mode` is on and `BlockView::Terminal` otherwise, `build_terminal` (`app/src/ui/terminal/build.rs`) draws from it and `UiState::pane_view` returns it, so the pane that paints and every caller that routes a chord read the same answer by construction. `PaneControls::view` is the subordinate filter — which conversation the agent surface shows — and is never asked whether the pane is an agent pane; `app/src/actions/make_actions.rs`'s `!cmd` path now asks `pane_view` for the owner, the last reader of `pane_controls(..).view` outside the writers. The writers move both halves together: `UiState::enter_agent_view` turns the switch on with the filter, `UiState::leave_agent_view` turns it off with it, and `bind_active_pane_conversation` (`app/src/state/ui_state/session.rs`) — the caller whose own comment says the pane returns to the shell — calls `leave_agent_view` instead of resetting `view` alone; `close_sub_agent_view` (`app/src/state/ui_state/agents.rs`) routes its shell branch through the same writer, and its parent-view branch (which writes the filter alone) is consistent either way, since a pane whose switch is off reads as a shell whatever filter it carries — the state `UiState::viewer_shape` leaves behind — which `filter_chord`'s and `the_panes_mode_decides_its_surface_and_the_switch_puts_the_agent_surface_up`'s shell-side assertions pin. Verified: `DEVELOPER_DIR=/Library/Developer/CommandLineTools cargo test -p goble-app -p goble-ui` → **exit 0** — `goble-app` lib **461 passed / 0 failed** (459 before; the two new cases `root_view::tests::return_to_shell::{a_conversation_switch,an_agent_delete}_that_returns_a_pane_to_the_shell_really_returns_it`), `goble-ui` lib **487 passed / 0 failed**, every `goble-app` integration binary green (`chat_flow` 9, `connector_flow` 4, `cron_flow` 4, `first_run_flow` 11, `harness_flow` 9, `harness_pages_flow` 1, `menu_flow` 13, `projects_flow` 3, `screen_flow` 8, `ui_render` 38, `vault_flow` 3), `0` `FAILED` lines. Each case drives the app's own gesture — the sidebar's selection, the agent delete — from an agent-mode terminal pane with a sub-agent child view open, then asserts the three things a user sees: the pane's frame draws the shell's own bar (`new conversation`) and none of the agent runs (`send to cloud`, `Esc to return`), the root's dispatched Cmd/Ctrl+F leaves that pane's whole-output filter open and holding the caret, and a `!cmd` sent from the reopened agent view claims exactly one block for the pane's conversation (`pane_agent_view` carries it, the shell's own list does not). Non-vacuity, both ways: with `bind_active_pane_conversation`'s old `view = BlockView::Terminal` line restored, both cases fail at `and the harness switch went off with it, so the surface drawn and the surface read are the same one`; with HEAD's pair — the drawer on `harness_mode` alone and `pane_view` on the raw `view` — and the switch left stuck on, they fail in turn on the drawn run (`the shell's own rich input is what the pane draws now`, the frame carrying `send to cloud`/`esc`/`for terminal`), then on the chord (`the pane's own filter chord answers Cmd+F`), then on the owner (`exactly one shell block became the conversation's`, left `0`) — all probes reverted and the acceptance command re-run green. `the_panes_mode_decides_its_surface_and_the_switch_puts_the_agent_surface_up` was rewritten to write the filter on its own instead of through `enter_agent_view` — not weakened: the case is exactly the filter-without-the-switch state, and every assertion it carried still runs. **Not verified:** the pixels — nothing was built or launched (`cargo build`/`cargo run`/`cargo install` are forbidden here), so the surface is proven by the frame's drawn commands and the chord by the dispatched key and the state it leaves.
