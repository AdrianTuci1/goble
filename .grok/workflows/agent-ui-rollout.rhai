// Implement the agent-UI plan (.agents/06-renderer/agent-parser.md, agent-tui.md,
// agent-terminal-bridge.md) as a long-running task: one tracker item at a time,
// each one adversarially verified with the repo's own test commands.
//
// Standing rules encoded in every prompt: never build/run the goble-app binary,
// one item per agent, no placeholders, no commits, tracker updated only with proof.

let meta = #{
    name: "agent-ui-rollout",
    description: "Implement the agent-UI plan in dependency waves - text pipeline, tool-call status, terminal bridge, card-free surface - one tracker item at a time, each adversarially verified with the repo's own test commands.",
    when_to_use: "Use to advance the pending items of .agents/06-renderer/agent-parser.md, agent-tui.md and agent-terminal-bridge.md. Never builds or runs the GUI binary; leaves every change uncommitted for review.",
    phases: [
        #{ title: "Wave 1: Foundations", detail: "shell hooks, transcript scrolling, parse-once" },
        #{ title: "Wave 2: Text", detail: "markdown, italic, mono code, diff rows" },
        #{ title: "Wave 3: Carriers", detail: "tool-call status, live events, reasoning" },
        #{ title: "Wave 4: Bridge", detail: "block ownership, claim protocol, pane routing" },
        #{ title: "Wave 5: Surface", detail: "inline rows, presenters, agent view, approval" },
        #{ title: "Wave 6: Integration", detail: "full suite plus adversarial review of the whole diff" },
    ],
};

// ---------------------------------------------------------------- args

let waves_arg = if args == () { "all" } else if args.waves == () { "all" } else { args.waves };
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
];

// ---------------------------------------------------------------- run

let rules = house_rules(root);
let status = #{};
let rows = [];

log("Agent UI rollout starting; waves=" + waves_arg + ", max_repairs=" + max_repairs.to_string());
log("Standing rule: no `cargo build`/`cargo run`/`cargo install` for the GUI, and no commits. Every change stays in the working tree for review.");

for w in waves {
    if !wave_selected(waves_arg, w.n) { continue; }
    phase(w.phase_title);
    log("Wave " + w.n.to_string() + " (" + w.title + "): " + w.items.len().to_string() + " items");

    for it in w.items {
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

        let r = agent(impl_prompt(it, rules, root), #{
            label: "impl:" + it.ids,
            capability_mode: "all",
            agent_type: "general-purpose",
            output_schema: impl_schema,
        });
        let claim = "the implementer returned no usable report";
        if r != () && r.success && r.output != () && r.output.evidence != () {
            claim = r.output.evidence;
        }

        let v = agent(verify_prompt(it, claim, root), #{
            label: "verify:" + it.ids,
            capability_mode: "all",
            agent_type: "general-purpose",
            output_schema: verdict_schema,
        });
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
            let rr = agent(repair_prompt(it, rules, root, verdict_text, evidence_text), #{
                label: "repair:" + it.ids + ":" + attempt.to_string(),
                capability_mode: "all",
                agent_type: "general-purpose",
                output_schema: impl_schema,
            });
            let claim2 = "the repair agent returned no usable report";
            if rr != () && rr.success && rr.output != () && rr.output.evidence != () {
                claim2 = rr.output.evidence;
            }
            let v2 = agent(verify_prompt(it, claim2, root), #{
                label: "reverify:" + it.ids + ":" + attempt.to_string(),
                capability_mode: "all",
                agent_type: "general-purpose",
                output_schema: verdict_schema,
            });
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

phase("Wave 6: Integration");
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
    #{ prompt: suite_prompt, label: "integ:suite", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema },
    #{ prompt: regression_prompt, label: "integ:regressions", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema },
    #{ prompt: design_prompt, label: "integ:design-rule", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema },
    #{ prompt: honesty_prompt, label: "integ:tracker-honesty", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema },
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
