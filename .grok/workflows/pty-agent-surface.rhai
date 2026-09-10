// Self-evaluation workflow for the "PTY as agent + terminal surface" feature.
// Phases run sequentially in the same workspace. Agents are read-only for
// inspection and only run `cargo check`/`cargo test` for the verify gate; they
// must NOT launch the GUI (never `cargo run`). Every feature is marked done only
// if its inspector confirmed the symbol/behavior is present AND the final cargo
// check passed (fail closed).

let meta = #{
    name: "pty-agent-surface",
    description: "Self-evaluate the PTY dual-mode agent-terminal surface (TUI agent detection, native-first routing, mode badge, launch action) in the goble native GUI",
    when_to_use: "Run against the goble PTY/terminal code to confirm the agent-surface feature set is implemented and the workspace still checks + tests green, without relaunching the app.",
    phases: [
        #{ title: "Inspect", detail: "one read-only inspector per feature piece" },
        #{ title: "Verify", detail: "cargo check + cargo test gate" },
    ],
};

let inspect_schema = #{
    "type": "object",
    "required": ["ok", "present", "missing", "detail"],
    "properties": #{
        "ok": #{ "type": "boolean" },
        "present": #{ "type": "array", "items": #{ "type": "string" } },
        "missing": #{ "type": "array", "items": #{ "type": "string" } },
        "detail": #{ "type": "string" },
    },
};

let verify_schema = #{
    "type": "object",
    "required": ["checks_pass", "tests_pass", "results"],
    "properties": #{
        "checks_pass": #{ "type": "boolean" },
        "tests_pass": #{ "type": "boolean" },
        "results": #{ "type": "string" },
    },
};

let repo = "/Users/adrian.tucicovencogmail.com/Projects/goble";
let base = "Project root: " + repo + ". This is a native wgpu/winit GUI; you must NOT launch it (never run `cargo run`). "
    + "Read the actual files with read_file / grep before answering — do not answer from memory. "
    + "Return ok=true ONLY if you confirmed every listed item is present in the current source. "
    + "Return ok=false if any item is missing or you were not able to confirm it, and list what is missing. ";

fn is_ok(r) { r != () && r.success }
fn done(r) { r != () && r.success && r.output.ok == true }

// ---- Phase: Inspect ---------------------------------------------------------
phase("Inspect");
let jobs = [];

jobs.push(#{
    label: "inspect:tui-agent-detection",
    prompt: base
        + "Feature: TUI agent detection. In " + repo + "/app/src/terminal.rs confirm all of: "
        + "(a) `pub enum TuiAgent` with variants Codex/Claude/Gemini/OpenCode/Cursor/Aider/Other, "
        + "(b) `impl TuiAgent { pub fn detect(&str) -> Option<TuiAgent> }` that matches the first command token, "
        + "(c) a private `fn first_command_token` that skips wrapper tokens (sudo/command/exec/npx/bunx/env) and strips a leading path. "
        + "If all present, ok=true; else ok=false listing the missing items.",
    capability_mode: "read-only",
    output_schema: inspect_schema,
});
jobs.push(#{
    label: "inspect:terminal-mode",
    prompt: base
        + "Feature: terminal pane surface mode. In " + repo + "/app/src/terminal.rs confirm: "
        + "(a) `pub enum TerminalMode { Shell, Agent(TuiAgent) }`, "
        + "(b) `TerminalRegistry` has a `modes: HashMap<u64, TerminalMode>` field, "
        + "(c) `TerminalRegistry` has `mode(u64)`, `set_mode(u64, TerminalMode)`, `update_mode_from_input(u64) -> Option<TuiAgent>`, and `launch_agent(u64, &str)`, "
        + "(d) `drop_pane` clears the pane's mode. If all present, ok=true; else ok=false listing missing items.",
    capability_mode: "read-only",
    output_schema: inspect_schema,
});
jobs.push(#{
    label: "inspect:native-first-routing",
    prompt: base
        + "Feature: native-first input routing. In " + repo + "/app/src/ui/terminal.rs confirm: "
        + "(a) in `handle_key`, the `Forward` arm calls `reg.update_mode_from_input(self.pane_id)` when the forwarded bytes are a newline (before the input mirror is cleared), "
        + "(b) in `handle_key`, the `RouteToAgent` arm checks the pane mode and, when it is `TerminalMode::Agent(_)`, forwards a newline to the pty and returns true instead of routing to the agent harness. "
        + "If both present, ok=true; else ok=false listing what is missing.",
    capability_mode: "read-only",
    output_schema: inspect_schema,
});
jobs.push(#{
    label: "inspect:mode-badge",
    prompt: base
        + "Feature: agent mode badge. In " + repo + "/app/src/ui/terminal.rs confirm the `rebuild` function reads the pane mode and, when it is `TerminalMode::Agent(agent)`, prepends a header badge Text showing `agent · <label>` using the label from the agent. "
        + "If present, ok=true; else ok=false listing what is missing.",
    capability_mode: "read-only",
    output_schema: inspect_schema,
});
jobs.push(#{
    label: "inspect:launch-action",
    prompt: base
        + "Feature: launch-a-TUI-agent action threaded through the app. Confirm ALL of: "
        + "(a) `UiActions` in " + repo + "/app/src/ui/mod.rs has `on_launch_tui_agent: Rc<RefCell<dyn FnMut(u64, String)>>`, "
        + "(b) `make_actions` in " + repo + "/app/src/actions.rs wires it to `reg.ensure_session(pane_id, \"\")` + `reg.launch_agent(pane_id, &command)`, "
        + "(c) `build_pane_header` in " + repo + "/app/src/ui/panes.rs renders a run-agent menu (a `build_run_agent` helper using a `PopupMenu` of `TuiAgent::ALL`) for terminal panes, "
        + "(d) a `terminal_run_agent_menu_open: Rc<RefCell<bool>>` flag exists in `UiSnapshot`/`UiState` and is cloned into the snapshot in " + repo + "/app/src/root_view.rs. "
        + "If all present, ok=true; else ok=false listing the missing items.",
    capability_mode: "read-only",
    output_schema: inspect_schema,
});
jobs.push(#{
    label: "inspect:tests",
    prompt: base
        + "Feature: regression tests. In " + repo + "/app/src/terminal.rs confirm unit tests exist covering: agent detection (`detects_known_tui_agents_by_first_token`), "
        + "wrapper/path skipping (`detection_skips_wrappers_and_paths`), rejection of shell commands (`detection_rejects_shell_commands`), "
        + "mode default/set/drop (`registry_mode_defaults_to_shell`), `update_mode_from_input`, and `launch_agent` mode settling. "
        + "If these tests (or equivalent coverage) are present, ok=true; else ok=false listing what is missing.",
    capability_mode: "read-only",
    output_schema: inspect_schema,
});

let results = parallel(jobs);
let inspect = [];
let i = 0;
for r in results {
    if is_ok(r) {
        inspect.push(r.output);
    } else {
        inspect.push(#{ ok: false, present: [], missing: [ "inspector failed" ], detail: "inspector did not return" });
    }
    i += 1;
}

let all_inspects_ok = true;
for r in inspect {
    if r.ok != true { all_inspects_ok = false; }
}

// ---- Phase: Verify ----------------------------------------------------------
phase("Verify");
let p = "";
p += "Self-evaluate the goble 'PTY agent + terminal surface' feature at " + repo + ". Read files to confirm the changes are present; do not edit anything. ";
p += "Run and capture the tail of each: `cargo check --workspace --all-targets 2>&1 | tail -40` and ";
p += "`cargo test -p goble-app --lib -p goble-ui -p goble-desktop-service 2>&1 | grep -E 'test result:|FAILED|error\\[' | sort | uniq -c`. ";
p += "Do NOT launch the GUI (never `cargo run`). Report checks_pass = (cargo check exit code 0), ";
p += "tests_pass = (no FAILED or error[ in the test output), and results = a terse bullet list of (a) the cargo check outcome, "
    + "(b) the per-crate test counts, (c) any warnings about unused code or leftovers from removed symbols.";
let verify = agent(p, #{
    label: "verify",
    phase: "verify",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: verify_schema,
});

let checks_pass = is_ok(verify) && verify.output.checks_pass == true;
let tests_pass = is_ok(verify) && verify.output.tests_pass == true;
let checks_gate = all_inspects_ok && checks_pass;

let feature_labels = [
    "tui-agent-detection",
    "terminal-mode",
    "native-first-routing",
    "mode-badge",
    "launch-action",
    "tests",
];
let features = [];
let fi = 0;
for r in inspect {
    features.push(#{ "feature": feature_labels[fi], "done": r.ok == true });
    fi += 1;
}

let report = #{
    checks_pass: checks_pass,
    tests_pass: tests_pass,
    all_inspects_ok: all_inspects_ok,
    features: features,
    inspect: inspect,
    verify: if is_ok(verify) { verify.output } else { () },
};

let text = "goble PTY agent+terminal surface — self-evaluation\n";
text += "cargo check --workspace --all-targets: " + (if checks_pass { "PASS" } else { "FAIL" }) + "\n";
text += "tests (app/ui/desktop-service): " + (if tests_pass { "PASS" } else { "FAIL" }) + "\n";
text += "all inspectors confirmed: " + (if all_inspects_ok { "yes" } else { "no" }) + "\n\n";
text += "Features (done = inspector confirmed & present):\n";
for f in features {
    text += "  - " + f.feature + ": " + (if f.done { "done" } else { "open" }) + "\n";
}

complete(report);
