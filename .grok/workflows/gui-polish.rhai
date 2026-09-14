// Workflow: the GUI punch list taken from the running app, one item at a time,
// each adversarially verified with the repo's own test commands.
//
// The agents never launch the GUI binary: they share one workspace and there is
// one window to one app instance. The on-screen pass is the orchestrator's, done
// after the run by building the app and screenshotting it.

let meta = #{
    name: "gui-polish",
    description: "Fix the running GUI punch list - chat geometry, topbar tabs, sidebar and work-overlay removals, and the shell's agent surface - one item at a time, each adversarially verified with the repo's own test commands",
    when_to_use: "Use to work through the GUI punch list against the goble native GUI. Never builds or runs the GUI binary; leaves every change uncommitted.",
    phases: [
        #{ title: "R4 Remove the work overlay" },
        #{ title: "R3 Remove the sidebar footers" },
        #{ title: "R2 Topbar tabs" },
        #{ title: "R1 Chat geometry" },
        #{ title: "R5 Shell agent surface" },
        #{ title: "Integration" },
    ],
};

let impl_schema = #{
    "type": "object",
    "required": ["done", "name", "summary", "changes", "command", "result", "unverified"],
    "properties": #{
        "done": #{ "type": "boolean" },
        "name": #{ "type": "string" },
        "summary": #{ "type": "string" },
        "changes": #{ "type": "array", "items": #{ "type": "string" } },
        "command": #{ "type": "string" },
        "result": #{ "type": "string" },
        "unverified": #{ "type": "string" },
    },
};

let verify_schema = #{
    "type": "object",
    "required": ["passes", "command", "result", "defects"],
    "properties": #{
        "passes": #{ "type": "boolean" },
        "command": #{ "type": "string" },
        "result": #{ "type": "string" },
        "defects": #{ "type": "array", "items": #{ "type": "string" } },
    },
};

let check_schema = #{
    "type": "object",
    "required": ["ok", "evidence"],
    "properties": #{
        "ok": #{ "type": "boolean" },
        "evidence": #{ "type": "string" },
    },
};

let root = "/Users/adrian.tucicovencogmail.com/Projects/goble";

let specs = [
    #{
        id: "R4",
        title: "Remove the work overlay",
        goal: "Remove the work-in-flight overlay (the panel titled Work in flight) that S7 added, because the user, looking at the running app, finds it redundant and wants it gone. Take out the element `crates/goble-ui/src/elements/work_overlay.rs` and its module registration; the WorkOverlay Sheet wiring and its on_close in `app/src/ui/mod.rs`; the Cmd/Ctrl+Shift+W binding in `app/src/root_view.rs`; and the cues that opened it - the footer still-running cue and the topbar running indicator - if and only if they exist only to open that panel. If either of those still carries information the user needs (how much work is running), keep that indicator but make it non-interactive and say so in your report. Remove the state that existed only for it: `work_overlay_open`, `work_overlay_groups`, `work_overlay_scroll`, `open_work_overlay`, `close_work_overlay`, the dismissal bookkeeping, the auto-open rule that watched the running count, and `on_kill_sub_agent`/`kill_sub_agent` if nothing else calls them (check first). Delete `app/src/integration_testing/work_overlay_flow.rs` and its registration in the integration-test list, and the element's unit tests. If the two `root_view` tests still carry the workaround that closed the auto-opened overlay, restore their premise to the plain pane-Esc behaviour. In `.agents/TRACKER.md`, mark the S7 row as removed with the date and the reason, and add a `.agents/RESOLVER.md` row recording the removal.",
        check: "cargo test -p goble-ui -p goble-app",
        verify: "Grep the tree for every surviving reference to the overlay - build_work_overlay, WorkOverlay, work_overlay_open, work_overlay_groups, work_overlay_scroll, open_work_overlay, close_work_overlay, on_close_work, on_kill_sub_agent, note_running_sub_agents, work_overlay_flow - and confirm each hit either is gone or has a stated remaining caller that is not the panel. Confirm `cargo check -p goble-app` and `cargo test -p goble-ui -p goble-app` both exit 0. Confirm no test was deleted except those that existed only for the panel, and that the two root_view tests were restored rather than deleted. Check the tracker row says removed and gives the reason.",
    },
    #{
        id: "R3",
        title: "Remove the sidebar footers",
        goal: "Remove both footer rows from the left conversation sidebar in `app/src/ui/sidebar.rs`: the row labelled Plugins with its plus TopbarButton that opened the MCP connectors panel, and the row labelled Screen with its monitor TopbarButton that opened the screen sheet. Keep the search field, the New conversation row, the conversation list, the View all control and the sidebar's scroll behaviour exactly as they are. Remove the actions, icons and imports that existed only for those two rows, and any test that asserts them. Both panels must stay reachable from the Cmd+K palette (Open connectors, Open screen) - do not touch those palette entries.",
        check: "cargo test -p goble-app",
        verify: "Read the final `build_sidebar` and confirm neither footer row remains and that the column still ends with the list rather than an empty divider. Grep `app/src/ui/` for the sidebar's use of on_open_connectors and on_open_screen and confirm those two rows are gone while the palette entries that use the same actions still exist. Run `cargo test -p goble-app` yourself and confirm exit 0.",
    },
    #{
        id: "R2",
        title: "Topbar tabs",
        goal: "In the general topbar, the workspace/space tabs (`build_workspace_strip` and `build_workspace_chip` in `app/src/ui/shell.rs`, plus the chip element they build) render today as rounded chips inset in the bar. The user wants them to read as browser-style tabs: each tab fills the full height of the topbar (`TOPBAR_HEIGHT`), its corners are square (no corner radius anywhere on the tab), and two adjacent tabs are separated by a single vertical divider line - one separator between neighbours, never a doubled line, and no gap that would read as two lines. The active tab must stay visually distinct using the existing theme tokens, the label must still become the inline rename field when `space_rename_editing` is set, and the tab's close control must still close that space. The + / dropdown control and the Settings icon keep their current place and size.",
        check: "cargo test -p goble-ui -p goble-app",
        verify: "Read the tab element's layout and paint and confirm all three: no corner radius on the tab, the tab's drawn height equals the topbar's height, and separators are drawn once per boundary rather than once per tab. Count what a strip of N tabs draws: N-1 separators between them, so no boundary carries two lines - a divider drawn by the tab plus one drawn by the bar is the classic way the doubled line appears. Run the acceptance command yourself and confirm exit 0.",
    },
    #{
        id: "R1",
        title: "Chat geometry",
        goal: "Change the chat transcript's geometry in the pane, on three counts at once. First, the agent's own replies must stop being a pill: no background fill, no border, no corner radius and no width cap, so the reply renders across the full width of the pane. Second, the user's own message also spans the full width; a grey background box is right, with square corners and no corner radius. Third, the transcript carries 10 to 15 px of horizontal padding at both edges so no line of text touches the pane's border. The code lives in `crates/goble-ui/src/elements/chat_message_bubble.rs` - the bubble container's background, its corner radius and its width cap - plus `crates/goble-ui/src/views/chat_view.rs` and the app's chat surface in `app/src/ui/chat.rs`. Keep everything that is not geometry exactly as it is: tool-call folds, reasoning folds, code blocks, diff blocks, terminal blocks, sub-agent rows, the streaming caret, every click target and the markdown rendering. Existing tests assert the old geometry, some by counting fills and stroke rects; update their premises to the new shape rather than deleting them.",
        check: "cargo test -p goble-ui -p goble-app",
        verify: "Read the final rendering path and confirm the assistant reply carries no background fill, no border and no corner radius and is not width-capped, while the user message keeps a grey fill with square corners. Confirm the horizontal padding at the transcript's own edges is between 10 and 15 px on both sides and that it comes from the transcript container rather than from an unrelated pane inset. Run the acceptance command yourself. Compare the test count against the counts this run started from (`goble_ui` lib 302 and `goble_app` lib 196 tests) and account for every difference - tests that asserted the old pill geometry must be updated, not removed.",
    },
    #{
        id: "R5",
        title: "Shell agent surface",
        goal: "Two things about the shell/terminal space. First, the terminal pane has no input bar at all today: the user can only type into the PTY grid, while Space 1 has a rich input bar at the bottom. Give the terminal space that same rich input bar at the bottom of the pane, built from the existing composer element (`crates/goble-ui/src/elements/chat_composer.rs`) so that it is literally the same widget and not a bespoke second one - the same send affordance, the same controls, in the same place at the bottom. Wire the actions it needs to the terminal pane's own state. If a control has no meaning for a terminal pane, omit that control and say which and why, rather than drawing a dead button. Second, when the harness (the agent session) is opened from a shell/terminal space, it must render with the same visual representation Space 1 uses, not the degraded one it shows today. Read `app/src/ui/terminal.rs`, `app/src/ui/panes.rs`, `app/src/ui/harness.rs` and the agent view path (`app/src/ui/chat.rs` and `crates/goble-ui/src/views/chat_view.rs`) to find the routing that decides which view a pane draws, and route the shell's harness through the same path.",
        check: "cargo test -p goble-ui -p goble-app",
        verify: "Read the pane-view routing and confirm a terminal pane with an open harness mounts the very same agent view element Space 1 mounts - name the single function both call, because a second copy of that view is not this item. Confirm the terminal pane's bottom bar is the shared composer element rather than a bespoke one. Run the acceptance command yourself. If either half is only partly done, say so and fail the item; do not pass it on the strength of the half that works.",
    },
];

let rules = "";
rules += "Repository root: " + root + ". Work only inside it. ";
rules += "Read the real files before editing and never guess an API. ";
rules += "Do NOT run the GUI: never `cargo run`, never execute ./target/debug/goble-app - other agents share this workspace and there is a single window onto one app instance. ";
rules += "Do not commit: leave every change in the working tree. ";
rules += "Never make a test pass by weakening it. Do not delete an assertion, do not add #[ignore], do not narrow a test to a case it already covered. If a test encodes behaviour this item deliberately changes, rewrite its premise and say so explicitly in your report. ";
rules += "When the change is done, update the owning row in .agents/TRACKER.md and add a row to .agents/RESOLVER.md carrying the exact command you ran and its real output. ";

let model = if args == () || args.model == () { "deepseek" } else { args.model };
let only = if args == () || args.only == () { "" } else { args.only };

let results = [];
for s in specs {
    if only != "" && only != s.id {
        log("skipping " + s.id + " (args.only = " + only + ")");
        continue;
    }
    phase(s.id + " " + s.title);
    let attempt = 0;
    let solved = false;
    let last = "";
    while attempt < 2 && !solved {
        let p = rules;
        if attempt == 0 {
            p += "Resolve exactly ONE item, " + s.id + " - " + s.title + ". ";
        } else {
            p += "A previous attempt at " + s.id + " did not survive adversarial verification. What the verifier reported: " + last + " ";
            p += "Repair the same item " + s.id + " - " + s.title + " - rather than starting a different one. ";
        }
        p += "What must be true when you are done: " + s.goal + " ";
        p += "Acceptance: the command `" + s.check + "` must exit 0 with no failing test, and the `result` field must carry the real tail of its output. ";
        p += "Keep the change as small as the item allows and keep every behaviour the item does not name. ";
        p += "Report with the schema: `done` (true only when the acceptance command passed), `name`, `summary` of what the code does now, one `changes` line per file, `command`, `result`, and `unverified` for what you could not check and why.";

        let r = agent(p, #{ label: "impl:" + s.id, capability_mode: "all", output_schema: impl_schema, model: model });
        let impl_ok = r != () && r.success && r.output.done == true;
        if !impl_ok {
            if r != () && r.success {
                last = "the implementer reported done=false: " + json_encode(r.output.unverified) + " " + json_encode(r.output.summary);
            } else {
                last = "the implementer produced no usable report";
            }
            attempt += 1;
        } else {
            let vp = "";
            vp += "Adversarially verify one item in the Goble repository at " + root + ". ";
            vp += "The item is " + s.id + " - " + s.title + ". What it required: " + s.goal + " ";
            vp += "The implementer claims: " + json_encode(r.output.summary) + " and reports these files: " + json_encode(r.output.changes) + ". ";
            vp += "Its claimed command output was: " + json_encode(r.output.result) + ". ";
            vp += "Do the work yourself: read the actual code on disk and run `" + s.check + "` yourself. ";
            vp += "Check specifically: " + s.verify + " ";
            vp += "You must NOT edit any file, and you must NOT run the GUI binary. Review only. ";
            vp += "Fail closed: set passes=true only when you reproduced the acceptance command's exit 0 and read the code that makes the claim true. Missing or unusable evidence is not a pass. ";
            vp += "Put each concrete defect in `defects`, and the real tail of the command you ran in `result`.";

            let v = agent(vp, #{ label: "verify:" + s.id, capability_mode: "all", output_schema: verify_schema, model: model });
            if v != () && v.success && v.output.passes == true {
                solved = true;
                results.push(#{ id: s.id, title: s.title, state: "verified", verdict: v.output.result, evidence: r.output.result });
            } else {
                if v != () && v.success {
                    last = json_encode(v.output.defects) + " (verifier output: " + json_encode(v.output.result) + ")";
                } else {
                    last = "the verifier produced no usable verdict";
                }
                attempt += 1;
            }
        }
    }
    let state = "failed";
    if solved {
        state = "verified";
    }
    if !solved {
        results.push(#{ id: s.id, title: s.title, state: "failed", verdict: last, evidence: "" });
    }
    log(s.id + ": " + state);
}

// ---- Integration -----------------------------------------------------------
phase("Integration");

let suite_p = "";
suite_p += "Integration gate on the Goble repository at " + root + ". Do NOT edit any file and do NOT run the GUI binary. ";
suite_p += "Run, from the repository root, `cargo test --workspace` and `cargo clippy -p goble-ui -p goble-app --all-targets --no-deps`, plus `cargo check --workspace --all-targets`. ";
suite_p += "Report `ok` only when every one of those exits 0 with no failing test; in `evidence` give the aggregate numbers - how many test-result lines, total passed, total failed, total ignored - and paste the key lines, including any warning that points at code changed by this run and any test that was ignored or removed.";

let checks = [
    #{
        output_schema: check_schema,
        prompt: "Regression hunt on the Goble repository at " + root + ", reading only; do not edit files and do not run the GUI binary. Five changes landed in this working tree: the work-in-flight overlay was removed along with its cues and state; the sidebar's Plugins and Screen footer rows were removed; the topbar's space chips became full-height square tabs with single separators; the chat transcript's geometry changed - no assistant pill, full width for both roles, square corners, 10-15 px side padding; and the shell/terminal space gained the shared composer at the bottom plus the agent view for its harness. Look for what broke: dead actions or icons left with no caller, a pane view that no longer mounts, a route that lost its only entry point, an overlay that can no longer be closed, an import or state field that is now unused but still compiled. Use grep and read_file over `app/src` and `crates/goble-ui/src`, and run `cargo test -p goble-ui -p goble-app` to see the suites. Report `ok=false` with concrete file and line evidence for anything you find, or `ok=true` naming what you checked.",
        label: "check:regressions", capability_mode: "all",
    },
    #{
        output_schema: check_schema,
        prompt: "Honesty check on the Goble repository at " + root + ", reading only; do not edit files and do not run the GUI binary. Each item in this run was supposed to leave a row in `.agents/TRACKER.md` and a row in `.agents/RESOLVER.md` carrying the real command it ran and the real output. Read those two files and the code they describe, and check every claim made by a row added recently: does the named file, function or behaviour exist as described, and does the quoted command output look like something the current tree actually produces? Run at least two of the quoted commands yourself to see. Report `ok=false` naming any claim the code does not support, or `ok=true` naming which claims you verified.",
        label: "check:honesty", capability_mode: "all",
    },
    #{
        output_schema: check_schema,
        prompt: "Test-integrity check on the Goble repository at " + root + ", reading only; do not edit files and do not run the GUI binary. Before this run the suites held `goble_ui` lib 302 tests and `goble_app` lib 196 tests, plus a `work_overlay_flow` integration suite of 6 tests that this run was asked to delete because its subject was removed. Run `cargo test -p goble-ui -p goble-app` and `cargo test --workspace` and compare. Account for every difference: the overlay's removal justifies deleting its element tests and its integration suite, and the chat geometry change justifies updating bubble-geometry assertions, but nothing justifies losing coverage of the tool-call folds, reasoning folds, streaming, pane views, composer or sub-agent rows. Report `ok=false` with the specific lost assertion if you find coverage that disappeared without its subject being removed, or `ok=true` with the counts you saw.",
        label: "check:tests", capability_mode: "all",
    },
];
let suite = agent(suite_p, #{ label: "check:full-suite", capability_mode: "all", output_schema: check_schema, model: model });
let check_results = parallel(checks);

let verified = 0;
let failed = 0;
for r in results {
    if r.state == "verified" {
        verified += 1;
    } else {
        failed += 1;
    }
}

let report = "# GUI polish\n\n";
report += "Items: " + results.len().to_string() + " - verified " + verified.to_string() + ", failed " + failed.to_string() + ".\n\n";
report += "Every change is UNCOMMITTED in the working tree on purpose: review the diff before it is kept.\n\n";
report += "Model routing for this run: every role ran on `" + model + "`.\n\n";
report += "## Items\n\n| Item | State | Verdict |\n|---|---|---|\n";
for r in results {
    report += "| " + r.id + " - " + r.title + " | " + r.state + " | " + r.verdict + " |\n";
}
report += "\n## Evidence per item\n\n";
for r in results {
    report += "### " + r.id + " - " + r.title + "\n";
    report += "- state: " + r.state + "\n";
    report += "- evidence: " + r.evidence + "\n\n";
}
report += "## Integration checks\n\n";
if suite != () && suite.success && type_of(suite.output) == "map" {
    report += "- full-suite: ok=" + suite.output.ok.to_string() + "\n  evidence: " + suite.output.evidence + "\n";
} else {
    report += "- full-suite: no usable result\n";
}
let ci = 0;
for c in check_results {
    let label = "check " + ci.to_string();
    if c != () && c.success {
        if type_of(c.output) == "map" {
            report += "- " + label + ": ok=" + c.output.ok.to_string() + "\n  evidence: " + c.output.evidence + "\n";
        } else {
            report += "- " + label + ": " + c.output.to_string() + "\n";
        }
    } else {
        report += "- " + label + ": no usable result\n";
    }
    ci += 1;
}
report += "\n## Not verified by this run\n\n";
report += "The on-screen look. The agents were forbidden from launching the GUI binary; the orchestrator builds and screenshots the app afterwards.\n";

let path = write_scratch_file("gui-polish-report.md", report);
log("report: " + path);
complete(#{ path: path, report: report });
