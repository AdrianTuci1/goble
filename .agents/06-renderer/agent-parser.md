# 06 — Agent parser (what the agent says, and how it reaches the screen)

**Status:** `[~]` partial — the pipeline runs end to end, but it flattens structure it parses, drops constructs it does not, and has no live tool-call state.
**Owns:** the agent's output turned into UI: Markdown → fragments → blocks → elements, and the presentation of a tool call (its status, its arguments and its result).
**Depends on:** [`agent-tui.md`](agent-tui.md), [`agent-terminal-bridge.md`](agent-terminal-bridge.md), [`renderer-architecture.md`](renderer-architecture.md), [`../04-agent-runtime/README.md`](../04-agent-runtime/README.md)

## 1. The pipeline as it stands

Three stages, each rebuilt from scratch on every frame:

```
SQLite row.content ──parse_markdown──► Vec<ChatFragment> ──group_fragments_into_blocks──► Vec<ChatBlock> ──► element tree
   row.tool_calls ──ToolCall::from_llm_json──► Vec<ToolCall> ─────────────────────────────► tool-call cards
```

- `crates/goble-ui/src/elements/markdown.rs` — a `pulldown_cmark` event fold, ~250 lines, one call per message per frame. It constructs `Parser::new(input)` with **default options** (`markdown.rs:16`), so no GFM extension is on.
- `crates/goble-ui/src/elements/chat_content.rs` — `ChatFragment` (13 kinds) folded into `ChatBlock` (7 kinds); consecutive inline kinds become a `Paragraph(Vec<InlineSpan>)` (`chat_content.rs:313-372`).
- `crates/goble-ui/src/elements/chat_message_bubble.rs` — the renderer, rebuilding its child tree every frame (`:356` → `:192`); the transcript that stacks the bubbles is `crates/goble-ui/src/views/chat_view.rs`.

The stages are sound. What is wrong is what the first two keep and what the third can express.

## 2. Text formatting: what survives, what does not

| Construct | State today | Where |
|---|---|---|
| Paragraphs, bold, inline code, fenced code (language preserved), links, ordered/unordered lists, blockquotes, headings, hard breaks | parsed and rendered | `markdown.rs:42-186` |
| **Italic** | **parsed, then discarded** — the renderer drops the flag, so `BoldItalic` renders as bold only | `inline_text.rs:274` |
| **Nested lists** | **broken** — `Tag::List` clears the shared `list_items` buffer, so a nested list replaces its parent's items instead of nesting | `markdown.rs:51-59` |
| **Horizontal rules** | **dropped** — `Event::Rule` falls through the `_ => {}` arm | `markdown.rs:186` |
| **Tables, task lists** | **never tokenised** — the GFM extensions are off, so pipes and `- [ ]` stay literal | `markdown.rs:16` |
| **Images** | render as their alt text; `Tag::Image` is unhandled | `markdown.rs:77` |
| **Raw HTML** | shown **verbatim as text** — neither rendered nor stripped | `markdown.rs:147-157` |
| Blockquote and list item contents | collapsed to one `String`, so a nested paragraph, a nested quote or inline emphasis inside them loses its structure | `markdown.rs:99-105`, `:85-98` |
| Ordered list numbering | restarts at 1 regardless of the source's start number | `chat_message_bubble.rs:247` |
| **Code blocks** | **done (Q4)** — rendered through `Code` in the mono family, preserving indentation, not word-wrapped; the fence's language label is drawn in mono | `chat_message_bubble.rs`, `code.rs` |

Two structural notes worth keeping in view:

- **Links break paragraphs.** `group_fragments_into_blocks` deliberately refuses to fold a `Link` into the inline flow (`chat_content.rs:306-308`), so a link in the middle of a sentence becomes a standalone chip on its own line. That was a defensible shortcut when links were the only interactive span; it is the wrong shape once the paragraph model matters.
- **There is no syntax highlighting anywhere, and no dependency for it.** `crates/goble-ui/Cargo.toml` has `pulldown-cmark` and nothing else in this area. `Code` (`crates/goble-ui/src/elements/code.rs`) is no longer dead code: `ChatMessageBubble` constructs it for a fenced block, and it draws the body in the mono family, unwrapped, with the fence's language label above it. It is still **plain** mono — a token colourer would be a new dependency, which this item deliberately does not add.

## 3. Two renderings in one transcript

Everything the agent does is drawn as native UI — the rows grok-build's pager draws — and **nothing is wrapped in a generic bordered card**. There is one reuse rather than one exception: **a segment that runs in the pane's terminal is drawn as a terminal block**, by the same code that draws a block in terminal mode.

That is the whole rule for the surface:

| What | How it is drawn |
|---|---|
| Assistant text | paragraph flow, Markdown, no box |
| A tool call, any tool | inline rows — status, tool, its arguments, its result — collapsed when there is nothing to show |
| A command the agent runs | the terminal block, shared with terminal mode |
| A file edit | diff rows |
| A sub-agent | its own rows, not a card |

Worth stating explicitly, because the current renderer does the opposite: a tool call is a bordered `surface_2` card with a `cpu` icon and the literal label `"tool"` (`chat_message_bubble.rs:39-92`), and its result is a separate `TerminalBlock` with a guessed status. Boxes are what make a transcript read as a list of widgets instead of as a conversation.

## 4. Tool calls: the largest gap

A `ToolCall` is `{ name, arguments }` and nothing else (`chat_content.rs:119-124`). Four things follow:

1. **No status.** There is no running / blocked / done / failed state, no duration, no result on the card. The only reason a finished card looks finished is that it can only be built after the turn.
2. **It appears too late.** `tool_calls` is written to the row only once the turn's stream has ended (`crates/goble-core/src/reasoning.rs:591-613`), so the card pops in at the end rather than when the call starts. Meanwhile `HarnessEvent::ToolCallStarted` fired long before.
3. **The live lifecycle is already on the wire and is thrown away.** `DaemonEvent::ToolCallStarted` / `ToolCallFinished` exist (`crates/goble-daemon-protocol/src/lib.rs:100-110`) and are consumed by the daemon's execution trace only (`crates/goble-desktop-service/src/state.rs:1374-1396`); the chat UI never sees them. This is [`agent-tui.md`](agent-tui.md) A3 — it is a prerequisite here, not a parallel task.
4. **Results are guessed, not known.** A tool result row renders as a `TerminalBlock` whose status is inferred by substring: `body.contains("ERROR:")` → error, otherwise success, and never running (`app/src/state.rs:190-221`). The plain-text block is also the only rendering a result gets — there is no diff view, no file-path chips, no search results, anywhere in `crates/goble-ui` or `app/src`.

**The target is a typed presentation per tool — inline rows, never a box.** A command shows the command and then its output as a terminal block; `read_file` shows a path; an edit shows diff rows; `web_search` shows the query and its sources; `create_agent` shows the sub-agent's own rows. That mapping is a property of the tool, not of the string that happens to be its name, so the presenter should read the same definition the harness dispatches on (`crates/goble-core/src/harness.rs:1022-1073`) rather than match on `call.name` in the renderer.

The second half of the rule matters just as much: **the terminal segment is not re-drawn for the agent.** A command the agent runs produces a real block in the pane's list — [`agent-tui.md`](agent-tui.md) A1's `BlockKind::Shell` with an owner — and the conversation draws that block with the element terminal mode already uses. One block, one renderer, two places it can appear.

## 5. Parsing under streaming

The current behaviour, traced end to end: each assistant delta is appended to the SQLite row (`reasoning.rs:551-577`, `store.rs:947-953`), the daemon emits `chat:updated` **per event** (`goble-desktop-service/src/state.rs:670-672`), the app answers by refreshing conversations (`app/src/root_view.rs:134`), which reloads every pane (`app/src/state.rs:865-880`) — a fresh `list_chat_messages` read of **all** rows and a full `ChatMessage::from_markdown` re-parse of every message (`app/src/state.rs:961-969`) — synchronously, on the UI thread, during layout.

So: N queued deltas cause N full re-reads and N full re-parses of the whole transcript. That is fine for a short conversation and quadratic for a long one, and it is the reason a "streaming-safe parse" has to be designed rather than patched.

Rules the parser needs to hold:

- **A partial construct must not flash as prose.** An unterminated code fence survives today only by luck — `pulldown-cmark` runs an unclosed fence to end of document, so it renders as a code block. Partially typed inline markup (`**bold`, `[x](http`) renders literally until it closes, which is visible flicker at 60 fps.
- **Parse once per change, not once per frame.** The transcript should be parsed when a row changes, and the result cached on the message; a frame with no new delta should re-parse nothing.
- **Keep the tail cheap.** While a turn streams, only the last message changes; re-parsing the rest is wasted work.

## 6. The transcript scrolls and follows the stream

**Done (Q1).** The transcript is built as `Scrollable::new(message_column, Axis::Vertical).with_state(self.scroll.clone())` (`crates/goble-ui/src/views/chat_view.rs`), so it now has a viewport, is clipped to it, handles the wheel and carries an offset. The state is app-owned per pane (`UiState::pane_chat_scroll`, keyed by pane id, surfaced through `PaneChatSnapshot::scroll`) because the tree is rebuilt every frame; a pane's scrollback position and its follow-the-stream flag therefore survive the rebuild.

`ScrollState::following()` (`crates/goble-ui/src/elements/scrollable.rs`) is the opt-in tailing mode: `set_metrics` moves the offset to the new end as content grows, `scroll_by` suspends following once the user leaves the end and resumes it when they come back, and `reset` returns to the top unpinned. A plain `ScrollState::default()` (the sidebar, the settings pane) never moves on its own, so this does not change their behaviour.

One layout bug had to be fixed for this to work at all: `Flex` with `CrossAxisAlignment::Stretch` returned `constraint.max` on the cross axis even when that was unbounded, so every message bubble (a stretch row) reported infinite height and the scroll content measured as infinite — an offset pinned to that "end" would have painted the transcript entirely off-screen. `Flex` now sizes to its content when the cross axis is unbounded (`crates/goble-ui/src/elements/flex.rs`).

`RunningIndicator` (`crates/goble-ui/src/elements/running_indicator.rs`) is still referenced only by its own test.

## 7. Build order

Each row is one turn of work.

| # | Item | Where | Verified by |
|---|---|---|---|
| Q1 | **Transcript scrolling + follow the stream.** Attach a `ScrollState`, clip to the viewport, handle the wheel, and stick to the bottom unless the user has scrolled up | `crates/goble-ui/src/views/chat_view.rs`, `crates/goble-ui/src/elements/scrollable.rs` | element tests for clamp/pin; app test that new content does not move a scrollback position the user chose |
| Q2 | **Markdown fidelity.** Turn on the GFM extensions and handle the fall-through arms: tables, task lists, rules, images; fix nested lists, and stop flattening blockquotes and list items to a string | `markdown.rs`, `chat_content.rs` | parser tests per construct, including the constructs that are untested today |
| Q3 | **Italic renders.** Carry the italic flag through to the span | `inline_text.rs`, `chat_content.rs` | element test: `_x_` is not bold, `***x***` is both |
| Q4 | **Code blocks are mono.** Render a code block in the mono family with its language label, preserving indentation and not word-wrapping it | `chat_message_bubble.rs`, `code.rs` | element test on family and wrap behaviour |
| Q5 | **Tool-call status.** `ToolCall { id, name, arguments, status, result }`, a row written on start as well as finish, and a live overlay from A3 so a running call appears while it runs | `chat_content.rs`, `chat_message_bubble.rs`, A3's wire | app test: a started call is visible before the turn ends |
| Q6 | **Inline tool-call rows.** Replace the bordered `surface_2` card with the inline row the style calls for — status glyph, tool, arguments, result — collapsed when there is nothing to show | `chat_message_bubble.rs` | element test: no border container, and a collapsed call renders in one row |
| Q7 | **Per-tool presentation.** A presenter keyed by the tool definition, not by string matching on the name (command vs path vs diff vs search vs sub-agent) | `chat_content.rs`, tool registry | tests per tool shape |
| Q8 | **Terminal segment as a block.** A command the agent runs renders through the same terminal block element as terminal mode, from the block the bridge produced | `chat_message_bubble.rs`, `agent-terminal-bridge.md` P1/P3 | element test: the same block renders identically in the transcript and in the terminal pane |
| Q9 | **Diff rendering.** Diff rows with line gutters and add/remove colouring — inline, no box | new element in `crates/goble-ui/src/elements/` | element tests on hunks and line counts |
| Q10 | **Parse once per change.** Cache fragments per message and re-parse only what changed, off the layout path | `app/src/state.rs` | app test with a scripted multi-message stream: unchanged messages are not re-parsed |
| Q11 | **Reasoning surface (done).** The model's thinking reaches the app and renders: the wire carries `ReasoningStarted`/`ReasoningDelta`/`ReasoningDone` on both `HarnessServerEvent` and `DaemonEvent`, `map_event` in `crates/goble-harness-internal` passes them through, `goble-desktop-service` emits `chat:reasoning`, and the app overlays each step as its own transcript row, recessed in `Muted` and collapsed to a clickable header until expanded | wire (`goble-harness-protocol`, `goble-harness-internal`, `goble-daemon-protocol`, `goble-daemon`, `goble-desktop-service`), then `app` + `goble-ui` | `cargo test -p goble-harness-internal` (adapter carries the events), `cargo test -p goble-app` (the live event reaches the transcript and paints recessed/collapsed/expandable) |

Q1 is first because nothing else can be judged on a transcript that cannot scroll. Q2–Q4 are the text-formatting half. Q5–Q9 are the agent-activity half: status, the inline row that replaces the card, the per-tool presenter, the terminal block for a command segment, and the diff. Q5 depends on [`agent-tui.md`](agent-tui.md) A3 landing the live events; Q8 depends on the bridge's P1/P3 producing the block. Q10 is the performance floor that keeps the rest affordable once transcripts are long.

## 8. What this does not cover

- **Where the tool calls come from.** The approval composer, the command proposal and the harness suspension are [`agent-tui.md`](agent-tui.md) A5–A6.
- **Where a command's output lives.** For an agent + terminal pane the output is a real block and the parser should read it from there rather than from a tool row — [`agent-terminal-bridge.md`](agent-terminal-bridge.md).
- **Slash commands.** A leading `/` currently just opens the Cmd+K palette with an empty query (`app/src/actions.rs:455-459`); there is no slash registry and the `/` is never stripped. That is a composer feature, not a parsing one, and is not planned here.
- **Attachments and voice.** Both composer actions are stubs that insert placeholder text (`app/src/actions.rs:522-533`).

## 9. Verification

- Parser and element items (Q2–Q4, Q7, Q9): `cargo test -p goble-ui`, with the test named in the row.
- App items (Q1, Q5, Q6, Q8, Q10): `cargo test -p goble-app`.
- Q11 is verified by `cargo test -p goble-harness-internal` (the adapter carries `Reasoning*` instead of dropping it) and `cargo test -p goble-app` (the `chat:reasoning` event reaches the transcript and paints a recessed, collapsed row that expands on click).
- No step here builds or runs the `goble-app` binary. Anything that needs a window to judge — line wrapping in a real pane, the feel of following a stream — is recorded as unverified rather than claimed.

## 10. Follow-ups found by the first rollout

The first end-to-end rollout (`.grok/workflows/agent-ui-rollout.rhai`) implemented Q1–Q11 and the bridge items; every item passed its own acceptance command, and the run's integration pass — a full suite, a regression review, a card-free audit and a bookkeeping audit — then found what per-item verification cannot see. Those items are tracked as R1–R6 in [`../TRACKER.md`](../TRACKER.md); three of them land on this doc.

- **R3 — a link inside a paragraph stopped being clickable.** Q2 folded links into the inline flow as `InlineSpan::link` (`chat_content.rs:423`), but `chat_message_bubble.rs:512-515` paints a paragraph as bare `InlineText`, and the older `ChatBlock::Action { OpenUrl }` path and its test `link_fragment_becomes_interactive_action` were deleted. Styling a link is not making it interactive.
- **R4 — a tool-result terminal segment still is not the shared block.** `crates/goble-ui/src/elements/group_chat_message.rs:288` renders `ChatFragmentKind::Terminal` as `Empty` instead of the one `terminal_block` renderer Q8 built. Q8's rule — one block, one renderer, two places it can appear — is not yet true for this path.
- **R5 — status still travels as a string.** `crates/goble-core/src/reasoning.rs:726`, `:925-928` encode a tool result's status as the literal `ERROR: ` prefix and `app/src/state.rs::tool_terminal_data` renders that text, so Q6's "read the real status from the persisted field" is true on the row and not yet true on the result.

The bookkeeping audit found one false claim — a resolver row naming a test that Q7 had renamed — which was corrected directly rather than left to a repair pass.
