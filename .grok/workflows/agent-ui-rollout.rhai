// Implement the agent-UI plan (.agents/06-renderer/agent-parser.md, agent-tui.md,
// agent-terminal-bridge.md) as a long-running task: one tracker item at a time,
// each one adversarially verified with the repo's own test commands.
//
// Standing rules encoded in every prompt: never build/run the goble-app binary,
// one item per agent, no placeholders, no commits, tracker updated only with proof.

let meta = #{
    name: "agent-ui-rollout",
    description: "Implement the agent-UI plan in dependency waves - text pipeline, tool-call status, terminal bridge, card-free surface, live chrome, sub-agents - one tracker item at a time, each adversarially verified with the repo's own test commands.",
    when_to_use: "Use to advance the pending items of .agents/06-renderer/agent-parser.md, agent-tui.md, agent-terminal-bridge.md and .agents/04-agent-runtime/subagents.md. Never builds or runs the GUI binary; leaves every change uncommitted for review.",
    phases: [
        #{ title: "Wave 1: Foundations", detail: "shell hooks, transcript scrolling, parse-once" },
        #{ title: "Wave 2: Text", detail: "markdown, italic, mono code, diff rows" },
        #{ title: "Wave 3: Carriers", detail: "tool-call status, live events, reasoning" },
        #{ title: "Wave 4: Bridge", detail: "block ownership, claim protocol, pane routing" },
        #{ title: "Wave 5: Surface", detail: "inline rows, presenters, agent view, approval" },
        #{ title: "Wave 6: Integration", detail: "full suite plus adversarial review of the whole diff" },
        #{ title: "Wave 7: Integration fixes", detail: "the findings the first run's integration pass caught" },
        #{ title: "Wave 8: Transcript fidelity", detail: "the tool-call fold, syntax highlighting, the IDE-shaped read, the diff bands" },
        #{ title: "Wave 9: Second-pass fixes", detail: "the findings this rollout's integration pass left open" },
        #{ title: "Wave 10: Open defects", detail: "the five findings the second rollout's integration pass left registered" },
        #{ title: "Wave 11: Live chrome", detail: "the activity footer, the topbar indicator and the work the overlay will show" },
        #{ title: "Wave 12: Sub-agents", detail: "model and budget, spawn, background and cancel, the wire, the transcript row, the child view, the overlay" },
    ],
};

// ---------------------------------------------------------------- args

let waves_arg = if args == () { "all" } else if args.waves == () { "all" } else { args.waves };
// `waves: "ids:S5,S6"` runs only those items; `passed: "S1,S2"` seeds their deps as
// already verified so a resumed run does not re-implement what already passed.
let passed_arg = if args == () || args.passed == () { "" } else { args.passed };
// Model routing: every role runs on DeepSeek - the implementers and repairs that
// produce edits as well as the verifiers and the review panel that only trawl the
// tree and run one command. No qwen. `deepseek` is the config entry whose model is
// `deepseek-flash`; the model slug list is read once at startup, so an edit to
// ~/.grok/config.toml only takes effect after a restart.
let write_model = if args == () || args.write_model == () { "deepseek" } else { args.write_model };
let read_model = if args == () || args.read_model == () { "deepseek" } else { args.read_model };
let max_repairs = if args == () || args.max_repairs == () { 1 } else { args.max_repairs };
let root = if args == () || args.goble_root == () { "." } else { args.goble_root };

// ---------------------------------------------------------------- schemas

let impl_schema = #{
    "type": "object", "required": ["id", "status", "evidence"],
    "properties": #{
        "id": #{ "type": "string" },
        "status": #{ "type": "string" },
        "files_changed": #{ "type": "array", "items": #{ "type": "string" } },
        "commands": #{ "type": "array", "items": #{ "type": "string" } },
        "evidence": #{ "type": "string" },
        "tracker_updated": #{ "type": "boolean" },
        "blocked_reason": #{ "type": "string" },
    },
};
let verdict_schema = #{
    "type": "object", "required": ["id", "passes", "evidence"],
    "properties": #{
        "id": #{ "type": "string" },
        "passes": #{ "type": "boolean" },
        "acceptance_met": #{ "type": "array", "items": #{ "type": "string" } },
        "acceptance_missing": #{ "type": "array", "items": #{ "type": "string" } },
        "evidence": #{ "type": "string" },
        "reason": #{ "type": "string" },
    },
};
let check_schema = #{
    "type": "object", "required": ["lens", "ok", "evidence"],
    "properties": #{
        "lens": #{ "type": "string" },
        "ok": #{ "type": "boolean" },
        "findings": #{ "type": "array", "items": #{ "type": "string" } },
        "evidence": #{ "type": "string" },
    },
};

// ---------------------------------------------------------------- helpers

fn join_list(arr, sep) {
    let out = "";
    if arr == () { return out; }
    let i = 0;
    for el in arr {
        if i > 0 { out += sep; }
        out += el;
        i += 1;
    }
    return out;
}

fn wave_selected(spec, n) {
    if spec == "all" { return true; }
    let wanted = n.to_string();
    for part in spec.split(",") {
        let p = part;
        p.trim();
        if p == wanted { return true; }
    }
    return false;
}

// `ids:A,B` selects single items across every wave, so a run stopped and picked up
// later can name exactly what is left instead of redoing a whole wave.
fn item_selected(spec, wnum, id) {
    if spec == "all" { return true; }
    if !spec.starts_with("ids:") { return wave_selected(spec, wnum); }
    // Rhai's `split` returns the pieces, so the "ids:" prefix is dropped by
    // splitting on the colon rather than by slicing the string.
    let halves = spec.split(":");
    if halves.len() < 2 { return false; }
    for part in halves[1].split(",") {
        let p = part;
        p.trim();
        if p == id { return true; }
    }
    return false;
}

fn with_model(opts, model) {
    if model != "" { opts["model"] = model; }
    return opts;
}

fn item(id, ids, title, doc, files, deps, goal, verify) {
    return #{
        id: id, ids: ids, title: title, doc: doc,
        files: files, deps: deps, goal: goal, verify: verify,
    };
}

fn house_rules(root) {
    let s = "HOUSE RULES for the Rust repo at " + root + " (non-negotiable):\n";
    s += "- NEVER build, run or install the goble-app GUI binary: no `cargo build`, no `cargo run`, no `cargo install`, never launch the app. The repository owner explicitly forbids it. Permitted: `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt --check`, `rustfmt`, `git status`, `git diff`, `grep`, `bash -n`, `zsh -n`.\n";
    s += "- Resolve EXACTLY ONE item. No drive-by refactors, no fixing other items, no placeholders, no `TODO: rest is left`.\n";
    s += "- Read before writing: `.agents/GUIDE.md`, the owning `.agents/06-renderer/` doc, and the real code. Do not guess APIs.\n";
    s += "- Verify with the item's own command and paste its real output. Never claim anything you did not observe.\n";
    s += "- Do not run `git commit` or `git push`. Leave the working tree dirty for review.\n";
    s += "- Finish by updating `.agents/TRACKER.md` (mark the item `[x]` ONLY if your acceptance command passed just now; otherwise `[~]` with the reason) and adding one row to the `Recently resolved` table in `.agents/RESOLVER.md` with the exact command and its result.\n";
    return s;
}

fn impl_prompt(it, rules, root) {
    let p = "Implement ONE tracker item in the Rust repo at " + root + ".\n\n";
    p += "ITEM: " + it.ids + " - " + it.title + "\n";
    p += "Owning design doc (read it first): .agents/06-renderer/" + it.doc + "\n";
    p += "Likely files: " + it.files + "\n";
    p += "Acceptance (the definition of done): " + it.verify + "\n\n";
    p += "WHAT TO DO:\n" + it.goal + "\n\n";
    p += rules;
    p += "\nReport: id, status (done|partial|failed), files_changed, every command you ran, evidence (the summary line you actually saw), tracker_updated.";
    return p;
}

fn verify_prompt(it, claim, root) {
    let p = "You are an ADVERSARIAL VERIFIER for one tracker item in the Rust repo at " + root + ". Assume the implementer is wrong until you prove otherwise with your own eyes.\n\n";
    p += "ITEM: " + it.ids + " - " + it.title + " (design doc .agents/06-renderer/" + it.doc + ")\n";
    p += "The acceptance criterion it must satisfy: " + it.verify + "\n";
    p += "The implementer claims: " + claim + "\n\n";
    p += "DO THIS: inspect the real tree (`git status --short`, `git diff`), read the changed files on disk, then run the acceptance command YOURSELF and read its actual output. If cargo reports a file-lock wait, wait for it and read the result. ";
    p += "Set passes=true ONLY if you personally observed the acceptance criterion pass. A missing file, a stub, a TODO, a test that does not exercise the claim, or a command you could not run means passes=false.\n";
    p += "Report acceptance_met, acceptance_missing, evidence (the exact lines you saw), reason.\n";
    p += "You must NOT edit any file and must NOT commit. Read and run commands only.\n";
    p += "House rule: never build or run the goble-app GUI binary - `cargo build`, `cargo run` and `cargo install` are forbidden here. Use `cargo test`, `cargo check`, `cargo clippy`.\n";
    return p;
}

fn repair_prompt(it, rules, root, reason, evidence) {
    let p = impl_prompt(it, rules, root);
    p += "\n\nA previous attempt at this item FAILED adversarial verification.\nReason: " + reason + "\nEvidence: " + evidence + "\n";
    p += "Fix the actual defect in the code - do not restate the claim and do not weaken the test. Re-run the acceptance command and report its real output.";
    return p;
}

// ---------------------------------------------------------------- goals

let g_b0 = "Write the shell-integration scripts (bash and zsh) that emit the DCS hex-JSON hook channel the Rust side already decodes. Read `crates/goble-terminal/src/hooks.rs` FIRST: its payload schema is authoritative and the scripts must match it exactly. Cover the handshake (`InitShell` at startup, `Bootstrapped` when the bootstrap finishes) and the per-command hooks (`Preexec` before the command runs, `CommandFinished` with the exit code, `Precmd` with cwd/git info and the honor_ps1 bit), and suppress the shell's own PS1 while the integration is active, restoring it on teardown. The exit criterion from the design doc is exact: a real shell session emits a parseable CommandFinished with an exit code for every command. Put the scripts under a new assets directory in crates/goble-terminal (e.g. crates/goble-terminal/assets/shell/) and wire the pane's PTY spawn in `app/src/terminal.rs` (portable_pty CommandBuilder) so the shell sources them. If a piece of the handshake cannot work without support the Rust side does not have yet, say so precisely in your report instead of inventing protocol.";

let g_q1 = "Make the transcript scroll and follow the stream. Today `crates/goble-ui/src/views/chat_view.rs:799` builds `Scrollable::new(message_column, Axis::Vertical)` with NO `with_state(...)`, so per the element's own contract (`crates/goble-ui/src/elements/scrollable.rs:158-163`) there is no viewport, no clipping, no wheel handling and no offset: a conversation taller than the pane simply overflows. Attach a ScrollState, clip to the viewport, handle the wheel, and pin to the bottom as new content arrives - unless the user has scrolled up, in which case hold their position and resume pinning only when they come back to the bottom. Implement the behaviour in the element layer with element tests for clamp and pin, and wire it in the chat view.";

let g_q10 = "Parse once per change, not once per frame. Today each assistant delta is appended to the SQLite row, the daemon emits `chat:updated` per event, and the app answers by re-reading every row and re-running `ChatMessage::from_markdown` on every message, synchronously on the UI thread during layout (`app/src/state.rs:865-880`, `:961-969`) - so N deltas cost N full re-reads and N full re-parses. Cache the parsed fragments per message, keyed on the row content so an edit invalidates the entry, and re-parse only the message that actually changed. A frame with no new delta must re-parse nothing. Do not change the rendering itself.";

let g_q2 = "Markdown fidelity. `crates/goble-ui/src/elements/markdown.rs:16` constructs `Parser::new(input)` with default options, so no GFM extension is on and tables and task lists never tokenise. Beyond that: `Event::Rule` is dropped by the `_ => {}` arm (`:186`), images render as alt text (`:77`), raw HTML is shown verbatim as text (`:147-157`), nested lists are broken because `Tag::List` clears the shared `list_items` buffer (`:51-59`), blockquotes and list items are flattened to a single String (`:99-105`, `:85-98`), and ordered-list numbering restarts at 1 (`chat_message_bubble.rs:247`). Enable the GFM extensions and handle those constructs properly, keeping paragraph structure inside blockquotes and list items. Also fix the paragraph break: `group_fragments_into_blocks` refuses to fold a Link into the inline flow (`chat_content.rs:306-308`), so a link mid-sentence becomes its own line - it must stay in the paragraph.";

let g_q3 = "Make italic render. `crates/goble-ui/src/elements/inline_text.rs:274` parses the emphasis flag and throws it away (`let _ = italic;`), and the rendering tuple hardcodes `FontFamily::System` and `ColorToken::Text`. Carry the italic flag through to the span and render it as italic. After this, `_x_` is italic and not bold, and `***x***` is both bold and italic.";

let g_q4 = "Make code blocks monospaced. A fenced code block currently renders through `Text` (`chat_message_bubble.rs:216-239`), i.e. the system font. `crates/goble-ui/src/elements/code.rs` looks like the intended home but is dead code: its `paint` draws nothing (`:83-85`), it hardcodes the system family (`text.rs:11-19`), and nothing constructs it. Render a code block in the mono family, preserving indentation and not word-wrapping it, with the fence's language label available to the caller. Do NOT add a syntax-highlighting dependency - check `crates/goble-ui/Cargo.toml` first, and if none exists, render plain mono and say so in your report.";

let g_q9 = "Add diff rendering. No diff view exists anywhere today: a tool result is a plain-text block. Add an element under `crates/goble-ui/src/elements/` that renders unified-diff rows: line-number gutters (which column is populated signals the change type), add/remove colouring drawn from the theme's existing tokens, and a separator row for skipped context between hunks. It is inline - rows and rails, no border and no box; this product never draws a bordered card. Expose a constructor over parsed hunks so a caller never hands it a raw string.";

let g_a2q5 = "Build the tool-call status carrier, persisted so a re-read renders terminal states. `ToolCall { name, arguments }` (`crates/goble-ui/src/elements/chat_content.rs:119-124`) has nowhere to put a status, and the `tool_calls` column is written only after the turn's stream has ended (`crates/goble-core/src/reasoning.rs:591-613`), so a call cannot be drawn while it runs. Add `id`, `status` and `result` per call and write the row on start as well as on finish/error. A row persisted by an older build - missing the new fields - must still parse. This one change satisfies BOTH tracker items A2 (the persisted carrier) and Q5 (the row written on start): mark both.";

let g_a3 = "Get live tool events to the app. `chat:updated` is emitted for every daemon event and the native UI reacts by re-reading the store (`app/src/root_view.rs:134`), so nothing unpersisted is ever visible. Emit a structured `chat:tool` event from the harness's `ToolCallStarted`/`ToolCallFinished`/`ToolCallError` - the events already exist on the wire (`crates/goble-daemon-protocol/src/lib.rs:100-110`) but today only feed the daemon's execution trace (`crates/goble-desktop-service/src/state.rs:1374-1396`). Hold a per-pane in-flight map and overlay it on the persisted rows while the turn runs, so a running call is visible before the turn ends.";

let g_q11 = "Carry the model's reasoning to the app and render it. Every `Reasoning*` event is dropped by the wire adapter (`crates/goble-harness-internal/src/lib.rs:265-268`) and `crates/goble-daemon-protocol` has no such variant, so reasoning is not merely unrendered - it never reaches the app. Add the protocol variant, carry the events through the adapter, and render the thinking as its own rows in the transcript: visually recessed (dimmed or blended toward the background), collapsed by default and expandable on demand. The plan marks this item blocked; if unblocking it needs materially more than the variant plus passthrough, stop at the adapter, record exactly what is still missing, and leave the tracker item `[~]` rather than guessing.";

let g_p1 = "Give blocks an owner. Add `BlockOwner::{User, Agent { conversation_id, call_id }}` to `crates/goble-terminal/src/blocks.rs`, a `Block::owner()` accessor, and `BlockList::claim_next_pre_exec(id)`: a `Preexec` hook with a pending claim attaches that owner, an unclaimed `Preexec` belongs to the user, and a claim that would attach to the wrong block is refused rather than silently accepted. A1 already landed BlockKind and the view filters - read that code before adding to it.";

let g_p2 = "Implement the claim protocol in the pane. In `app/src/terminal.rs` (the pty lives here, via portable_pty), add `TerminalSession::claim_command(...)`: write the command to the pty, hold the claim, resolve it on the next `Preexec` hook, and FAIL it on a bounded timeout. Rule three of the design doc is the point: a claim must never silently attach to the wrong block, and a claim that is still pending when a different command's `Preexec` arrives is a failure, not a guess. Depends on P1.";

let g_p3 = "Extract the tool result from a claimed block. On `CommandFinished` for a block whose owner is an agent claim, produce the output text and the exit code and hand them back to the caller. Cover the cases the design doc names: exit 0 with output, non-zero exit, empty output, interruption (exit 130), and background promotion - a `Precmd` arriving while the command still runs promotes the block, and the tool result is then 'still running' plus the output that arrived, never a hang. Depends on P1.";

let g_p4 = "Route the agent's shell tool through the pane instead of the sandbox. Today `SandboxedCommandRunner` (`crates/goble-core/src/harness.rs:167-188`) spawns its own process, allow-lists nine commands and times out at 60s, so an agent-issued command is invisible in the terminal. For an agent+terminal pane the shell tool must run in the pane's own session; a pane with no terminal keeps the sandbox. This is a deliberate change of security posture - the command now runs in the user's own shell with the user's environment and credentials - so the sandbox path must stay reachable and the trade must be written plainly in your report. Depends on P2 and P3.";

let g_p6 = "Feed the owner into the views. Use the block owner added by P1 with `BlockList::associate_with_conversation` so that a command the agent ran appears inside its conversation's view and a command the user typed does not. Nothing calls associate_with_conversation today; this is what makes the two views agree with reality after a mixed sequence of user and agent commands. Depends on P1.";

let g_q6 = "Replace the tool-call card with inline rows. The renderer draws a tool call as a bordered `surface_2` card with a `cpu` icon and the literal label \"tool\" (`crates/goble-ui/src/elements/chat_message_bubble.rs:39-92`), and the result as a separate block whose status is guessed by substring - `body.contains(\"ERROR:\")` (`app/src/state.rs:190-221`). Both go. A tool call is drawn as inline rows in the pager idiom: a status affordance on the first row, then the tool, its arguments and its result; collapsed to a single row when there is nothing to show, expanded in place when there is. No border, no box, no card, and no guessed status - read the real status from the persisted field. Depends on A2/Q5.";

let g_q7 = "Give each tool its own presentation, keyed on the tool definition rather than on its name string. A command shows the command and then its output; a read shows a path; an edit shows diff rows; a web search shows the query and its sources; a sub-agent shows its own rows. The mapping is a property of the tool, so read the same definition the harness dispatches on (`crates/goble-core/src/harness.rs:1022-1073`) instead of matching `call.name` inside the renderer, and use the diff element added by Q9 for the edit shape. Depends on Q6.";

let g_q8 = "Draw the terminal segment as a block. A command the agent runs must render through the same element that terminal mode already uses to draw a block - one block, one renderer, two places it can appear - instead of being redrawn as agent output. Read `app/src/ui/terminal.rs` to see how the pane paints a block, and reuse that path from the transcript for a block whose owner is the conversation. Depends on P3 and A2/Q5.";

let g_a6 = "Add the harness approval suspension. There is no approval gate today: `run_command` expands credential placeholders and calls the runner (`crates/goble-core/src/harness.rs:1358-1375`) behind the runner's allow-list only. Suspend before a command tool executes, the same way `ask_user` already suspends (`:371-376`, consumed in `crates/goble-core/src/reasoning.rs:448`), and emit a `CommandProposed` event carrying the candidates and the cwd. Resume on the decision: approve runs the chosen text verbatim, edit runs the edited text, reject becomes a `ToolCallError`. The existing `auto_approve` bit is the single switch that skips the suspension, so keep both features on it.";

let g_a5 = "Give the composer a proposal mode. `crates/goble-ui/src/elements/chat_composer.rs` shows a command proposal with its candidate variants, allows editing it in place, approving or rejecting it, and cycling between candidates from the keyboard. It renders what the harness hands it - candidate generation is a model call and stays in the harness. Depends on A6, which emits the proposal.";

let g_a7 = "Make the agent view work over the block list. Cmd+Enter pushes the agent-view block and switches the pane's view filter; Esc returns to the terminal; the card left behind in the terminal is clickable to come back. Wire it in `app/src/ui/terminal.rs` and `app/src/state.rs` - A1 already landed the block kind, the visibility set and the two view filters.";

let g_a8 = "Add the conversation card element. The card stands for 'there is a conversation here, click to enter': conversation title, status, last activity, and a click handler, as an element under `crates/goble-ui/src/elements/`. It reads the same block that A7 pushes into the list, and it is drawn in the pager idiom - a row with a rail or accent, not a bordered widget.";

// ------------------------------------------------ wave 7 goals (verified findings)

let g_r1 = "The approval gate must actually run commands. Today `if !auto_approve && call.name == COMMAND_TOOL` in `crates/goble-core/src/reasoning.rs:664` suspends the call, persists a pending command and yields `HarnessEvent::CommandProposed`, then returns; `auto_approve` defaults to false (`crates/goble-core/src/harness.rs:515`, `app/src/state.rs:815`, `:1786`). That event is then discarded: `crates/goble-harness-internal/src/lib.rs:255` maps `HarnessEvent::CommandProposed { .. } => Vec::new()`, and `Harness::resume_command` (`harness.rs:662` -> `reasoning.rs:856`) is called only from tests. So under the default configuration no command tool ever runs and the turn never closes - a suspension with no path back. Wire the path end to end: carry the proposal through the harness/daemon boundary, emit a structured event the app can see (the desktop service already emits `chat:reasoning` for a comparable case), hold the pending proposal per pane, answer it from A5's composer proposal UI with approve/edit/reject, and resume the turn through the existing resume path. A reject must produce a `ToolCallError`, and every exit path must still emit the turn-finished event so the pane cannot stay busy. Read `agent-tui.md` section 5 and section 9 first: the design is written, the wire is what is missing.";

let g_r2 = "Pane routing must fall back to the sandbox when the pane's shell cannot answer. `fn shell_command` in `app/src/terminal.rs` (around line 403) writes the hook scripts and bootstraps only bash and zsh, and spawns any other shell unchanged; but `TerminalSession::pane_session` (`app/src/terminal.rs:1300-1316`) returns a `PaneCommandRunner` for any pane that has a session. Under fish or any other shell the agent's command is written to the pty, no `Preexec` hook ever arrives, and the claim fails after `CLAIM_TIMEOUT` (30s) instead of running - where before this work the same command ran through `SandboxedCommandRunner`. Fix both halves: the pane must offer a session to the shell tool only when its shell was actually bootstrapped (otherwise return None so the harness keeps the sandbox), and a claim that can never resolve must fail immediately and visibly rather than costing the full timeout. Read `agent-terminal-bridge.md` section 9 for the finding.";

let g_r3 = "Restore clickable links inside a paragraph. Q2 moved markdown links into the inline flow as `InlineSpan::link` (`crates/goble-ui/src/elements/chat_content.rs:423`), but `crates/goble-ui/src/elements/chat_message_bubble.rs:512-515` paints a paragraph as a bare `InlineText`, and the previous `ChatBlock::Action { OpenUrl }` path and its test `link_fragment_becomes_interactive_action` were deleted. A link must be a real hit target again, not just styled text: carry the link's action through the inline span to the painted element and make the click reach something - the app never wired `ChatView::with_on_action`, so wire the path or state precisely where the click terminates and why. Bring back a test named `link_fragment_becomes_interactive_action` that proves a link inside a paragraph is one live action target carrying its URL.";

let g_r4 = "One renderer for a tool-result terminal segment. `crates/goble-ui/src/elements/group_chat_message.rs:288` renders `ChatFragmentKind::Terminal` as `Empty::new()`, so a tool result that is a terminal segment disappears instead of drawing its block. Q8 built the shared renderer (`crates/goble-ui/src/elements/terminal_block.rs::terminal_block`, fed by `TerminalData::for_command`) and `app/src/ui/terminal.rs::executed_command_block` is the pane path that feeds it. Make this path draw the same block as the transcript's command tool call and as terminal mode - one block, one renderer, two places it can appear - and delete whatever bespoke drawing it replaces. Read `agent-parser.md` section 10.";

let g_r5 = "Stop encoding status as a string, and stop inferring it from a boolean. `crates/goble-core/src/reasoning.rs:726` and `:925-928` build a tool-result row whose status is the literal text prefix `ERROR: `, and `app/src/state.rs::tool_terminal_data` (around lines 197-218) renders that text with no status read from a field; separately `app/src/ui/terminal.rs:562-566` (`build_agent_view`) computes `if block.failed { Error } else { Success }`, which paints a still-running command as Success. Carry the outcome as data - the block already knows its lifecycle and `ToolResult`/`ToolOutcome` already exist - and read it at both surfaces. A running block must not render as a finished one, and a failed one must not be recognised by string matching. Read `agent-parser.md` section 10 and `agent-tui.md` section 9.";

let g_r6 = "Make the two remaining cards card-free. A5's command proposal in `crates/goble-ui/src/elements/chat_composer.rs:620-629` is a `Container` with `with_background(SurfaceRaised)`, `Border::all(1.0)` and `corner_radius(8.0)`, and its test around `:1376` asserts that border; the app draws the conversation card itself in `app/src/ui/terminal.rs::build_card` (`:185-211`, a `Container` with `with_border(ColorToken::Border)` and a corner radius) while A8's card-free `crates/goble-ui/src/elements/conversation_card.rs` element is exported at `crates/goble-ui/src/elements.rs:32` and constructed nowhere. The rule for this product is explicit: everything the agent does is drawn as native rows - an accent rail, a one-character bullet, full-width bands - and nothing is wrapped in a generic bordered card. Rebuild the proposal and the card as row-based surfaces, construct the real `ConversationCard` element from the app instead of a private bordered container, and update the tests that currently assert the border so they assert its absence. Read `agent-tui.md` section 9.";

// ------------------------------------------------ wave 8 goals (transcript fidelity)

let g_a4 = "A tool call must fold exactly as grok-build's does. Today `build_tool_call_rows` (`crates/goble-ui/src/elements/chat_message_bubble.rs:102`) draws the whole body unconditionally, and the only collapse/expand state in the element is reasoning-only (`reasoning_expanded`, `:379-432`, Q11). Read the reference at `~/Projects/grok-build` (read-only, and copy nothing out of it): a per-entry `DisplayMode::{Collapsed, Truncated, Expanded}` (`crates/codegen/xai-grok-pager/src/scrollback/types.rs:54`) whose default is set per block when the entry is made (`scrollback/entry.rs:188`), a `toggle_fold` (`entry.rs:258`) driven by `e` to fold, `h`/`l` to collapse/expand, `E` for all entries and a double-click on the row (`app/agent_view/selection.rs:1091`); and, more importantly, the per-block defaults — a read collapses to a single header line and cycles Collapsed -> Truncated (first 5 + last 3 lines) -> Collapsed on `e`, while `l` expands to everything (`scrollback/blocks/tool/read.rs:17-18, 360, 443-448`); an edit collapses to its header carrying a `+N/-M` diffstat (`blocks/tool/edit.rs:996-1007`); an agent's command collapses to its header, and a command the user ran with `!` starts truncated at first 2 + last 3 and auto-expands when it finishes (`blocks/tool/execute.rs:518, 750-758`; `appearance/config.rs:693-694`). Implement the same three-state fold for our tool-call rows, with the state owned by the app next to `reasoning_expanded` so it survives a rebuild, the per-shape defaults above (a read starts folded, an edit starts folded with its diffstat, a command starts folded), and a toggle that works from the keyboard and from a click on the row. Keep our own key bindings where grok-build's would collide with the pane's; the defaults and the three states are the part that must match. Do not remove the Q6 inline-row shape, the status glyph or the R5 status read.";

let g_h1 = "Add syntax highlighting, decided and scoped: `syntect` with `default-features = false` plus its pure-Rust regex backend (no C dependency) and `two-face` for bat's grammar set, exactly the stack grok-build uses (`~/Projects/grok-build`, read-only: `crates/codegen/xai-grok-markdown/src/syntax.rs:31` `Syntect::new`, `:39` `two_face::syntax::extra_newlines()`, `:45` `find_syntax_by_extension`; `crates/codegen/xai-grok-pager-render/src/syntax.rs:39` keeps only the foreground and bold/italic/underline and DROPS syntect's backgrounds). Author **our own** `.tmTheme` in grok-build's visual idiom under a new asset directory (e.g. `crates/goble-ui/assets/themes/`) and load it from there: do NOT copy any theme or grammar file out of `~/Projects/grok-build` or `~/Projects/warp-new`, and do not add a dependency on either. The language is resolved from a file extension, from a markdown fence info string, or from the `bash` token a command carries (grok-build's `views/tasks_pane.rs:49` uses `\"bash\"`). Expose one entry point that turns `(text, language_or_extension)` into styled runs — reuse the existing span/run types the renderer already paints, do not invent a second text model — and wire it into the markdown fence path (`crates/goble-ui/src/elements/code.rs` currently draws one colour plus the language label) and into the tool-call bodies Q7 shapes. Keep `Code`'s mono family and its no-wrap contract; a failure to resolve a language must fall back to today's plain rendering rather than panicking or drawing nothing. Note in your report the exact crate versions you added and the size cost of the grammar set.";

let g_h2 = "A file read must look like an editor, not a text blob. `path_body` (`crates/goble-ui/src/elements/chat_message_bubble.rs:223-236`) draws the path and then the whole result string as one `Muted` row. Build the read presentation the way grok-build's `ReadToolCallBlock::render_content_lines` does (`~/Projects/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/read.rs:257`): a right-aligned line-number gutter (`format!(\"{:>w$}  \", base_line + i)` at `:284`, gutter width taken from the largest number), then the line's content through H1's highlighter (`:289`), drawn on its own background band (`with_panel_background(theme.bg_dark)` at `:307`). Folded, the row is the path alone (A4's default for this shape). Derive the base line number from what the call actually returned — a read may carry an offset or a `cat -n`-style gutter, so state in your report which source you used and what happens when it is absent; a read whose result is not line-addressable must degrade to today's plain rows, never to wrong line numbers. Depends on A4 and H1.";

let g_h3 = "The diff must read as added and removed. The band exists but is far too weak: `crates/goble-ui/src/elements/diff.rs:15` sets `TINT = 0.10` and `:461` paints `surface.mix(&color, TINT)` over each changed row, while each changed line's text is one flat `Success`/`Error` colour (`:354-358`). grok-build fills the content area with real insert/delete backgrounds taken from the theme (`~/Projects/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs:413-418` chooses by tag; the fill is painted in `scrollback/wrappers/entry_renderer.rs:857-866`; the values live in `crates/codegen/xai-grok-pager-render/src/theme/tokyonight.rs:204-209` as `diff_insert_bg`/`diff_delete_bg`) and highlights the code inside the band. Do the same with our own values: add insert/delete background tokens to our theme (dark-palette green and red, strong enough to read as a band at a glance — the current 10% tint is not), paint the changed rows' full width with them, and run each changed line through H1's highlighter so the code is coloured inside the band (the diff lines are code, so the language comes from the edited file's path where the caller knows it, and the highlighter must degrade to the flat colour when it does not). Keep the right-aligned gutters, the rail, the hunk headers and the gap rows exactly as they are — this item changes colour, not layout. Depends on H1.";

let g_h4 = "A command's colours must survive the transcript. `TerminalData::for_command` (`crates/goble-ui/src/elements/terminal_block.rs:151-163`) splits the tool result on newlines and makes every line a plain `TerminalLine::output`, so output that emitted ANSI colours arrives flat in the transcript — while the same command in the pane keeps its colours, because the real emulator draws it. Nothing in `goble-ui` parses ANSI (`grep -rn 'ansi|SGR|escape'` over the terminal-block and chat-content files finds nothing). Two halves, both required: (1) the command line itself is highlighted as shell through H1 (grok-build renders `$ ` then `highlight_bash_command(&command)`, `~/Projects/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/execute.rs:167-179`); (2) the output's own escape sequences are interpreted so the text keeps the colour, weight and emphasis the command emitted. Look at how grok-build does the second half — `render_terminal_lines(output, theme.primary())` in `crates/codegen/xai-grok-pager-render/src/render/terminal_output.rs:34`, a bounded VTE-style parser — and prefer our own vendored emulator in `crates/goble-terminal` if its parser accepts a plain byte string; otherwise write a bounded SGR parser (colour, bold, italic, underline; ignore everything else safely). Never let an unknown sequence drop text: the bytes that are not recognised must appear verbatim. Depends on H1.";

// ------------------------------------------------ wave 9 goals (second-pass fixes)

let g_r7 = "A link click must reach an opener. R3 made a link inside a paragraph a live hit target that fires `ChatAction::OpenUrl`, and then stopped: `grep -rn 'with_on_action\\|on_action' app/src/` returns nothing, so `ChatView::with_on_action` (`crates/goble-ui/src/views/chat_view.rs`, around `:670-681`) is still the end of the line, and the app has no external-URL opener anywhere (`app/src/state.rs`'s `detect_screen_link` only recognises `rdp://` and `goble://desktop` handoffs). Wire it: `app/src/ui/chat.rs` passes an action handler into the chat view (the composer already receives its own callbacks the same way), and the app opens the URL through a guard that accepts only `http` and `https` — anything else is refused, never handed to a shell. Keep the existing handoff behaviour intact (`rdp://`, `goble://desktop` keep doing what they do today). Add a test that proves the action reaches the opener and that a non-http scheme does not. Report exactly where the click terminates now.";

let g_r8 = "`executed_command_block` must read the block's status instead of asserting one. `app/src/ui/terminal.rs:90-106` ends with `Some(TerminalData::new(\"terminal\", lines).with_status(TerminalStatus::Success))`, hard-coded, so a pane command that is still running or that exited non-zero is drawn as a finished success; `block_status` (`:113`) reads the block's `state` and `exit_code` and R5 already uses it for the agent view. This path is the one the chat transcript reuses (`app/src/ui/chat.rs:38`), so the same command is drawn with two different statuses depending on which surface asks. Make it read the block. Add a test that a running block and a failed block each come out of this function with the right status.";

let g_r9 = "Four surfaces in the agent's own UI are still bordered cards, and the rule has no exception for them. `AskUserCard` (`crates/goble-ui/src/elements/ask_user.rs:297`) wraps the agent's pending question in `Container::new(...).with_border(Border::all(1.0)...).with_corner_radius(8.0)` and is drawn inline by `crates/goble-ui/src/views/chat_view.rs:690` — while the sibling proposal (A5) is a borderless full-width band, so two agent interruptions look like they came from two different products. The other three are in `crates/goble-ui/src/views/chat_view.rs`: the queued-prompt block (`:755`), the inline remote-desktop handoff (`:806`) and the screen-link handoff (`:851`). Rebuild each as rows in the pager idiom — an accent rail or a full-width band, no border, no corner radius — and flip every test that currently asserts the border so it asserts its absence. Do not change what each surface says or does, only how it is drawn. Read `agent-tui.md` section 10.";

let g_r10 = "Two loose ends from R1's implementation. (1) `resume_command_chat_turn` (`crates/goble-desktop-service/src/state.rs:2733`) issues the resume and only then subscribes to the bus, so a command that finishes quickly can emit its `chat:turn_finished` before anyone is listening — R1's verifier could not trigger it in five runs, but it is a race by inspection. Subscribe before issuing the resume, and say in your report which ordering you chose and why it cannot lose an event. (2) The acceptance clause 'a turn containing a command tool never leaves the session busy' is an assertion inside the approve/reject tests; give it a test of its own, that drives a turn through the proposal and asserts the pane's busy state is cleared on every exit path (approved, rejected, and approved-but-failing).";

// ------------------------------------------------ wave 10 goals (second-pass fixes)

let g_r11 = "The clear paths must clear the approval state too. `finish_turn` clears `rt.pending_command` and `rt.command_selection` (`app/src/state.rs:1523-1524`), but the other three paths that reset a pane's transcript state do not: `on_clear_transcript` (`app/src/actions.rs:633-644`), the store-less branch of `bind_active_pane_conversation` (`app/src/state.rs:1623-1625`) and `on_stop` (`app/src/actions.rs:645-654`, which only sets `busy = false`). `pane_chat_snapshot` reads those fields back and `app/src/ui/chat.rs:211` renders them, so a stale approval card and its selected candidate can outlive the transcript they belonged to. Fix all three paths, and make the existing clear test assert the approval state is gone too — the current test only asserts the message list is empty. Read `agent-tui.md` section 10.";

let g_r12 = "The model must be able to see that a tool call failed. R5 removed the `ERROR: ` prefix so the UI could read the status from the persisted record, but that prefix was also the model's only failure signal: `crates/goble-core/src/reasoning.rs:728` and `:924-925` now write the tool row as `<call_id>\\n<message>`, and `build_history` (`:966-969`) puts that remainder straight into the LLM-facing `Message.content`. The structured replacement never reaches it, because the assistant row's `tool_calls` deserializes into `LlmToolCall` (`crates/goble-core/src/llm.rs:8-12`), which carries `id`/`name`/`arguments` and no status. Carry the outcome into the LLM-facing history — the natural place is a status on the tool message the model reads (the `role = \"tool\"` row), so a failed call is legible to the model without putting a magic prefix back in front of the user. Keep the UI's status read (the persisted call record) exactly as R5 left it, and do not regress R5's test that the row's text carries no status marker. Read `agent-parser.md` section 11.";

let g_r13 = "The error notice is the last bordered card. `build_agent_error` (`app/src/ui/chat.rs:408-413`) wraps the agent's failure in `Container::with_background(Fill::Solid(Surface)).with_border(ColorToken::Error.into()).with_corner_radius(radius).with_padding(...)`, and it is painted inline in the agent's transcript through `build_agent_chat`'s `with_notice` (`app/src/ui/chat.rs:160` -> `crates/goble-ui/src/views/chat_view.rs:216`, drawn at `:694-695`). After R9 it is the only `with_border` left anywhere in the agent surface (`grep -rn 'with_border\\|Border::all'` over the element and view files finds only it and the sanctioned terminal block). Rebuild it in the pager idiom — a full-width band with an accent rail, or rows — with no border and no corner radius, and flip any test that asserts the border. Read `agent-tui.md` section 10.";

let g_r14 = "One command, one rendering. H4 taught `TerminalData::for_command` to highlight the command line as shell and to keep the output's ANSI colours; the pane (`app/src/ui/terminal.rs:150`) and the assistant row's tool call (`crates/goble-ui/src/elements/chat_message_bubble.rs:382`) both go through it, but the app's persisted tool-result row builds its block by hand with `TerminalData::new` + `TerminalLine::output` (`app/src/state.rs:278-301`), so the third path keeps the old flat rendering and the same command can appear highlighted and flat in one transcript. Route that path through `for_command` too (the first line of the row is the call id and the rest is the result body, so the command text is not in the row — state in your report exactly where the command text comes from and what you did when it is genuinely unavailable, rather than inventing one). Add a test that the persisted path and the tool-call path produce the same block for the same command and output. Read `agent-parser.md` section 11.";

let g_h5 = "The markdown boxes are boxes. A fenced code block and a blockquote are wrapped in a rounded `SurfaceRaised` container: `crates/goble-ui/src/elements/chat_message_bubble.rs:991-1007` (`ChatBlock::CodeBlock` and `ChatBlock::BlockQuote`) and the group view's copies at `crates/goble-ui/src/elements/group_chat_message.rs:234-243`, `:326-330`, `:404-413`. They carry no border, so R9's border sweep did not catch them, but the product rule is \"no bordered card or box\" and grok-build draws both as full-width bands: a fenced block sits on a panel band (the same band treatment the read excerpt and the terminal block use) and a blockquote is an indent plus a rail. Rebuild both as bands/rows with no rounded container, keep the fence's language label and the H1 highlighting, and flip any test that asserts the container. Read `agent-tui.md` section 10.";

// ------------------------------------------------ wave 11 goals (live chrome)

let g_c1 = "The chrome needs one place to ask: what work is in flight right now? Nothing can answer it today. `RootView::drain_events` (`app/src/root_view.rs:120-308`) polls `bus.take_events()` (`:123`) and matches a fixed set of names (`:131-291`) - `chats:updated`/`chat:updated`, `chat:turn_finished`, `chat:ask_user`, `chat:command_proposed`, `chat:tool`, `chat:reasoning`, `workflows:updated`, `agents:updated`, `vault:updated`, `executions:updated`, `screen:handoff` - and nothing else, so the `agent:*` names the desktop service already emits are dropped: `agent:log` (`crates/goble-desktop-service/src/state.rs:1553`), `agent:started` (`:1584`), `agent:finished` (`:1613`), `agent:state_update` (`:1623`), `agent:tool_result` (`:1705`); each carries inline JSON with `worker_id`, `trace_id` and one more field. The service also holds live state the app only ever reads into product pages: `executions: HashMap<trace_id, ExecutionInfo>` with `ExecutionInfo { id, agent_id, worker_id, status, trace, started_at, finished_at }` (`state.rs:259-267`, field `:378`, read by `list_executions` `:2034-2036`) and the per-chat `tool_in_flight` map (`:405`, `in_flight_tool_calls` `:990-994`). On the app side the only in-flight state is per pane: `PaneRuntime::in_flight_tools: HashMap<call_id, ToolCall>` (`app/src/state.rs:576`, fed by `chat:tool`), `PaneRuntime::busy` (`:566`), `pending_command` (`:561`), `pending_ask` (`:558`), and the global mirror `UiState::agent_busy` (`:741`, set at `:1570`); `pane_in_flight_tool_count` (`:1549-1554`) is documented 'test observability; not read by the app' and has no production caller. Close the gap: (1) handle the five `agent:*` names in `drain_events` so a worker agent's start/finish/state/tool-result/log updates app state (the `executions:updated` arm already refreshes the execution pages - extend it only if the running set is not already available after it); (2) add one accessor that answers what is running, with enough detail for the chrome to draw it: the running executions (agent id, worker id, started at, status), the active pane's in-flight tool calls and whether its turn is busy, and whether an approval or a question is pending; (3) if the chrome needs an elapsed time it has no start time for, record the turn's start in the same state, and if a real start time genuinely cannot be observed, say so instead of inventing one. Choose the home deliberately - extend `UiState` in `app/src/state.rs` or add a small struct beside it - and make `pane_in_flight_tool_count` either the real accessor the chrome reads (drop the 'test observability' note) or delete it in favour of the new one; say which. This item renders nothing. It also delivers the `07 - Observability` tracker row 'Drain `executions:updated`/`agent:*` events into app state', so mark that row too and name this item as what delivered it. Read `agent-tui.md` section 11.";

let g_c2 = "Build the live turn-status footer. There is no footer anywhere: no `footer.rs`, no `status_bar`/`status_line`/`status_row` element, and nothing in `app/src` reserves a row for one - the chat column's last two children are the divider and the composer (`crates/goble-ui/src/views/chat_view.rs:1058-1059`). The one element that was meant to be it is dead and draws nothing: `RunningIndicator` (`crates/goble-ui/src/elements/running_indicator.rs`) is an 8x8 box whose `paint` only records its origin (`:47-49`), and it is constructed only in its own test (`:68`). Meanwhile the composer already owns the one control that belongs here: `stop_visible` (`crates/goble-ui/src/elements/chat_composer.rs:44`, builder `:237-240`) draws a red stop button (`:925-941`) and the app feeds it `session.agent_busy` (`app/src/ui/chat.rs:193`) with the cancel callback (`:206`). Add a one-row status band in the pager idiom - full width, no border, no rounded container, an accent rail or a bullet, exactly the treatment the transcript rows use - and place it in the chat column directly above the composer's divider, wired the way the composer's own flags are wired: a `ChatView` builder field (compare `with_composer_stop_visible`, `chat_view.rs:62,350-352,956`) fed by the app (`app/src/ui/chat.rs`). It has three states and no others. (a) The pane's turn is busy: the activity for what it is actually doing, derived from C1's live state - a running tool call drawn as its command/name, `Waiting on approval...` while `pending_command` is set, `Waiting on a question...` while `pending_ask` is set, else `Thinking...`/`Responding...` - plus the elapsed time since the turn started, and on the right the in-flight work count. Advance a spinner from whatever the element layer really has (a paint time or frame counter in the renderer); if there is no clock, say so and drive it from the frame rather than faking a time. Include a token count only if a real source exists - name it; otherwise omit it and say so. Do not add a second stop control: the composer's stays the only one. (b) The pane is idle with work still in flight: `{n} {kind} still running` naming each kind that is really running (a command, an execution, later a sub-agent), as one hit target that fires an app action opening the work overlay (add the action/flag here even though the overlay itself is a later item, and say plainly that the action has no visible surface until then). (c) Nothing is in flight: the row takes zero height, so it costs no space. Revive `RunningIndicator` for the spinner (make its `paint` actually draw) or delete it in favour of the new element and say which. `pane_in_flight_tool_count` stops being test-only here: the app reads the real count. Read `agent-tui.md` section 11.";

let g_c3 = "Give the topbar a live indicator. The shipping topbar is the app's own `build_topbar` (`app/src/ui/shell.rs:56-123`), built at `app/src/ui/mod.rs:1414` and painted in the root stack (`:1527-1539`); its root row is `SpaceBetween` with exactly two children, `left` (sidebar toggle, workspace chips, the `+ v` environment control at `:82-88`) and the settings icon (`:90-98`), so there is no slot for anything else - while `goble-ui`'s own `Topbar` element (`crates/goble-ui/src/elements/topbar.rs:39-171`, right group hard-coded to two icon buttons at `:132-137`) is used only by `ShellView` and never by the app. Add a live indicator between the workspace strip and the settings button: when work is in flight, a spinner plus `* N` with N from C1's live state, and the same count available to a click that fires the app's work-overlay action (the one C2 adds; say plainly that it has no visible surface until the overlay item lands); when nothing is in flight, the indicator is absent or inert so the topbar keeps its current look. Show a context/token indicator next to it only if a real source exists - name the source; do not invent a number. Do not change `TOPBAR_HEIGHT` (the 36px/40px rule stands), do not disturb the workspace chips or the `+ v` control, and do not switch the app onto `goble-ui`'s `Topbar`. Read `agent-tui.md` section 11.";

// ------------------------------------------------ wave 12 goals (sub-agents)

let g_s1 = "Add the sub-agent model. Nothing in the tree models a spawned child agent today: `crates/goblin-worker/src/agent_runtime.rs` is a one-line re-export (`pub use goble_core::agent_runtime::{ChecklistItem, RuntimeState};`) and its sibling `agent_runtime/` directory is not compiled (`goblin-worker/src/lib.rs` declares only `pub mod agent_runtime;`), so `.agents/04-agent-runtime/subagents.md`'s 'Reuse' claim about it is not true in code - do not build on it, and say so in your report. Create the model the doc specifies as `crates/goble-core/src/subagent.rs`, exported from `lib.rs`: a `SubAgentId`; a `SubAgentSpec { id, parent_chat_id, parent_call_id, description, subagent_type, prompt, cwd, run_in_background, depth, budget }`; a `SubAgentBudget { max_turns, max_tool_calls, max_tokens }` whose charge API makes exhaustion a terminal outcome (a typed exhausted reason that ends the child), never a loop or a hang; a `SubAgentStatus { Initializing, Running { turns, tool_calls, tokens, activity }, Completed { output, duration_ms, turns, tool_calls, tokens }, Failed { error }, Cancelled { reason } }` with `is_running()`/`is_terminal()` and the one-line activity label the renderer will print; and a `SubAgentRecord { spec, status, started_at, finished_at }` with `elapsed()`/`duration_ms()`. Add the two bounds the doc names: a depth constant and a `can_spawn_subagent(depth)` predicate (the spawn tool is what goes away at the limit, not a counter), and the per-child CWD rule - the child's working directory is a subdirectory of the workspace root derived from its id, so two children never share one. Keep this module pure: no I/O, no async, no store. `cargo test -p goble-core`: each of the three budgets is terminal at its limit, the depth predicate refuses at the limit and allows below it, a record's elapsed/duration come from its own timestamps, and a terminal status is distinguishable from a running one for every variant. Read `.agents/04-agent-runtime/subagents.md` first - its Model section is the specification.";

let g_s2 = "Make a sub-agent a real child run. Add the `spawn_subagent` tool and a bounded child turn loop. The registry is declared in `harness_tool_definitions()` (`crates/goble-core/src/harness.rs:684-1175`, each entry `ToolDefinition { name, description, parameters }` from `llm.rs:30-35`), dispatched by `execute_tool_call` (`:1230-1292`, match on `call.name` with an `anyhow::Result<String>` error everywhere), and shaped by `tool_presentation` (`:1202-1212`) - the `SubAgent` variant already exists and `create_agent`/`run_agent` already map to it (`:1209`), while the `run_agent` handler itself runs nothing (`:2236-2244`, it formats a prompt and returns a string). Do three things. (1) Declare `spawn_subagent` with a schema carrying the description, the prompt, `subagent_type`, `run_in_background` and an optional `cwd`, and map it to `ToolPresentation::SubAgent`. (2) Give a child its own conversation: `chats` has no parent column and its migration idiom is the probe-then-`ALTER TABLE` block at `crates/goble-core/src/store.rs:286-300` (the one that added `workspace_routing`), so add `parent_chat_id` that way; the child's chat id is its `SubAgentId`, and its messages are persisted under that id through `insert_chat_message` (`store.rs:919-936`) so the parent can later render the child's own transcript (a later item) - do not write the child's rows into the parent's conversation. (3) Run the child: a bounded loop in a new module (e.g. `crates/goble-core/src/subagent_run.rs`) that calls the same model provider `run_mission_turn` uses (read `run_mission_turn`'s signature, `crates/goble-core/src/reasoning.rs:321`, and thread the provider through rather than constructing a second client), builds its history with `Message` (`llm.rs:15-21`), dispatches each child tool call through `execute_tool_call`, persists the child's assistant/tool rows under its own chat id, enforces S1's budget (turn/step and token) by ending the child with a terminal status the moment a budget is exhausted, and withholds `spawn_subagent` from the child's tool list when `can_spawn_subagent(depth)` is false. The child's cwd is S1's per-child subdirectory under the harness's `workspace_dir` (`harness.rs:488`, `with_workspace_dir` `:548-551`). For this item a spawn is foreground only: the handler awaits the child and returns its final output as the tool result; background semantics, cancellation and the record registry are the next item. Do NOT touch the wire in this item. `cargo test -p goble-core` with a stub `LlmProvider` (no network): a spawned child gets its own `chats` row carrying the parent id and writes its messages under it, its final text comes back as the tool result, a budget exhausted mid-run ends it terminal with the reason rather than looping, and a spawn at the depth limit is refused. Read `.agents/04-agent-runtime/subagents.md`.";

let g_s3 = "Make a sub-agent concurrent, cancellable and observable to the harness. S2 runs a spawn foreground only; this item adds the other half. `run_in_background: true` must return to the model immediately with the child's id and a `Running` record while the child keeps going on a `tokio::spawn` task. The `Harness` owns a registry of child records (a map keyed by `SubAgentId`, S1's `SubAgentRecord`) with accessors to list the records and to snapshot one, so a later renderer and a later wire event have a real source; completion, failure, cancellation and budget exhaustion all move the record to its terminal status (a child that panics is `Failed`, never a hang). Cancellation is real: the parent's existing `cancel: Arc<AtomicBool>` (`crates/goble-core/src/harness.rs:584`, `cancel()` at `:584`/`Harness::cancel`) must reach every child, and a single child must be cancellable on its own through a new entry point - both stop the child's loop promptly and mark the record `Cancelled` with the reason. Do not break the foreground path S2 built. `.agents/04-agent-runtime/subagents.md` is the model; the reference for the execution semantics is grok-build's coordinator (read-only, copy nothing): a foreground await with a deadline that auto-backgrounds, and `will_wake` for a background child that may wake the parent - adopt the first if it falls out cheaply and state plainly what you did with the second. `cargo test -p goble-core`: a background spawn returns before the child finishes and its record later reaches `Completed` with the output, cancelling one child leaves another running, a budget-exhausted child is terminal, and a panicking child becomes `Failed`.";

let g_s4 = "Put sub-agents on the wire. The app cannot see a child until the three lifecycle events cross the daemon boundary, and there is an exact template to follow: `CommandProposed` is the nearest existing event and it is wired through six places - `HarnessEvent` (`crates/goble-core/src/harness.rs:342-388`), `map_event` (`crates/goble-harness-internal/src/lib.rs:239-334`; `CommandProposed` arm `:285-293`), `HarnessServerEvent` (`crates/goble-harness-protocol/src/lib.rs:44-136`; `:81-87`), the daemon's `HarnessServerEvent -> DaemonEvent` mapper (`crates/goble-daemon/src/state.rs:603-700`; arm `:647-654`), `DaemonEvent` (`crates/goble-daemon-protocol/src/lib.rs:101-193`; `:132-137`), and `translate_daemon_event` in the desktop service (`crates/goble-desktop-service/src/state.rs:774-933`; `CommandProposed` arm `:791-806`) - and the protocol crate must not depend on `goble-core`, so the event payloads are plain data. Add three variants, mirroring exactly the fields S1's record actually has (do not invent any): `SubAgentSpawned { chat_id, subagent_id, subagent_type, description, parent_call_id, run_in_background }`, `SubAgentProgress { chat_id, subagent_id, status, activity, turns, tool_calls, tokens, duration_ms }`, `SubAgentFinished { chat_id, subagent_id, status, output or error, duration_ms, turns, tool_calls, tokens }`, emitted by S3's registry transitions, and translate them in the desktop service into `chat:subagent_spawned`, `chat:subagent_progress`, `chat:subagent_finished` with a typed payload struct beside `ToolCallEvent`/`CommandProposedEvent` (`state.rs:111-166`). The daemon's mapper already has a non-exhaustive `_ => {}` (`:933`), so a daemon-only variant is possible, but carry all six hops for every event so the chain is complete. `cargo test -p goble-harness-internal -p goble-daemon -p goble-desktop-service`: one adapter test per variant in the style of the existing `CommandProposed` mapping test, and a desktop-service test that a scripted spawned/progress/finished sequence emits the three named events with the fields intact.";

let g_s5 = "Draw a sub-agent in the parent transcript. Today a sub-agent renders as `sub_agent_body` (`crates/goble-ui/src/elements/chat_message_bubble.rs:748-767`) - the agent id row, the input row, and the result - under the `ToolPresentation::SubAgent` presentation, with no status, no elapsed time and no child of any kind; the presentation and the body both hang off the tool name (`tool_call_summary` `:189-250`, `build_tool_call_body` `:341-361`). Build the row grok-build draws (read-only reference: `scrollback/blocks/subagent.rs` - `Subagent running: \"desc\" . {activity}`, `Subagent completed in {Xs}: \"desc\"`, `failed in {Xs}{err}:`, `cancelled in {Xs}:`), in our pager idiom: one ruler row with a status bullet, the description and, while running, the activity label and the elapsed time from the record - no card, no border, no rounded container. It must fold exactly like every other tool call (A4's three states, one line folded, the child's detail expanded) so a sub-agent is a quiet row first; the child's own transcript rows appear in the expanded body only if S2's persisted child conversation is readable from the element's caller - if it is not, show only what the record carries and say so. The row's data must come from the live sub-agent records (S4's events held by the app), not from the tool's arguments alone, and a click or Enter on the row fires the action that opens the child view (the next item; add the action here and say it has no surface until then). Keep `ToolPresentation::SubAgent` as the resolution key and do not regress the existing presentation test. `cargo test -p goble-ui -p goble-app`: each of the four statuses draws its own line, a running row carries the activity and the elapsed time, the fold behaves like the other shapes, and the click fires the open action.";

let g_s6 = "Give the child its own view. A sub-agent must be enterable, not just visible. S2 persisted each child's messages under its own chat id (with `parent_chat_id` on `chats`), so this is a view over a real conversation, and A7 already built the mechanism to use: `BlockList::push_agent_view_block` puts a conversation's card into the pane's block list and points the pane's filter at that conversation's agent view, Esc returns the filter to the terminal leaving the card behind, and a click on the card reopens it - all reachable from the app (`app/src/ui/terminal.rs`, `app/src/state.rs`, `app/src/actions.rs`; the A7 tracker row in `.agents/TRACKER.md` names the round-trip test). Entering from S5's row must do for the child's conversation what A7 does for the parent's: push the child's card and filter the pane to it, with a framed title carrying the child's type, description, status and elapsed time, and Esc returning to the parent transcript. The child's rows are read from the store by its chat id and rendered through the one terminal-block/message renderer, not a bespoke path. If entering a child conflicts with A7's one-card-at-a-time filter (a child entered from inside the parent's own agent view), resolve it deliberately - say which conversation the pane switches to and how the parent is reached again - rather than leaving an unreachable state. `cargo test -p goble-app`: entering from the row shows the child's conversation and its persisted messages, Esc returns to the parent, and the round trip can be made twice. NOTE, start here: an interrupted repair left the working tree red. `root_view::tests::a_nested_child_row_inside_the_child_view_is_enterable_and_esc_returns_to_the_parent` fails - a sub-agent row drawn inside a child's own view is drawn from the tool arguments with no live record behind it, so it offers no way in, while the S6 record in `.agents/TRACKER.md` claims nested children stay enterable. Decide the case on the evidence: either make it work (the child view feeds its own pane's sub-agent rows and relays the open action for that pane) or state in your report and in the tracker that the design does not require it and remove the test. Do not leave a failing test in the tree, and do not weaken a test that encodes a claim the tracker still makes.";

let g_s7 = "Build the work overlay. The chrome has two cues pointing at nothing: C2's `{n} still running` line and C3's topbar indicator both fire an app action that opens it, and S4 gives it real data to show. Build the overlay with the app's established overlay pattern (`Sheet`/`Dialog` constructed in `app/src/ui/mod.rs:1479-1514` and `:1520-1524`, closed by Esc or a backdrop click) - a card-free panel of rows and bands, per the product rule. It lists the work actually in flight, grouped by kind, with a per-sub-agent row carrying a status bullet, the type and description, the activity label and the elapsed time from S4's records, plus the turns/tool-calls/tokens the record really has (omit a field the record does not carry and say so). Each sub-agent row offers a kill that reaches S3's per-child cancel (through the app's existing cancel path or a new desktop-service entry point - say which), and a finished/failed/cancelled row shows its terminal line rather than disappearing. It opens from C2's and C3's action and from our own keyboard binding, closes on Esc and on a backdrop click, and auto-opens when the running sub-agent count goes from zero to one, unless the user has dismissed it since the last time it was empty (state the rule you implement). Nothing in it may be a bordered box. `cargo test -p goble-ui -p goble-app`: the sheet opens and closes by each route, lists the records it is given grouped by kind, a kill fires the cancel path, and a dismissed overlay does not re-open for the same run.";

// ---------------------------------------------------------------- waves

let waves = [
    #{ n: 1, title: "Foundations", phase_title: "Wave 1: Foundations", items: [
        item("B0", "B0", "Shell integration for bash and zsh", "terminal-blocks.md",
            "crates/goble-terminal/src/hooks.rs, crates/goble-terminal/assets/shell/, app/src/terminal.rs",
            [], g_b0,
            "cargo test -p goble-terminal, plus `bash -n` and `zsh -n` on every script, plus a test that feeds the exact bytes a script emits through the hook decoder and asserts each hook variant parses with its exit code"),
        item("Q1", "Q1", "Transcript scrolling and follow the stream", "agent-parser.md",
            "crates/goble-ui/src/views/chat_view.rs, crates/goble-ui/src/elements/scrollable.rs",
            [], g_q1,
            "cargo test -p goble-ui and cargo test -p goble-app"),
        item("Q10", "Q10", "Parse once per change, off the layout path", "agent-parser.md",
            "app/src/state.rs",
            [], g_q10,
            "cargo test -p goble-app"),
    ]},
    #{ n: 2, title: "Text", phase_title: "Wave 2: Text", items: [
        item("Q2", "Q2", "Markdown fidelity", "agent-parser.md",
            "crates/goble-ui/src/elements/markdown.rs, crates/goble-ui/src/elements/chat_content.rs, crates/goble-ui/src/elements/chat_message_bubble.rs",
            [], g_q2,
            "cargo test -p goble-ui, with a parser test per construct including the ones that have no test today"),
        item("Q3", "Q3", "Italic renders", "agent-parser.md",
            "crates/goble-ui/src/elements/inline_text.rs, crates/goble-ui/src/elements/chat_content.rs",
            [], g_q3,
            "cargo test -p goble-ui"),
        item("Q4", "Q4", "Code blocks are monospaced", "agent-parser.md",
            "crates/goble-ui/src/elements/chat_message_bubble.rs, crates/goble-ui/src/elements/code.rs",
            [], g_q4,
            "cargo test -p goble-ui"),
        item("Q9", "Q9", "Diff rendering element", "agent-parser.md",
            "crates/goble-ui/src/elements/ (new element)",
            [], g_q9,
            "cargo test -p goble-ui covering hunks, context gaps and line counts"),
    ]},
    #{ n: 3, title: "Carriers", phase_title: "Wave 3: Carriers", items: [
        item("A2/Q5", "A2 + Q5", "Tool-call status carrier, persisted", "agent-tui.md",
            "crates/goble-core/src/reasoning.rs, crates/goble-ui/src/elements/chat_content.rs, persistence",
            [], g_a2q5,
            "cargo test -p goble-core and cargo test -p goble-app, including a started call being readable from the store before the turn ends"),
        item("A3", "A3", "Live tool events to the app", "agent-tui.md",
            "crates/goble-desktop-service/src/state.rs, app/src/root_view.rs",
            [], g_a3,
            "cargo test -p goble-desktop-service and cargo test -p goble-app, with a test that the in-flight map gains and clears an entry per call"),
        item("Q11", "Q11", "Reasoning reaches the app and renders", "agent-parser.md",
            "crates/goble-harness-internal/src/lib.rs, crates/goble-daemon-protocol/, crates/goble-ui/",
            [], g_q11,
            "cargo test -p goble-harness-internal and cargo test -p goble-app"),
    ]},
    #{ n: 4, title: "Bridge", phase_title: "Wave 4: Bridge", items: [
        item("P1", "P1", "Block ownership", "agent-terminal-bridge.md",
            "crates/goble-terminal/src/blocks.rs",
            [], g_p1,
            "cargo test -p goble-terminal: a claimed Preexec takes the owner, an unclaimed one is the user's, a claim aimed at the wrong block is refused"),
        item("P2", "P2", "The claim protocol in the pane", "agent-terminal-bridge.md",
            "app/src/terminal.rs",
            ["P1"], g_p2,
            "cargo test -p goble-app with scripted hook sequences: claim then resolve, claim then timeout, claim then a user command arrives first"),
        item("P3", "P3", "Tool-result extraction", "agent-terminal-bridge.md",
            "app/src/terminal.rs, crates/goble-terminal/src/blocks.rs",
            ["P1"], g_p3,
            "cargo test -p goble-terminal: exit 0 with output, non-zero exit, empty output, interrupted (130), background promotion"),
        item("P4", "P4", "Route the agent's shell tool through the pane", "agent-terminal-bridge.md",
            "crates/goble-core/src/harness.rs, app/src/actions.rs",
            ["P2", "P3"], g_p4,
            "cargo test -p goble-core: the tool returns the pane's output, and the sandbox path is still used when there is no session"),
        item("P6", "P6", "Owner into the views", "agent-terminal-bridge.md",
            "app/src/state.rs, app/src/ui/terminal.rs",
            ["P1"], g_p6,
            "cargo test -p goble-app: the two views show the right blocks after a mixed sequence"),
    ]},
    #{ n: 5, title: "Surface", phase_title: "Wave 5: Surface", items: [
        item("Q6", "Q6", "Inline tool-call rows replace the card", "agent-parser.md",
            "crates/goble-ui/src/elements/chat_message_bubble.rs, app/src/state.rs",
            ["A2/Q5"], g_q6,
            "cargo test -p goble-ui: no border container, and a collapsed call renders in one row"),
        item("Q7", "Q7", "Per-tool presentation", "agent-parser.md",
            "crates/goble-ui/src/elements/chat_content.rs, crates/goble-core/src/harness.rs",
            ["Q6"], g_q7,
            "cargo test -p goble-ui, one test per tool shape"),
        item("Q8", "Q8", "The terminal segment drawn as a block", "agent-parser.md",
            "app/src/ui/terminal.rs, crates/goble-ui/src/elements/",
            ["P3", "A2/Q5"], g_q8,
            "cargo test -p goble-ui and cargo test -p goble-app: the same block renders identically in the transcript and in the terminal pane"),
        item("A6", "A6", "Harness approval suspension", "agent-tui.md",
            "crates/goble-core/src/harness.rs, crates/goble-core/src/reasoning.rs",
            [], g_a6,
            "cargo test -p goble-core: suspend, approve-runs-chosen, edit-runs-edited, reject-errors"),
        item("A5", "A5", "Composer proposal mode", "agent-tui.md",
            "crates/goble-ui/src/elements/chat_composer.rs",
            ["A6"], g_a5,
            "cargo test -p goble-ui: selection, edit, submit payload"),
        item("A7", "A7", "The agent view over the block list", "agent-tui.md",
            "app/src/ui/terminal.rs, app/src/state.rs",
            [], g_a7,
            "cargo test -p goble-app: enter/leave/click round-trip"),
        item("A8", "A8", "The conversation card element", "agent-tui.md",
            "crates/goble-ui/src/elements/ (new element)",
            [], g_a8,
            "cargo test -p goble-ui"),
    ]},
    #{ n: 7, title: "Integration fixes", phase_title: "Wave 7: Integration fixes", items: [
        item("R1", "R1", "The approval gate must run commands", "agent-tui.md",
            "crates/goble-core/src/reasoning.rs, crates/goble-harness-internal/src/lib.rs, crates/goble-daemon-protocol/, crates/goble-desktop-service/src/state.rs, app/src/, crates/goble-ui/src/elements/chat_composer.rs",
            [], g_r1,
            "cargo test -p goble-core -p goble-harness-internal -p goble-desktop-service -p goble-app, including a test that a suspended command, once approved, runs and the turn finishes; that a rejected command produces a ToolCallError and finishes; and that a turn containing a command tool never leaves the session busy"),
        item("R2", "R2", "Pane routing falls back to the sandbox", "agent-terminal-bridge.md",
            "app/src/terminal.rs",
            [], g_r2,
            "cargo test -p goble-app: a pane whose shell was not bootstrapped offers no pane session (the sandbox path is kept), and a claim that can never resolve fails immediately rather than after the full timeout"),
        item("R3", "R3", "Links stay clickable inside a paragraph", "agent-parser.md",
            "crates/goble-ui/src/elements/chat_content.rs, crates/goble-ui/src/elements/chat_message_bubble.rs, crates/goble-ui/src/elements/inline_text.rs, crates/goble-ui/src/views/chat_view.rs",
            [], g_r3,
            "cargo test -p goble-ui (and -p goble-app if the action path is wired there): a link inside a paragraph is one live action target carrying its URL"),
        item("R4", "R4", "One renderer for a tool-result terminal segment", "agent-parser.md",
            "crates/goble-ui/src/elements/group_chat_message.rs, crates/goble-ui/src/elements/terminal_block.rs",
            [], g_r4,
            "cargo test -p goble-ui and cargo test -p goble-app: a Terminal fragment draws through the shared terminal_block renderer instead of Empty"),
        item("R5", "R5", "Read the status, do not encode it as a string", "agent-parser.md",
            "crates/goble-core/src/reasoning.rs, app/src/state.rs, app/src/ui/terminal.rs",
            [], g_r5,
            "cargo test -p goble-core -p goble-ui -p goble-app: a still-running block is not drawn as Success, and no status is recovered from an ERROR: string prefix"),
        item("R6", "R6", "The card-free cards", "agent-tui.md",
            "crates/goble-ui/src/elements/chat_composer.rs, crates/goble-ui/src/elements/conversation_card.rs, app/src/ui/terminal.rs",
            [], g_r6,
            "cargo test -p goble-ui and cargo test -p goble-app: neither the proposal nor the conversation card contains a border, and the app constructs the ConversationCard element"),
    ]},
    #{ n: 8, title: "Transcript fidelity", phase_title: "Wave 8: Transcript fidelity", items: [
        item("A4", "A4", "The tool-call fold, as grok-build has it", "agent-tui.md",
            "crates/goble-ui/src/elements/chat_message_bubble.rs, crates/goble-ui/src/views/chat_view.rs, app/src/state.rs",
            [], g_a4,
            "cargo test -p goble-ui -p goble-app: a tool call starts folded for each shape, cycles Collapsed -> Truncated -> Expanded and back, `e` folds it, the state survives a rebuild, and a folded read draws one line while a folded command hides its output"),
        item("H1", "H1", "Syntax highlighting infrastructure", "agent-parser.md",
            "Cargo.toml, crates/goble-ui/Cargo.toml, crates/goble-ui/assets/themes/ (new), crates/goble-ui/src/elements/code.rs, crates/goble-ui/src/elements/chat_message_bubble.rs",
            [], g_h1,
            "cargo test -p goble-ui: a Rust snippet and a shell command each come out of the entry point as more than one colour, an unknown language falls back to the plain rendering, and no file was copied from the reference trees"),
        item("H2", "H2", "A file read is an IDE excerpt", "agent-parser.md",
            "crates/goble-ui/src/elements/chat_message_bubble.rs, crates/goble-ui/src/elements/code.rs",
            ["A4", "H1"], g_h2,
            "cargo test -p goble-ui: an expanded read draws right-aligned line numbers and highlighted lines, a folded one draws the path only, and a read with no usable line information degrades to plain rows"),
        item("H3", "H3", "The diff bands, and the code inside them", "agent-parser.md",
            "crates/goble-ui/src/elements/diff.rs, crates/goble-ui/src/theme.rs",
            ["H1"], g_h3,
            "cargo test -p goble-ui: a changed row paints a full-width insert or delete background from the theme, the code inside it is highlighted, and the layout (gutters, rail, hunk headers, gap rows) is unchanged"),
        item("H4", "H4", "The command line and its output keep their colours", "agent-parser.md",
            "crates/goble-ui/src/elements/terminal_block.rs, crates/goble-ui/src/elements/chat_message_bubble.rs",
            ["H1"], g_h4,
            "cargo test -p goble-ui: the command line draws as more than one colour, an ANSI-coloured output blob keeps its colours, and an unrecognised escape sequence loses no text"),
    ]},
    #{ n: 9, title: "Second-pass fixes", phase_title: "Wave 9: Second-pass fixes", items: [
        item("R7", "R7", "The link's click reaches an opener", "agent-parser.md",
            "app/src/ui/chat.rs, crates/goble-ui/src/views/chat_view.rs, app/src/state.rs",
            [], g_r7,
            "cargo test -p goble-app (and -p goble-ui if the relay changes): a link click reaches an app-side opener carrying its URL, and a non-http(s) scheme is refused"),
        item("R8", "R8", "The pane block reads its status", "agent-parser.md",
            "app/src/ui/terminal.rs",
            [], g_r8,
            "cargo test -p goble-app: a running block and a failed block each come out of executed_command_block with the right status"),
        item("R9", "R9", "The four surfaces that are still cards", "agent-tui.md",
            "crates/goble-ui/src/elements/ask_user.rs, crates/goble-ui/src/views/chat_view.rs",
            [], g_r9,
            "cargo test -p goble-ui and cargo test -p goble-app: the ask-user surface, the queued prompt and both handoffs paint no border, and the flip is asserted, not just claimed"),
        item("R10", "R10", "The two R1 loose ends", "agent-tui.md",
            "crates/goble-desktop-service/src/state.rs",
            [], g_r10,
            "cargo test -p goble-desktop-service: the resume listener is subscribed before the resume is issued, and a named test proves the pane is freed on every exit path"),
    ]},
    #{ n: 10, title: "Open defects", phase_title: "Wave 10: Open defects", items: [
        item("R11", "R11", "The clear paths clear the approval state", "agent-tui.md",
            "app/src/actions.rs, app/src/state.rs",
            [], g_r11,
            "cargo test -p goble-app: every clear path drops the pending approval and its selected candidate, asserted by a test that fails if either survives"),
        item("R12", "R12", "A failed tool call is legible to the model", "agent-parser.md",
            "crates/goble-core/src/reasoning.rs, crates/goble-core/src/llm.rs, crates/goble-core/src/store.rs",
            [], g_r12,
            "cargo test -p goble-core: the LLM-facing history shows a failed tool call as failed, and R5's test that the persisted row's text carries no status marker still passes"),
        item("R13", "R13", "The error notice is not a bordered card", "agent-tui.md",
            "app/src/ui/chat.rs, crates/goble-ui/src/views/chat_view.rs, crates/goble-ui/src/elements/",
            [], g_r13,
            "cargo test -p goble-ui -p goble-app: the error notice paints no border and no corner radius, and the absence is asserted"),
        item("R14", "R14", "One command, one rendering, on every path", "agent-parser.md",
            "app/src/state.rs, crates/goble-ui/src/elements/terminal_block.rs",
            [], g_r14,
            "cargo test -p goble-app: the persisted tool-result path and the tool-call path build the same terminal block for the same command and output"),
        item("H5", "H5", "The markdown boxes are boxes", "agent-tui.md",
            "crates/goble-ui/src/elements/chat_message_bubble.rs, crates/goble-ui/src/elements/group_chat_message.rs",
            [], g_h5,
            "cargo test -p goble-ui: a fenced block and a blockquote draw as bands with no rounded container, and the fence label and H1 highlighting survive"),
    ]},
    #{ n: 11, title: "Live chrome", phase_title: "Wave 11: Live chrome", items: [
        item("C1", "C1 + the 07 drain item", "Drain live agent activity into app state", "agent-tui.md",
            "app/src/root_view.rs, app/src/state.rs, crates/goble-desktop-service/src/state.rs",
            [], g_c1,
            "cargo test -p goble-app: a scripted sequence of chat:tool, chat:turn_finished, agent:started, agent:finished, agent:state_update, agent:tool_result and executions:updated leaves the live accessor reporting exactly the work still in flight"),
        item("C2", "C2", "The live turn-status footer", "agent-tui.md",
            "crates/goble-ui/src/elements/ (new status element), crates/goble-ui/src/elements/running_indicator.rs, crates/goble-ui/src/views/chat_view.rs, app/src/ui/chat.rs",
            ["C1"], g_c2,
            "cargo test -p goble-ui -p goble-app: the footer draws the activity and the elapsed time while busy, the still-running line naming each kind while idle, takes zero height when nothing is in flight, and its cue fires the work-overlay action"),
        item("C3", "C3", "The topbar live indicator", "agent-tui.md",
            "app/src/ui/shell.rs, app/src/ui/mod.rs",
            ["C1"], g_c3,
            "cargo test -p goble-app: the indicator shows the live count when work is in flight, is inert at zero, its click fires the work-overlay action, and TOPBAR_HEIGHT and the workspace controls are unchanged"),
    ]},
    #{ n: 12, title: "Sub-agents", phase_title: "Wave 12: Sub-agents", items: [
        item("S1", "S1", "The sub-agent model, budget and lifecycle", "subagents.md",
            "crates/goble-core/src/subagent.rs (new), crates/goble-core/src/lib.rs",
            [], g_s1,
            "cargo test -p goble-core: each budget is terminal at its limit, the depth predicate refuses at the limit and allows below it, elapsed/duration come from the record, and every status distinguishes running from terminal"),
        item("S2", "S2", "A sub-agent is a real child run", "subagents.md",
            "crates/goble-core/src/harness.rs, crates/goble-core/src/subagent_run.rs (new), crates/goble-core/src/store.rs, crates/goble-core/src/reasoning.rs",
            ["S1"], g_s2,
            "cargo test -p goble-core with a stub provider: a child gets its own chats row carrying the parent id and writes its messages under it, its output returns as the tool result, an exhausted budget ends it terminal with the reason, and a spawn at the depth limit is refused"),
        item("S3", "S3", "Background, cancellation and the record registry", "subagents.md",
            "crates/goble-core/src/subagent_run.rs, crates/goble-core/src/harness.rs, crates/goble-core/src/subagent.rs",
            ["S2"], g_s3,
            "cargo test -p goble-core: a background spawn returns before the child ends and its record later reaches Completed with the output, cancelling one child leaves another running, a budget-exhausted child is terminal, and a panicking child becomes Failed"),
        item("S4", "S4", "Sub-agent lifecycle on the wire", "subagents.md",
            "crates/goble-core/src/harness.rs, crates/goble-harness-internal/src/lib.rs, crates/goble-harness-protocol/src/lib.rs, crates/goble-daemon/src/state.rs, crates/goble-daemon-protocol/src/lib.rs, crates/goble-desktop-service/src/state.rs",
            ["S2"], g_s4,
            "cargo test -p goble-harness-internal -p goble-daemon -p goble-desktop-service: each of spawned/progress/finished maps across all six hops, and a scripted sequence emits chat:subagent_spawned, chat:subagent_progress and chat:subagent_finished with their fields intact"),
        item("S5", "S5", "The sub-agent row in the parent transcript", "subagents.md",
            "crates/goble-ui/src/elements/chat_message_bubble.rs, app/src/state.rs, app/src/ui/chat.rs",
            ["S4"], g_s5,
            "cargo test -p goble-ui -p goble-app: each status draws its own line with the activity and the elapsed time while running, the fold matches the other tool shapes, and a click fires the open action"),
        item("S6", "S6", "The child view", "subagents.md",
            "app/src/ui/terminal.rs, app/src/state.rs, app/src/actions.rs, crates/goble-ui/src/",
            ["S5"], g_s6,
            "cargo test -p goble-app: entering from the row shows the child's conversation with its persisted messages, Esc returns to the parent transcript, and the round trip can be made twice"),
        item("S7", "S7", "The work overlay", "subagents.md",
            "app/src/ui/mod.rs, crates/goble-ui/src/elements/ (new overlay body), app/src/state.rs, app/src/actions.rs",
            ["S4"], g_s7,
            "cargo test -p goble-ui -p goble-app: the sheet opens and closes by each route, lists the records it is given grouped by kind, a kill fires the cancel path, and a dismissed overlay does not re-open for the same run"),
    ]},
];

// ---------------------------------------------------------------- run

let rules = house_rules(root);
let status = #{};
let rows = [];

for p in passed_arg.split(",") {
    let t = p;
    t.trim();
    if t != "" { status[t] = "pass"; }
}

let write_shown = if write_model == "" { "session" } else { write_model };
let read_shown = if read_model == "" { "session" } else { read_model };

log("Agent UI rollout starting; waves=" + waves_arg + ", max_repairs=" + max_repairs.to_string());
log("Model routing: writing = " + write_shown + ", reading/verifying = " + read_shown);
if passed_arg != "" { log("Treating as already verified (deps satisfied): " + passed_arg); }
log("Standing rule: no `cargo build`/`cargo run`/`cargo install` for the GUI, and no commits. Every change stays in the working tree for review.");

for w in waves {
    let run_items = [];
    for it in w.items {
        if item_selected(waves_arg, w.n, it.id) { run_items.push(it); }
    }
    if run_items.len() == 0 { continue; }
    phase(w.phase_title);
    log("Wave " + w.n.to_string() + " (" + w.title + "): " + run_items.len().to_string() + " items");

    for it in run_items {
        let blocked_by = "";
        for d in it.deps {
            let dep_state = status[d];
            if dep_state == () || dep_state != "pass" {
                if blocked_by == "" { blocked_by = d; } else { blocked_by += ", " + d; }
            }
        }
        if blocked_by != "" {
            status[it.id] = "skip";
            rows.push(#{ ids: it.ids, title: it.title, state: "skipped", verdict: "dependency not satisfied: " + blocked_by, evidence: "" });
            log("SKIP " + it.ids + " - blocked by " + blocked_by);
            continue;
        }

        let r = agent(impl_prompt(it, rules, root), with_model(#{
            label: "impl:" + it.ids,
            capability_mode: "all",
            agent_type: "general-purpose",
            output_schema: impl_schema,
        }, write_model));
        let claim = "the implementer returned no usable report";
        if r != () && r.success && r.output != () && r.output.evidence != () {
            claim = r.output.evidence;
        }

        let v = agent(verify_prompt(it, claim, root), with_model(#{
            label: "verify:" + it.ids,
            capability_mode: "all",
            agent_type: "general-purpose",
            output_schema: verdict_schema,
        }, read_model));
        let passes = v != () && v.success && v.output != () && v.output.passes == true;
        let verdict_text = "no usable verdict";
        let evidence_text = "";
        if v != () && v.success && v.output != () {
            if v.output.reason != () { verdict_text = v.output.reason; }
            if v.output.evidence != () { evidence_text = v.output.evidence; }
        }

        let attempt = 0;
        while !passes && attempt < max_repairs {
            attempt += 1;
            log("REPAIR " + it.ids + " (attempt " + attempt.to_string() + "): " + verdict_text);
            let rr = agent(repair_prompt(it, rules, root, verdict_text, evidence_text), with_model(#{
                label: "repair:" + it.ids + ":" + attempt.to_string(),
                capability_mode: "all",
                agent_type: "general-purpose",
                output_schema: impl_schema,
            }, write_model));
            let claim2 = "the repair agent returned no usable report";
            if rr != () && rr.success && rr.output != () && rr.output.evidence != () {
                claim2 = rr.output.evidence;
            }
            let v2 = agent(verify_prompt(it, claim2, root), with_model(#{
                label: "reverify:" + it.ids + ":" + attempt.to_string(),
                capability_mode: "all",
                agent_type: "general-purpose",
                output_schema: verdict_schema,
            }, read_model));
            passes = v2 != () && v2.success && v2.output != () && v2.output.passes == true;
            if v2 != () && v2.success && v2.output != () {
                if v2.output.reason != () { verdict_text = v2.output.reason; }
                if v2.output.evidence != () { evidence_text = v2.output.evidence; }
            }
        }

        let state = if passes { "verified" } else { "failed" };
        status[it.id] = if passes { "pass" } else { "fail" };
        rows.push(#{ ids: it.ids, title: it.title, state: state, verdict: verdict_text, evidence: evidence_text });
        if passes {
            log("PASS " + it.ids + " - " + it.title);
        } else {
            log("FAIL " + it.ids + " - " + verdict_text);
        }
    }
}

// ---------------------------------------------------------------- integration

phase("Integration");
log("Integration: full suite plus adversarial review of the whole diff");

let suite_prompt = "Run the repository's full verification suite for the Rust repo at " + root + " and report exactly what you observe. Run, in this order: `cargo test --workspace`, then `cargo clippy -p goble-terminal -p goble-ui -p goble-core -p goble-app --all-targets --no-deps`, then `cargo fmt --check` (report only files the rollout touched if the tree already had unrelated formatting drift). ";
suite_prompt += "NEVER run `cargo build`, `cargo run` or `cargo install` for the app binary - that is forbidden in this repository. ";
suite_prompt += "Report lens='full-suite', ok=true only if every command you ran exited successfully (or, for clippy or fmt, if any failure is pre-existing drift you can demonstrate on an untouched file - say which), with the exact summary lines as evidence, plus findings listing any failing test with its name and message.";

let regression_prompt = "Adversarially review the WHOLE uncommitted diff of the Rust repo at " + root + " (`git status --short`, `git diff`) for regressions against what the repository already did. ";
regression_prompt += "Hunt for the failure mode where a change works in isolation but breaks an existing surface: a caller left un-updated, a test that was weakened or deleted rather than fixed, an assumption about ordering or ownership that another module still relies on, a public API changed without its call sites. Read the real code. ";
regression_prompt += "Do NOT edit anything and do NOT commit. Never build or run the GUI binary. ";
regression_prompt += "Report lens='regressions', ok=false if you find a concrete regression, with findings naming file and line plus evidence quoting the code.";

let design_prompt = "Audit the uncommitted diff of the Rust repo at " + root + " against the design rule of this feature: everything the agent does is drawn as native UI rows in the pager idiom - an accent rail, a one-character bullet, full-width background bands - and NOTHING is wrapped in a generic bordered card or box; and a segment that runs in the pane's terminal is drawn as a terminal block by the same code terminal mode uses, rather than redrawn as agent output. ";
design_prompt += "Read the changed rendering code and its call sites. Flag every remaining bordered card, every status guessed from a string instead of read from a field, and every place a tool result is rendered by a path other than the shared block renderer. ";
design_prompt += "Do NOT edit anything and do NOT commit. Report lens='card-free-rule', ok=false if the rule is violated anywhere, with findings naming the file and the offending construct.";

let honesty_prompt = "Audit the bookkeeping of the rollout in the Rust repo at " + root + ". Read `.agents/TRACKER.md`, the `Recently resolved` table in `.agents/RESOLVER.md`, and the uncommitted diff. ";
honesty_prompt += "For every item the tracker now marks `[x]`, find the test named as its proof, then RUN that test (`cargo test -p <crate> <test_name>`) and confirm it exists and passes. ";
honesty_prompt += "A `[x]` whose test does not exist, does not pass, or does not actually exercise the claim is a false claim - report it. Also confirm no agent committed anything (`git log --oneline -3` should show no new commits from this run) and that the forbidden `cargo build`/`cargo run`/`cargo install` were not used. ";
honesty_prompt += "Report lens='tracker-honesty', ok=true only if every `[x]` is backed by a test you ran and saw pass, with findings naming any false or unproven claim.";

let checks = parallel([
    with_model(#{ prompt: suite_prompt, label: "integ:suite", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema }, read_model),
    with_model(#{ prompt: regression_prompt, label: "integ:regressions", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema }, read_model),
    with_model(#{ prompt: design_prompt, label: "integ:design-rule", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema }, read_model),
    with_model(#{ prompt: honesty_prompt, label: "integ:tracker-honesty", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema }, read_model),
]);

let check_lines = "";
let c = 0;
for ch in checks {
    if ch != () && ch.success && ch.output != () {
        let lens = "check[" + c.to_string() + "]";
        if ch.output.lens != () { lens = ch.output.lens; }
        let ok_text = "unknown";
        if ch.output.ok == true { ok_text = "ok"; } else if ch.output.ok == false { ok_text = "PROBLEM"; }
        check_lines += "- " + lens + ": " + ok_text + "\n  evidence: " + ch.output.evidence + "\n";
        if ch.output.findings != () {
            for f in ch.output.findings {
                check_lines += "  - " + f + "\n";
            }
        }
    } else {
        check_lines += "- check[" + c.to_string() + "]: NO USABLE RESULT (treat as unverified)\n";
    }
    c += 1;
}

// ---------------------------------------------------------------- report

let verified = 0;
let failed = 0;
let skipped = 0;
for row in rows {
    if row.state == "verified" { verified += 1; }
    else if row.state == "failed" { failed += 1; }
    else { skipped += 1; }
}

let report = "# Agent UI rollout\n\n";
report += "Waves requested: `" + waves_arg + "`. Items: " + rows.len().to_string()
    + " - verified " + verified.to_string()
    + ", failed " + failed.to_string()
    + ", skipped " + skipped.to_string() + ".\n\n";
report += "Every change is UNCOMMITTED in the working tree on purpose: review the diff before it is kept.\n\n";
report += "Model routing for this run: writing = `" + write_shown + "`, reading/verifying = `" + read_shown + "`.\n\n";
report += "## Items\n\n| Item | State | Verdict |\n|---|---|---|\n";
for row in rows {
    report += "| " + row.ids + " - " + row.title + " | " + row.state + " | " + row.verdict + " |\n";
}
report += "\n## Evidence per item\n\n";
for row in rows {
    report += "### " + row.ids + " - " + row.title + "\n";
    report += "- state: " + row.state + "\n";
    report += "- verdict: " + row.verdict + "\n";
    report += "- evidence: " + row.evidence + "\n\n";
}
report += "## Integration checks\n\n" + check_lines;
report += "\n## What stays unverified\n\n";
report += "- Anything that needs a window: line wrapping in a real pane, the feel of following a stream, the agent-view round trip on screen. The standing rule forbids building or running the GUI binary, so these are inferred from element and app tests, never claimed as seen.\n";
report += "- The agent+terminal mode against a real zsh: the shell-integration scripts and the claim protocol are exercised by scripted sequences and decoder tests, not by a live interactive shell.\n";
report += "- P5 (in-band claim id) was deliberately out of scope: the plan makes it a hardening step taken only if the claim protocol's correlation proves insufficient.\n";
report += "- B1-B4 of terminal-blocks.md (layout sum-tree, prompt hiding, clear semantics, block persistence) are separate items and were not part of this run.\n";

let report_id = write_scratch_file("agent-ui-rollout-report.md", report);
log("Rollout finished: verified=" + verified.to_string() + ", failed=" + failed.to_string() + ", skipped=" + skipped.to_string());
complete(#{ path: report_id, report: report, verified: verified, failed: failed, skipped: skipped });
