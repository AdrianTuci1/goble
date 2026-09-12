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

- **R3 — a link inside a paragraph stopped being clickable (fixed).** Q2 folded links into the inline flow as `InlineSpan::link` (`chat_content.rs:423`), but `chat_message_bubble.rs:512-515` painted a paragraph as bare `InlineText`, and the older `ChatBlock::Action { OpenUrl }` path and its test `link_fragment_becomes_interactive_action` were deleted. Styling a link is not making it interactive. `TextSpan` now carries an optional click handler and `InlineText` hit-tests each placed run and dispatches the click; the paragraph renderer attaches the run's handler so an `InlineStyle::Link(url)` span calls `on_action(OpenUrl(url))`. The click reaches `ChatMessageBubble::on_action` and is relayed by `ChatView::with_on_action`; `app/src/ui/chat.rs` still never calls it and the app has no external-URL opener, so in the app the action terminates at that unwired slot (wiring an OS opener was out of scope). Restored test: `chat_message_bubble::tests::link_fragment_becomes_interactive_action`.
- **R4 — a tool-result terminal segment still is not the shared block (fixed).** `crates/goble-ui/src/elements/group_chat_message.rs:288` rendered `ChatFragmentKind::Terminal` as `Empty` instead of the one `terminal_block` renderer Q8 built, so Q8's rule — one block, one renderer, two places it can appear — was not true for this path. `GroupChatMessage::build_content_column` now flushes the pending inline run and draws the fragment through `render_terminal`, which calls `terminal_block(data, TerminalFilter::default(), None, None)`; the fragment thus draws the same block the transcript's command tool call and terminal mode draw. New test: `group_chat_message::tests::terminal_fragment_draws_the_shared_block`, which compares the fragment's drawn runs (`text, size`) against `terminal_block`'s.
- **R5 — status still travels as a string (fixed).** `crates/goble-core/src/reasoning.rs:726`, `:925-928` encoded a tool result's status as the literal `ERROR: ` prefix and `app/src/state.rs::tool_terminal_data` rendered that text, so Q6's "read the real status from the persisted field" was true on the row and not yet true on the result. The result row is now `<call_id>\n<body>` on both the immediate and the resumed path; `tool_terminal_data` takes the status and `MessageParseCache::resolve` resolves it from the call record on the assistant row's `tool_calls` column (the cache also invalidates a result row when only that resolved status changed), so nothing reads the result text. `app/src/ui/terminal.rs`'s agent view reads the block's own `BlockState`/`exit_code` instead of a `failed` boolean. Tests: `reasoning::tests::test_failed_tool_result_row_does_not_encode_status_as_text`, `state::pane_session_tests::a_tool_result_reads_its_status_from_the_call_record`, `ui::terminal::tests::the_agent_view_never_draws_a_running_block_as_success`.

The bookkeeping audit found one false claim — a resolver row naming a test that Q7 had renamed — which was corrected directly rather than left to a repair pass.

## 11. Transcript fidelity: the fold, highlighting, and the diff

The second rollout closed six of those findings. What it did not touch is the part of the transcript that has to *look* like a development tool: a tool call as a foldable row, code coloured as code, a diff whose changed lines sit on a green or red band. grok-build is the reference for all of it — not to copy bytes from, but to match behaviour — and the two decisions below are made, not open.

- **H1 — highlighting infrastructure.** `syntect` (default features off; its pure-Rust regex backend, no C dependency) plus `two-face` for bat's grammar set, and **our own** `.tmTheme` under `crates/goble-ui/assets/` written in grok-build's visual idiom. Nothing is copied out of `~/Projects/grok-build` (the harness reference) or `~/Projects/warp-new` (AGPL) — not a theme, not a grammar. One entry point turns `(text, language)` into styled spans; the language is resolved from a file extension, a markdown fence info string, or the `bash` token a command carries. syntect's foreground colours are kept and its backgrounds dropped, the way grok-build's `syntect_to_ratatui_fg` does. This supersedes Q4's "do not add a syntax-highlighting dependency": the owner asked for coloured code after Q4 landed, so the dependency is now the requirement.
- **H2 — a read is an IDE excerpt.** Collapsed, a read is the path alone (that is A4's default for this presentation). Expanded, it is the file's lines with a right-aligned line-number gutter and H1's colours, on their own background band — the shape grok-build's `ReadToolCallBlock::render_content_lines` builds.
- **H3 — the diff bands.** The band already exists (`crates/goble-ui/src/elements/diff.rs:461` fills each changed row with `surface.mix(&color, 0.10)`) but a 10% tint does not read as *added* or *removed*, and each changed line's text is a single flat colour. Saturate our own insert/delete background tokens to our dark palette, and run the changed lines through H1 so the code is coloured inside the band.
- **H4 — a command's own colours.** `TerminalData::for_command` flattens the tool result onto one colour per line, so an agent command's output loses the ANSI colours it emitted, while the pane keeps them because the emulator draws it. The command line itself is highlighted as shell. Reuse `crates/goble-terminal`'s parser if it accepts a plain byte string, otherwise a bounded SGR parser; either way the transcript and the pane must agree on what a command looked like.
- **R7 — where a link click ends (fixed).** R3 made the link a live target and stopped there: nothing in `app/src/` called `with_on_action`, and the app had no external-URL opener at all. The design said where it terminates — `app/src/ui/chat.rs` passes the action through, and the app opens the URL behind a scheme guard (http/https only, never an arbitrary string) — and that is now wired: `chat_action_relay` relays each fragment's `ChatAction::OpenUrl` to `UiActions::on_open_url` (`app/src/actions.rs`), which hands the URL to `app/src/state.rs::open_external_url`; `web_scheme` accepts only a well-formed `http`/`https` URL and every other scheme is refused before the platform opener (`open`/`xdg-open`/`explorer`, invoked with the URL as a single argument, never through a shell) is reached. The `rdp://`/`goble://desktop` handoff path is unchanged. Tests: `ui::chat::link_action_tests::{a_link_click_reaches_the_app_opener_with_its_url,a_non_http_scheme_click_is_refused_before_the_opener}` and `state::external_url_tests::{web_scheme_accepts_only_http_and_https,open_external_url_refuses_non_http_schemes}`.
- **R8 — the pane's executed-command block (fixed).** `executed_command_block` (`app/src/ui/terminal.rs:90`) hard-coded `TerminalStatus::Success` while `block_status` (`:113`) reads the block's real state and exit code. One of the two was wrong; the block is the one the function now reads. It takes the newest visible block that carries a command — a finished command leaves an empty prompt block behind it, so `last()` is not it — and maps it through `block_status`, so a command still running draws `Running` and a finished non-zero exit draws `Error`. Test: `ui::terminal::tests::the_executed_command_block_reads_the_blocks_status` (a session fed `Bootstrapped`+`Preexec` with no `CommandFinished` is `Running`; one closed with exit 2 is `Error`).

### Found by the second rollout's integration pass

Three defects survived those items' own acceptance tests, because each test proved its own path and not the wiring between paths.

- **R14 — one command still draws two ways.** H4 taught `TerminalData::for_command` to highlight the command line as shell and to keep the output's ANSI colours, and the pane (`app/src/ui/terminal.rs:150`) and the assistant row's tool call (`chat_message_bubble.rs:382`) both go through it — but the app's persisted tool-result row builds its block by hand with `TerminalData::new` + `TerminalLine::output` (`app/src/state.rs:278-301`), so the third path silently keeps the old flat rendering. One command must have one rendering; that path goes through `for_command` too.
- **R12 — R5 took the model's only failure signal.** Removing the `ERROR: ` prefix was right for the UI, and the status really is persisted on the assistant row — but `build_history` (`crates/goble-core/src/reasoning.rs:966-969`) hands the tool row's text to the *model*, and the structured status never reaches it: `tool_calls` deserializes into `LlmToolCall` (`crates/goble-core/src/llm.rs:8-12`), which has no status field. Before R5 the model read `ERROR: <message>`; now it reads `<message>`. Whatever replaces it must be visible to the model and not only to the renderer — and it must not be a prefix re-added for the UI's benefit.
- **R11 — the new approval state is not cleared.** R1 added `pending_command` and `command_selection` to the pane's runtime and `finish_turn` clears them (`app/src/state.rs:1523-1524`), but the other paths that reset a pane's transcript state do not: `on_clear_transcript` (`app/src/actions.rs:633-644`), the store-less branch of `bind_active_pane_conversation` (`app/src/state.rs:1623-1625`) and `on_stop` (`app/src/actions.rs:645-654`). A stale approval card can outlive the transcript it belonged to. The rule to keep: every clear path resets every suspension field that exists.

## 12. Tool-call parsing matched to grok-build, and the pill lock (done)

Q7's "per-tool presentation" is now grok-build's own parse, name for name, and the
surface is locked pill-free. Two pieces, both landed:

- **The parse is one function, shared by every shape.** `goble_core::harness::presentation`
  answers `tool_row(name, arguments, result) -> ToolRow { verb, subject, detail, kind }`
  and `tool_kind_for(name) -> ToolKind` (`Execute, Read, Edit, Create, List, Search,
  WebSearch, WebFetch, SearchTools, UseTool, MemorySearch, Skill, SubAgent, Other`).
  grok-build's names come first (`read`, `edit`, `write`, `bash`, `grep`, `ls`,
  `web_fetch`, `search_tool`, `use_tool`, `memory_search`, `skill`), then our harness
  names (`crates/goble-core/src/harness/definitions.rs`). The row is the bold verb, the
  operand the arguments carry and the dim detail the result states — `● Read src/lib.rs`,
  `Edit src/lib.rs +1/-1`, `Run cargo test`, `Search "fn build" in src`,
  `Subagent “audit the migration” running · reading · 4.5s` — never the raw tool name and
  never the raw argument JSON. Core's `the_parse_answers_for_every_defined_tool` iterates
  every defined tool, so the parse cannot drift from the definitions.
- **The transcript draws that row and nothing around it.** `tool_call_header`
  (`crates/goble-ui/src/elements/chat_message_bubble/tool_call.rs`) paints one line of
  runs inside a paint-free `Chip`: the status glyph, the bold verb, the subject and the
  detail. The bodies stay the family's own (`tool_body.rs`): a command is the shared
  terminal block, a read is an excerpt, an edit is a diff, a sub-agent is its own rows.

**The pill lock.** `crates/goble-ui/src/test_util.rs` now owns the predicate
(`is_pill`, `pills`, `assert_pill_free`, `assert_no_row_border`, `pill_rects`), and every
agent row is tested against it:

| Level | Test |
|---|---|
| Element | `chat_message_bubble::tests::pill::{no_agent_row_paints_a_pill,no_sub_agent_row_paints_a_pill,no_interruption_panel_paints_a_pill,a_tool_result_row_draws_only_the_shared_block}` — prose and markdown, both roles, reasoning collapsed and expanded, every family in every fold and in `Pending`/`Running`/`Error`, every sub-agent status, the ask card, the footer |
| Element (tripwire) | `the_lock_catches_a_row_wrapped_in_a_card` — the same row wrapped in a rounded card *is* reported, and unwrapped is not |
| Pane | `views::chat_view::tests::the_agent_transcript_paints_no_pill` — the whole pane in one frame, with a tool call in flight, an interrupted ask, a command proposal, a queued prompt and the footer |
| App | `ui::chat::agent::agent_pill_tests::the_agent_pane_paints_no_pill_around_its_rows` — the `RootView` over a real store, no pill covering any row's own run |

What counts as a pill, and what does not: a rounded fill that wraps a row, and a border
with no control under it, fail. Two shapes pass, and both are deliberate: a **bordered
control** (a border and a fill over the same rect — the shared terminal command block's
card, or a button such as the ask card's option), and a **text highlight** (a rounded fill
no taller than the line of text it backs — an inline code span). The pane's own square
split frame encloses every row and is window chrome, not a row's box; the app-level test
excludes it by that property and asserts it is square.
