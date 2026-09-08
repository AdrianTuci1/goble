// Orchestrates five sequential implementation phases for the goble "reversible
// substrate" desktop app. Each phase is one impl agent + one verify agent; the
// run never hard-stops so partial delivery is preserved and failures can be
// fixed afterward. Reversibility stays agent-controlled (no user-facing rewind).

let meta = #{
    name: "agent-substrate-run",
    description: "Independent panes, real PTY terminal + Cmd+Enter harness routing, medium->project->session tree, section-D UX, and macOS seatbelt sandbox.",
    phases: [
        #{ title: "Independent panes", detail: "each pane = own conversation + per-session path" },
        #{ title: "Real terminal", detail: "PTY pane, Cmd+Enter routes to harness, else shell" },
        #{ title: "Medium tree", detail: "medium -> project -> session selector" },
        #{ title: "Section-D UX", detail: "keyboard nav, command palette, a11y, onboarding, screen replay, hardening" },
        #{ title: "macOS sandbox", detail: "real seatbelt backend" },
    ],
};

let verify_schema = #{
    "type": "object",
    "required": ["success"],
    "properties": #{
        "success": #{ "type": "boolean" },
        "notes": #{ "type": "string" },
    },
};

let workspace = "/Users/adrian.tucicovencogmail.com/Projects/goble";

fn ok(r) {
    r != () && r.success
}

fn ver_ok(v) {
    if v == () { false }
    else if v.success != true { false }
    else if type_of(v.output) != "map" { false }
    else { v.output.success == true }
}

// ---------------------------------------------------------------------------
// Phase 1 — independent panes + per-session path
// ---------------------------------------------------------------------------
phase("Independent panes");
let p_impl = "Goal: make each chat pane an INDEPENDENT session/conversation in the Rust app at ";
p_impl += workspace;
p_impl += ".\n\nFiles to read first (use read_file/grep before editing):\n";
p_impl += "- app/src/ui/mod.rs  : PaneKind { Chat, Terminal }, Pane tree, Space, UiSnapshot.spaces/active_pane_id.\n";
p_impl += "- app/src/state.rs   : UiState holds GLOBAL selected_id, chat_messages, composer_draft, composer_path; every chat leaf currently renders the SAME global transcript.\n";
p_impl += "- app/src/ui/panes.rs: build_leaf calls chat::build_agent_chat(app, state, actions) for every Chat pane.\n";
p_impl += "- app/src/ui/chat.rs : renders state.chat_messages (global).\n";
p_impl += "- app/src/actions.rs : on_send_message, on_split_right/down, on_add_space, on_pane_activate, on_close_pane all mutate global UiState.\n";
p_impl += "- app/src/root_view.rs: builds snapshots + dispatches global keyboard shortcuts.\n";
p_impl += "- crates/goble-harness-types: existing MediumId/MediumKind types for scoping a turn.\n";
p_impl += "\nRequired changes:\n";
p_impl += "1. Give each Chat leaf pane its own conversation identity + per-pane composer draft + per-pane cwd. Prefer extending the in-memory pane model (a PaneSession keyed by pane id, or extend PaneKind::Chat to carry a conversation id) rather than replacing the tree layout logic.\n";
p_impl += "2. On pane creation (add space, split), create OR select a DISTINCT conversation for the new pane so two panes never share a transcript. Bind each pane's conversation id.\n";
p_impl += "3. chat::build_agent_chat must render the ACTIVE pane's own transcript + its own composer draft + its own path, not the globals. on_send_message must send to the active pane's bound conversation id (routed through crate::runtime::run_turn).\n";
p_impl += "4. Per-session path: composer_path must become per-pane/per-session (each pane has its own cwd). Initialize each new pane's cwd from its medium/project directory when known, else a sane default. Changing medium/project in the selector updates the active pane's path label and the cwd the harness runs in.\n";
p_impl += "5. Extend PersistedPaneState (app/src/state.rs save_panes/restore_panes) so per-pane session data (conversation id, draft, cwd) survives an app restart; restore it on load. Keep split/close/drag/persistence working.\n";
p_impl += "6. Do NOT add any user-facing rewind/replay/fork/checkpoint/jump-to-prior-state UI. Reversibility is agent-controlled and stays in the backend.\n";
p_impl += "\nConstraints:\n";
p_impl += "- The app crate (app/) must not newly depend on goble-ui/goble-core beyond existing adapter boundaries (goble-ui elements, goble_desktop_service DesktopState, goble-harness-types are already imported).\n";
p_impl += "- After editing run `cargo check -p goble-app` and `cargo check --workspace --all-targets`; fix every error you introduced. The workspace must stay green.\n";
p_impl += "- Update/add unit tests where behavior changes (app/src/ui/mod.rs pane tests, app/src/state.rs, app/src/actions.rs) so independent panes + per-pane path are covered.\n";
p_impl += "- Verify with `cargo test -p goble-app` at the end. Report what you changed and any behavior now intentionally different.";

let p1 = agent(p_impl,
    #{ label: "impl:independent-panes", capability_mode: "all" });
if !ok(p1) { log("phase 1 impl FAILED"); complete(#{ done: false, phase: 1, error: "independent-panes impl failed" }); }

let v_impl = "Verify the 'independent panes' implementation in ";
v_impl += workspace;
v_impl += ".\nRun `cargo check --workspace --all-targets` and `cargo test -p goble-app`. Then confirm by reading code that:\n";
v_impl += "- each Chat leaf owns a distinct conversation id / transcript (no two chat panes render the same global chat_messages);\n";
v_impl += "- on_send_message routes the turn to the ACTIVE pane's bound conversation, not a single global selected_id;\n";
v_impl += "- composer draft + path are per-pane (per-session cwd), and the composer path reflects the active pane's cwd;\n";
v_impl += "- PersistedPaneState now captures per-pane session data and restore_panes rebuilds the independent panes.\n";
v_impl += "Set success=true ONLY if compile is clean, goble-app tests pass, and all four points hold. Otherwise success=false with notes explaining the gap.";

let v1 = agent(v_impl,
    #{ label: "verify:independent-panes", capability_mode: "all", output_schema: verify_schema });
if !ver_ok(v1) { complete(#{ done: false, phase: 1, error: "verify independent-panes failed" }); }
log("phase 1 done");

// ---------------------------------------------------------------------------
// Phase 2 — real PTY terminal + Cmd+Enter harness routing + per-session cwd
// ---------------------------------------------------------------------------
phase("Real terminal");
let t_impl = "Goal: turn the Terminal pane placeholder into a REAL working shell in the Rust app at ";
t_impl += workspace;
t_impl += ".\n\nRead first:\n";
t_impl += "- app/src/ui/panes.rs  : build_terminal_placeholder() returns a static text leaf; build_leaf/PaneKind::Terminal.\n";
t_impl += "- crates/goble-ui/src/elements/chat_composer.rs: how the chat composer captures text + submits.\n";
t_impl += "- app/src/root_view.rs  : dispatch_event handles global keydown (ctrl/cmd+space split, ctrl/cmd+w close); Cmd+Enter is NOT yet bound.\n";
t_impl += "- app/src/actions.rs, app/src/state.rs, app/src/runtime.rs: how a turn is started (crate::runtime::run_turn) and how paths/medium/project scope a turn.\n";
t_impl += "- crates/goble-ui: Element trait, event.rs, platform/window.rs (none of these may block).\n";
t_impl += "- Check the Cargo.toml of the crate where you add the PTY dependency; do NOT reuse blocking calls on the UI thread.\n";
t_impl += "\nRequirements:\n";
t_impl += "1. Implement a real PTY-backed terminal pane. Add a PTY dependency (prefer `portable-pty`, or `nix`+`openpty`) to a suitable crate, spawn /bin/zsh (or $SHELL) in the pane's cwd, and render its output (ANSI-aware-enough) as a scrolling line buffer in the pane. Written input goes to the PTY.\n";
t_impl += "2. The terminal runs in the pane's per-session cwd (the same per-session path from the independent-panes work). Spawn the shell in that cwd.\n";
t_impl += "3. Cmd+Enter (command/ctrl + Return) in the terminal must route the current input through the DEFAULT harness (an agent turn) instead of executing it as a shell command. Plain Enter executes it as a normal shell command. Implement this key distinction.\n";
t_impl += "4. The Cmd+Enter harness routing uses the active session's conversation id + medium_id/project_id (mirror how on_send_message -> run_turn works for chat) so the agent turn lands on the right conversation and project.\n";
t_impl += "5. Wire the terminal into the pane lifecycle so it splits/closes/persists like any pane, and reads PTY output non-blocking.\n";
t_impl += "6. Do NOT add user-facing rewind/replay/checkpoint UI.\n";
t_impl += "\nConstraints:\n";
t_impl += "- Do not block the UI/window thread; read PTY output asynchronously and coalesce into frames.\n";
t_impl += "- Respect crate dependency rules (crates that must not depend on goble-ui/goble-core stay that way).\n";
t_impl += "- After editing run `cargo check --workspace --all-targets` and `cargo test -p goble-app`; fix every new error. Add tests where feasible (PTY output line splitting, Enter vs Cmd+Enter routing, cwd selection) without requiring a real terminal in CI.\n";
t_impl += "- In your report name the crate that got the PTY dependency and explain how Cmd+Enter vs Enter is wired.";

let p2 = agent(t_impl,
    #{ label: "impl:real-terminal", capability_mode: "all" });
if !ok(p2) { log("phase 2 impl FAILED"); complete(#{ done: false, phase: 2, error: "real-terminal impl failed" }); }

let v_impl2 = "Verify the 'real PTY terminal + Cmd+Enter' work in ";
v_impl2 += workspace;
v_impl2 += ".\nRun `cargo check --workspace --all-targets` and `cargo test -p goble-app`. Confirm by reading code that:\n";
v_impl2 += "- Terminal pane no longer renders a static placeholder; it spawns a real PTY shell (PTY dependency + shell spawn + read/write plumbing);\n";
v_impl2 += "- the shell starts in the session's per-pane cwd;\n";
v_impl2 += "- Cmd+Enter routes the line through run_turn/harness while plain Enter sends it to the PTY as a shell command;\n";
v_impl2 += "- the PTY does not block the UI thread.\n";
v_impl2 += "Set success=true ONLY if compile is clean and all four hold. Else success=false with notes.";

let v2 = agent(v_impl2,
    #{ label: "verify:real-terminal", capability_mode: "all", output_schema: verify_schema });
if !ver_ok(v2) { complete(#{ done: false, phase: 2, error: "verify real-terminal failed" }); }
log("phase 2 done");

// ---------------------------------------------------------------------------
// Phase 3 — medium -> project -> session tree selector
// ---------------------------------------------------------------------------
phase("Medium tree");
let m_impl = "Goal: make the environment/dev-medium selector a full hierarchical TREE (medium -> project -> session) in the Rust app at ";
m_impl += workspace;
m_impl += ".\n\nRead first:\n";
m_impl += "- app/src/media/state.rs: MediaState holds a FLAT `mediums` list (kind, id, label, project_id) + selected_medium; the flat model to replace/extend.\n";
m_impl += "- app/src/ui/media.rs: build_medium_selector renders flat vertical chips; MediumEntry has id/label/kind/project_id.\n";
m_impl += "- app/src/ui/mod.rs: MediumEntry, MediaSnapshot, MediaActions.\n";
m_impl += "- app/src/ui/shell.rs: where the medium selector is placed (sidebar).\n";
m_impl += "- crates/goble-harness-types: MediumKind, MediumId, and any session/project types.\n";
m_impl += "- app/src/actions.rs + root_view.rs: where selected medium/project are read for run_turn.\n";
m_impl += "\nRequirements:\n";
m_impl += "1. Replace the flat chip list with a collapsible TREE: top-level medium nodes (Local, VM, Remote xrdp, Browser, Container), each expandable to its PROJECTS, each project expandable to its SESSIONS.\n";
m_impl += "2. Represent the hierarchy in app/src/media/state.rs (medium -> list of projects -> list of sessions). Sessions/projects should come from real backend data (DesktopState list_chats / session store per project) when available, with a deterministic fallback only when the store is absent.\n";
m_impl += "3. Selecting a leaf (a session) sets active medium+project+session for the next turn. Selecting a branch expands/collapses. Keep the selected path highlighted.\n";
m_impl += "4. run_turn must receive medium_id, project_id, AND session id from the tree selection (update on_send_message / on_send_queued in app/src/actions.rs and root_view.rs to also read the selected session).\n";
m_impl += "5. Choosing a session updates the active pane's cwd to that session's path (aligns with the independent-panes work).\n";
m_impl += "6. No user-facing rewind/replay/checkpoint UI.\n";
m_impl += "\nConstraints:\n";
m_impl += "- Read real backend sources for sessions/projects (DesktopState) instead of hardcoding; fall back to a small deterministic mock only when the store is absent.\n";
m_impl += "- After editing run `cargo check --workspace --all-targets` and `cargo test -p goble-app`; fix new errors. Add unit tests for tree expand/collapse and selection propagation.\n";
m_impl += "- Report the tree state shape and how the three-level selection reaches run_turn.";

let p3 = agent(m_impl,
    #{ label: "impl:medium-tree", capability_mode: "all" });
if !ok(p3) { log("phase 3 impl FAILED"); complete(#{ done: false, phase: 3, error: "medium-tree impl failed" }); }

let v_impl3 = "Verify the 'medium->project->session tree selector' in ";
v_impl3 += workspace;
v_impl3 += ".\nRun `cargo check --workspace --all-targets` and `cargo test -p goble-app`. Confirm by reading code that:\n";
v_impl3 += "- the medium selector is a collapsible tree (medium -> projects -> sessions), not a flat chip list;\n";
v_impl3 += "- sessions/projects come from real backend data where available;\n";
v_impl3 += "- selecting a session propagates medium+project+session into run_turn (medium_id, project_id, session id);\n";
v_impl3 += "- selecting a session updates the active pane's cwd.\n";
v_impl3 += "Set success=true ONLY if compile clean, goble-app tests pass, and all four hold. Else success=false with notes.";

let v3 = agent(v_impl3,
    #{ label: "verify:medium-tree", capability_mode: "all", output_schema: verify_schema });
if !ver_ok(v3) { complete(#{ done: false, phase: 3, error: "verify medium-tree failed" }); }
log("phase 3 done");

// ---------------------------------------------------------------------------
// Phase 4 — section D: keyboard nav, command palette, a11y, onboarding, screen reuse, hardening
// ---------------------------------------------------------------------------
phase("Section-D UX");
let d_impl = "Goal: implement the 'section D' polish/hardening set in the Rust app at ";
d_impl += workspace;
d_impl += ".\n\nRead first:\n";
d_impl += "- app/src/root_view.rs dispatch_event (global keyboard shortcuts) + crates/goble-ui/src/platform/window.rs + crates/goble-ui/src/event.rs (Modifiers, KeyDown).\n";
d_impl += "- app/src/ui/panes.rs, app/src/ui/mod.rs (Space/Pane/SplitNode), app/src/state.rs + actions.rs.\n";
d_impl += "- app/src/ui/shell.rs, app/src/ui/sidebar.rs (topbar + sidebar).\n";
d_impl += "- app/src/ui/chat.rs (onboarding banners show_llm_key_banner/show_workspace_choice), app/src/screen/* + app/src/ui/screen.rs (existing screen/broadcast/computer-use sheet for record/replay reference).\n";
d_impl += "- crates/goble-ui theme/color tokens + existing focus/hover patterns.\n";
d_impl += "\nImplement ALL of the following, scoped and real:\n";
d_impl += "1. PANE KEYBOARD NAVIGATION: move focus between panes with Ctrl/Cmd+Arrow keys (up/down/left/right) in the active space; the active pane highlight follows focus. Honor existing split/close shortcuts. Add a small shortcut hint.\n";
d_impl += "2. COMMAND PALETTE: a Cmd+K overlay listing available commands (new space, split right/down, close pane, switch space, new conversation, open settings/projects, key toggles). Keyboard-scrollable, enterable; wire each to existing UiActions; render as an overlay dialog.\n";
d_impl += "3. ACCESSIBILITY: ensure interactive elements have a keyboard/activation path (tab/enter), a visible focus ring/visual, and sensible labels/tooltips. For a native app, focus on consistent keyboard focus + focus visuals.\n";
d_impl += "4. ONBOARDING: complete the first-run flow end to end (model key banner -> local/remote choice -> a short tip surface), make it dismissible without a dead end, and skip it on a returning run. Keep existing banners; add a compact getting-started hint.\n";
d_impl += "5. SCREEN-EVENT RECORD/REPLAY: use the existing screen registry/controller (goble_screen_core + app/src/screen) to record input/observation events and replay them deterministically (record start/stop, replay once/loop, timing). This ties into the reversible substrate WITHOUT being a conversation rewind.\n";
d_impl += "6. HARDENING: handle empty states (no conversations/sessions/projects), guard index/name overflows, clamp pane-ratio and sidebar-width inputs, surface errors readably, and fix obvious unwraps/panics on user-controlled state.\n";
d_impl += "\nConstraints:\n";
d_impl += "- Conversation rewind/replay/fork/checkpoint stays agent-controlled: do NOT add user-facing controls for the conversation. (Screen-input record/replay for the screen surface is a distinct, allowed feature.)\n";
d_impl += "- After editing run `cargo check --workspace --all-targets` and `cargo test -p goble-app`; fix new errors. Add tests for keyboard focus traversal, palette command routing, onboarding skip on return, and record/replay scheduling where feasible.\n";
d_impl += "- Do not regress existing first-run/login flows.\n";
d_impl += "- Report which sub-items landed and which were deferred; be explicit about screen record/replay vs conversation rewind.";

let p4 = agent(d_impl,
    #{ label: "impl:section-d", capability_mode: "all" });
if !ok(p4) { log("phase 4 impl FAILED"); complete(#{ done: false, phase: 4, error: "section-d impl failed" }); }

let v_impl4 = "Verify the 'section D' set in ";
v_impl4 += workspace;
v_impl4 += ".\nRun `cargo check --workspace --all-targets` and `cargo test -p goble-app`. Confirm by reading code that each landed (note any deferred):\n";
v_impl4 += "1. pane keyboard nav via Ctrl/Cmd+Arrow moves active pane focus;\n";
v_impl4 += "2. a Cmd+K command palette overlay routes commands to UiActions;\n";
v_impl4 += "3. keyboard focus + visible focus ring + labels present on interactive elements;\n";
v_impl4 += "4. onboarding completes and skips on return without a dead end;\n";
v_impl4 += "5. screen record/replay controls wired against the screen registry/controller (NOT conversation rewind);\n";
v_impl4 += "6. hardening (empty states, clamped ratio/sidebar, no obvious panics on user state).\n";
v_impl4 += "Set success=true ONLY if compile is clean, goble-app tests pass, and items 1,2,4,6 are genuinely present. success=false with notes if a core item is missing.";

let v4 = agent(v_impl4,
    #{ label: "verify:section-d", capability_mode: "all", output_schema: verify_schema });
if !ver_ok(v4) { complete(#{ done: false, phase: 4, error: "verify section-d failed" }); }
log("phase 4 done");

// ---------------------------------------------------------------------------
// Phase 5 — real macOS Seatbelt sandbox
// ---------------------------------------------------------------------------
phase("macOS sandbox");
let s_impl = "Goal: make the Apple Seatbelt sandbox backend REAL on macOS in the Rust workspace at ";
s_impl += workspace;
s_impl += ".\n\nRead first:\n";
s_impl += "- crates/goble-sandbox/src/backends/seatbelt.rs: currently a stub returning BackendUnavailable for Hardened.\n";
s_impl += "- crates/goble-sandbox/src/backends/mod.rs, crates/goble-sandbox/src/lib.rs: Sandbox trait, SandboxProfile, SandboxLevel, allowed(), PreparedCommand, harness_sandbox()/harness_sandbox_profile() selection, and the macOS path quirk (/var -> /private/var).\n";
s_impl += "- crates/goble-sandbox/src/backends/{bwrap,landlock,seccomp}.rs for backend conventions.\n";
s_impl += "- crates/goble-sandbox/Cargo.toml (target-specific deps; likely already cfg-gated per OS).\n";
s_impl += "- crates/goble-core/src/harness.rs (how harness_sandbox is composed/opted-in).\n";
s_impl += "\nRequirements:\n";
s_impl += "1. Implement a REAL Seatbelt sandbox on macOS via Apple's sandbox framework: call `sandbox_init`/`sandbox_compile_string` + `sandbox_apply` through FFI against libsandbox, building an `.sb` profile string from the SandboxProfile (allow-file-read*, allow-file-write* on allowed paths, deny others, deny network unless allowed). Gate on `#[cfg(target_os = \"macos\")]`.\n";
s_impl += "2. On non-macOS the Seatbelt backend stays as before (unavailable). On macOS, prepare() must succeed ONLY when the sandbox can actually be applied; surface sandbox_init failure as a clear error, never silently pretend to harden. This lets harness_sandbox() return Some on macOS.\n";
s_impl += "3. Keep read_only_fs=false semantics from harness_sandbox_profile() so write_file/edit_file/delete_file/git_commit keep working through the profile's allowed write paths.\n";
s_impl += "4. Do NOT weaken the Runner's allowed_commands allow-list gate in crates/goblin-worker/src/runner.rs and the composition roots (crates/goble-desktop-service/src/state.rs, crates/goble-desktop-native/src/state_api.rs, crates/goble-desktop/src-tauri/src/lib.rs) that call `.with_sandbox(harness_sandbox())`.\n";
s_impl += "5. Make the sandbox actually constrain the process: compile a nontrivial profile (allow-file-read* on the repo, deny other file writes, deny network by default, allow-process only where needed). If sandbox_apply cannot run in this environment, the code path must still be correct and testable via unit tests on profile compilation and the allow/deny decision table.\n";
s_impl += "\nConstraints:\n";
s_impl += "- The goble-sandbox crate stays acyclic (no dep on goble-core/goble-ui/app). It may use libc/nix FFI + `unsafe` only behind macos gates.\n";
s_impl += "- After editing run `cargo check --workspace --all-targets` and `cargo test -p goble-sandbox`; fix new errors. Add tests: hardened profile builder, allow/deny decisions, and that a Hardened profile is not reported as hardened when the sandbox cannot be applied.\n";
s_impl += "- This machine is macOS; if sandbox_init cannot be exercised in the harness, gate the integration test behind a probe and always exercise the decision table + profile builder.\n";
s_impl += "- Report how the FFI is declared (link name, unsafe blocks), the generated .sb profile shape, and whether harness_sandbox() now returns Some on macOS.";

let p5 = agent(s_impl,
    #{ label: "impl:macos-seatbelt", capability_mode: "all" });
if !ok(p5) { log("phase 5 impl FAILED"); complete(#{ done: false, phase: 5, error: "macos-seatbelt impl failed" }); }

let v_impl5 = "Verify the real macOS Seatbelt sandbox in ";
v_impl5 += workspace;
v_impl5 += ".\nRun `cargo check --workspace --all-targets` and `cargo test -p goble-sandbox`. Confirm by reading code that:\n";
v_impl5 += "- seatbelt.rs calls the Apple sandbox framework (sandbox_init/compile_string/apply) via FFI behind #[cfg(target_os=\"macos\")];\n";
v_impl5 += "- a hardened profile builds a nontrivial .sb string (allow-file-read*, deny write elsewhere, deny network by default);\n";
v_impl5 += "- prepare() returns success ONLY when the sandbox can actually be applied; sandbox_init failure is surfaced, not swallowed;\n";
v_impl5 += "- harness_sandbox() can now return Some on macOS;\n";
v_impl5 += "- the crate stays acyclic and its tests pass.\n";
v_impl5 += "Set success=true ONLY if compile clean, goble-sandbox tests pass, and the FFI + true hardening path is present. Else success=false with notes.";

let v5 = agent(v_impl5,
    #{ label: "verify:macos-seatbelt", capability_mode: "all", output_schema: verify_schema });
if !ver_ok(v5) { complete(#{ done: false, phase: 5, error: "verify macos-seatbelt failed" }); }
log("phase 5 done");

complete(#{ done: true, phases: [1, 2, 3, 4, 5] });
