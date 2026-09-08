// Workflow: rework goble's input model to the warp-new model and surface real
// harness functionality, self-evaluating per phase. Phases run sequentially in
// the same workspace so edits accumulate. Agents must NOT launch the GUI (no
// `cargo run`); verify with `cargo check` + `cargo test`.
//
// Canonical direction: the LOCAL state (goble-daemon/harness substrate + the
// "agent" naming) is the source of truth. Do not resurrect the upstream
// `backup/gui-upstream` direction (goblin-worker backend / "routine" naming).

let meta = #{
    name: "agent-real-input",
    description: "goble: default-terminal input model + parser (warp-new), Cmd+Enter new conversation, '!' terminal in agent mode, no-model text reply, topbar pages over real harness data, drop mock fallbacks",
    when_to_use: "Run against the goble native GUI to switch the input to the warp-new terminal/agent model and surface real harness functionality, reporting per-feature progress without relaunching the app.",
    phases: [
        #{ title: "Rename", detail: "'New agent' -> 'New conversation'; creates a conversation/session" },
        #{ title: "Input", detail: "default terminal + parser + Cmd+Enter new conversation + '!' in agent mode" },
        #{ title: "NoModel", detail: "visible text reply when no model is configured" },
        #{ title: "Pages", detail: "topbar pages (workflows/tasks/timeline/costs/MCPs) over real data" },
        #{ title: "NoMocks", detail: "drop mock fallbacks where a real backend exists" },
        #{ title: "Verify", detail: "workspace cargo check + tests + report" },
    ],
};

let impl_schema = #{
    "type": "object",
    "required": ["done", "features", "summary", "issues"],
    "properties": #{
        "done": #{ "type": "boolean" },
        "features": #{ "type": "array", "items": #{ "type": "string" } },
        "summary": #{ "type": "string" },
        "issues": #{ "type": "array", "items": #{ "type": "string" } },
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

fn is_ok(r) { r != () && r.success }
fn done(r) { r != () && r.success && r.output.done == true }

// ---- Phase: Rename ----------------------------------------------------------
phase("Rename");
let p = "";
p += "At /Users/adrian.tucicovencogmail.com/Projects/goble, rename the 'New agent' action to 'New conversation' so it clearly creates a conversation/session. ";
p += "Read the actual files before editing. Do NOT run the GUI (never `cargo run`); verify with `cargo check -p goble-app -p goble-ui` and `cargo test`. ";
p += "The sidebar row is app/src/ui/sidebar.rs (~line 66 `Text::new(\"New agent\")`; the row's hover flag is state.new_agent_hover, keep the field name). ";
p += "The default conversation title is set in app/src/actions.rs on_create_submit (~line 175 `\"New agent\".to_string()`) and the fallback title in app/src/state.rs (~line 538 `\"New conversation\"`). ";
p += "Update every user-facing 'New agent' string to 'New conversation' in app/src. ";
p += "Update tests that assert the old label: app/src/integration_testing/chat_flow.rs (~line 82 asserts conversations[0].name == \"New agent\") and any other assertion; change to \"New conversation\". ";
p += "Confirm clicking the row creates a real conversation/session via on_create_submit -> DesktopState::create_chat and binds it to the active pane. ";
p += "ACCEPTANCE: `cargo check -p goble-app -p goble-ui` exits 0; `cargo test -p goble-ui -p goble-app` exits 0 (update any test relying on the old label). ";
p += "Return JSON via output_schema: done = (renamed AND green), features = ['rename-new-conversation'], summary = what you changed, issues = anything incomplete.";
let rename = agent(p, #{
    label: "rename",
    phase: "rename",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let rename_done = done(rename);

// ---- Phase: Input -----------------------------------------------------------
phase("Input");
let p = "";
p += "At /Users/adrian.tucicovencogmail.com/Projects/goble, rework the pane input to the warp-new model. Read files first. Do NOT launch the GUI; verify with `cargo check -p goble-app -p goble-ui` and `cargo test`. ";
p += "REFERENCE the warp-new input model: app/src/terminal/view.rs in /Users/adrian.tucicovencogmail.com/Projects/warp-new (submit-to-local-agent = cmd-enter / ctrl-shift-enter, always starts a NEW conversation), and its terminal command parser. ";
p += "CANONICAL DIRECTION = LOCAL state (goble-daemon/harness + 'agent' naming). Do NOT use the goblin-worker / 'routine' direction or revert harness_runner.rs. ";
p += "KEY FILES: app/src/terminal.rs (classify_key ~205, is_agent_enter ~195, update_input_mirror ~255, TerminalKeyAction ~184), app/src/ui/terminal.rs (handle_key ~221 maps Forward to the pty and RouteToAgent to on_route), app/src/ui/chat.rs (composer wiring ~152-190), app/src/actions.rs (on_send_message ~233-337, on_agent_enter/route), crates/goble-ui/src/elements/chat_composer.rs (with_on_submit ~428, dispatch_event ~687), app/src/root_view.rs (global KeyDown ~583). ";
p += "IMPLEMENT this input model: ";
p += "(A) DEFAULT = TERMINAL command. The bottom input is a prompt line; pressing Enter (no modifier) runs the input as a terminal command in the pane's shell, not an agent turn. ";
p += "(B) Cmd/Ctrl+Enter sends the input to the agent as a NEW agent conversation (warp-new behavior), instead of Enter doing that. ";
p += "(C) A terminal-command parser classifies an input line (a real parser, not a stub): by default an input is a terminal command; in agent mode (an active agent conversation) an input is an agent prompt unless it starts with '!' (then it is a terminal command with the '!' stripped). ";
p += "(D) From BOTH modes (terminal and agent) the user can run terminal commands; in agent mode prefix '!' to force a terminal command. ";
p += "(E) Actually execute terminal commands: each pane (chat or terminal) must have a real PTY session (app/src/terminal.rs TerminalSession::spawn / TerminalRegistry) so a terminal command runs and its output is shown. Draw the terminal output inline or route it to a real terminal view. ";
p += "Keep existing behaviors working: create a NEW conversation on Cmd+Enter (bind the pane to a fresh conversation/session), and a conversation turn still goes through app/src/runtime.rs run_turn -> DaemonModel. ";
p += "ACCEPTANCE: `cargo check -p goble-app -p goble-ui` green; `cargo test -p goble-ui -p goble-app` green. Add unit tests for the parser (shortest: an input like `ls`/`git status` is a terminal command; `hello world` is an agent prompt; `!foo` is a terminal command in agent mode). ";
p += "Return JSON: done, features = ['input-default-terminal','input-parser','cmdenter-new-conversation','bang-terminal'], summary, issues. If this is large and cannot all compile in one pass, still make the parser + routing and report what is left.";
let input = agent(p, #{
    label: "input",
    phase: "input",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let input_done = done(input);

// ---- Phase: NoModel ---------------------------------------------------------
phase("NoModel");
let p = "";
p += "At /Users/adrian.tucicovencogmail.com/Projects/goble, make the agent input return visible text even when no model/API key is configured. Read files first. Do NOT run the GUI; verify with `cargo check -p goble-app -p goble-ui` and `cargo test`. ";
p += "In app/src/actions.rs on_send_message, the `!configured` branch (~line 321-330) currently adds the user message and sets state.show_llm_key_banner = true, but produces NO assistant reply. ";
p += "Change it so a real assistant text message is added to the conversation/pane transcript when no model is configured, instead of only showing a banner. The reply must be HONEST and useful, not a fabricated AI answer: explain that no model is configured (state.settings_llm_model / settings_llm_api_key are empty), and that the user can configure a model in Settings OR prefix '!' to run the line as a terminal command (consistent with the input model). ";
p += "Keep showing the model-key banner as well. If the model key is configured but the provider/model is empty, also surface a clear message rather than silently failing. ";
p += "ACCEPTANCE: `cargo check -p goble-app -p goble-ui` green; `cargo test -p goble-ui -p goble-app` green. Add/adjust a test proving that submitting with no configured model still appends an assistant message. ";
p += "Return JSON: done, features = ['nomodel-text-reply'], summary, issues.";
let nomodel = agent(p, #{
    label: "nomodel",
    phase: "nomodel",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let nomodel_done = done(nomodel);

// ---- Phase: Pages -----------------------------------------------------------
phase("Pages");
let p = "";
p += "At /Users/adrian.tucicovencogmail.com/Projects/goble, add topbar pages that surface REAL harness functionality, not mocks. Read files first. Do NOT launch the GUI; verify with `cargo check -p goble-app -p goble-ui` and `cargo test`. ";
p += "The topbar is app/src/ui/shell.rs build_topbar (~46-155, left = menu + projects, right = inbox + settings(disabled)) and build_main (~190-202) switches on AppTab (app/src/ui/mod.rs ~139: Chat/Settings/Projects, currently Settings is disabled). ";
p += "Add navigation entries (topbar buttons that open pages) for: Workflows, Tasks/Executions, Timeline, Costs, and MCPs/Connectors. Reuse existing page modules where present: app/src/ui/connectors.rs (MCP/connectors), app/src/ui/crons.rs (scheduled tasks), app/src/ui/projects.rs, app/src/ui/vault.rs. ";
p += "Wire the pages to REAL data from the embedded daemon / store: goble-desktop-service (DesktopState) and goble-daemon expose workflows (goble-workflow / DaemonPort::run_workflow), executions, MCP servers (goble-core mcp_manager/mcp_registry), projects/sessions/tasks (goble-persistence). Read real data via the daemon/store rather than hardcoded arrays. ";
p += "If a requested surface has NO backend yet (e.g. 'costs'), derive it from real execution/usage records if available, else render an honest data-driven empty/derived state and report the gap in `issues` — do NOT invent fake mock data in the UI. ";
p += "Add the new AppTab variants + build_main arms + topbar buttons + action wiring (UiActions in app/src/ui/mod.rs and handlers in app/src/actions.rs). ";
p += "ACCEPTANCE: `cargo check -p goble-app -p goble-ui` green; `cargo test -p goble-ui -p goble-app` green. Add a smoke test that navigates to at least one new tab and back to Chat. ";
p += "Return JSON: done, features = ['topbar-pages','real-workflows','real-tasks','real-timeline','real-costs','real-mcp'], summary, issues (name any surface that still needs a backend).";
let pages = agent(p, #{
    label: "pages",
    phase: "pages",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let pages_done = done(pages);

// ---- Phase: NoMocks ---------------------------------------------------------
phase("NoMocks");
let p = "";
p += "At /Users/adrian.tucicovencogmail.com/Projects/goble, remove mock fallbacks where a REAL backend exists so the app runs real functionality. Read files first. Do NOT launch the GUI; verify with `cargo check -p goble-app -p goble-ui` and `cargo test`. ";
p += "Mock sources: app/src/media/state.rs (build_mock ~129, MediaState::mock ~205, refresh_falls_back_to_mock_without_a_store ~555), app/src/projects/state.rs (ProjectsState::mock ~45), app/src/screen/state.rs (ScreenState::mock ~427 + MOCK_SOURCE ~26), app/src/daemon/state.rs (~189/224 'mock'), app/src/state.rs (UiState::mock ~879). ";
p += "Where a real backend exists (DesktopState store / embedded daemon / goble-screen-core registry), make the default path use it and only keep mock as an explicit TEST fixture (a `#[cfg(test)]` helper or a clearly-labeled test-only factory), NOT as a runtime fallback when the store fails to open. If the store genuinely cannot open, surface an error state instead of silently substituting mock data. ";
p += "Do NOT remove mock data that is only used by tests; keep test helpers compiling. Update tests that construct mock state via the production path. ";
p += "ACCEPTANCE: `cargo check -p goble-app -p goble-ui` green; `cargo test -p goble-ui -p goble-app -p goble-desktop-service` green. Report in `issues` any mock you intentionally left (and why). ";
p += "Return JSON: done, features = ['no-mock-media','no-mock-projects','no-mock-screen','no-mock-daemon'], summary, issues.";
let nomocks = agent(p, #{
    label: "nomocks",
    phase: "nomocks",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let nomocks_done = done(nomocks);

// ---- Phase: Verify ----------------------------------------------------------
phase("Verify");
let p = "";
p += "Self-evaluate the goble rework at /Users/adrian.tucicovencogmail.com/Projects/goble. Read files to confirm the changes are present; do not edit. ";
p += "Run and capture the tail of each: `cargo check --workspace --all-targets 2>&1 | tail -40` and `cargo test -p goble-ui -p goble-app -p goble-desktop-service 2>&1 | grep -E 'test result:|FAILED|error\\[' | sort | uniq -c`. ";
p += "Report checks_pass = (cargo check exit 0), tests_pass = (no FAILED/error in the test output), and results = a terse bullet list of (a) the cargo check outcome, (b) the per-crate test counts, (c) any warnings about unused code or leftover references to removed symbols. ";
p += "Do not modify anything.";
let verify = agent(p, #{
    label: "verify",
    phase: "verify",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: verify_schema,
});

let checks_pass = is_ok(verify) && verify.output.checks_pass == true;
let tests_pass = is_ok(verify) && verify.output.tests_pass == true;

let features = [];
features.push(#{ "feature": "rename-new-conversation", "done": rename_done });
features.push(#{ "feature": "input-default-terminal", "done": input_done });
features.push(#{ "feature": "input-parser", "done": input_done });
features.push(#{ "feature": "cmdenter-new-conversation", "done": input_done });
features.push(#{ "feature": "bang-terminal", "done": input_done });
features.push(#{ "feature": "nomodel-text-reply", "done": nomodel_done });
features.push(#{ "feature": "topbar-pages", "done": pages_done });
features.push(#{ "feature": "real-workflows", "done": pages_done });
features.push(#{ "feature": "real-tasks", "done": pages_done });
features.push(#{ "feature": "real-timeline", "done": pages_done });
features.push(#{ "feature": "real-costs", "done": pages_done });
features.push(#{ "feature": "real-mcp", "done": pages_done });
features.push(#{ "feature": "no-mock-media", "done": nomocks_done });
features.push(#{ "feature": "no-mock-projects", "done": nomocks_done });
features.push(#{ "feature": "no-mock-screen", "done": nomocks_done });
features.push(#{ "feature": "no-mock-daemon", "done": nomocks_done });

let report = #{
    checks_pass: checks_pass,
    tests_pass: tests_pass,
    features: features,
    rename: if is_ok(rename) { rename.output } else { () },
    input: if is_ok(input) { input.output } else { () },
    nomodel: if is_ok(nomodel) { nomodel.output } else { () },
    pages: if is_ok(pages) { pages.output } else { () },
    nomocks: if is_ok(nomocks) { nomocks.output } else { () },
    verify: if is_ok(verify) { verify.output } else { () },
};

let text = "goble real-input rework — self-evaluation\n";
text += "cargo check --workspace --all-targets: " + (if checks_pass { "PASS" } else { "FAIL" }) + "\n";
text += "tests (goble-ui/app/desktop-service): " + (if tests_pass { "PASS" } else { "FAIL" }) + "\n\n";
text += "Features (done = owner phase reported implemented & green):\n";
for f in features {
    text += "  - " + f.feature + ": " + (if f.done { "done" } else { "open" }) + "\n";
}

complete(#{
    checks_pass: checks_pass,
    tests_pass: tests_pass,
    report: text,
    features: features,
});
