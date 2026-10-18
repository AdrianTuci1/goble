// Terminal-first adaptation: remove the remote-desktop surface and add the
// window-manager capabilities that tuios has, while keeping goble's moats.
//
// The run happens inside a Modal sandbox. The repo is at /work/goble on branch
// `terminal-first`; each wave lands as one local commit. Nothing is pushed.
//
// House rule for every agent: never build or run the GUI binary. Use
// `cargo check`, `cargo test`, `cargo clippy` and `cargo fmt`.

let meta = #{
    name: "goble-terminal-first",
    description: "Remove xrdp, add daemon-owned PTY, per-pane agent state, an inbox, a control plane, copy/search, and terminal-window-manager layouts",
    when_to_use: "Run inside the Modal harness when advancing goble's terminal-first product direction.",
    phases: [
        #{ title: "F0 cut xrdp", detail: "remove the remote-desktop product surface" },
        #{ title: "F1a PTY daemon", detail: "the attachable PTY daemon crate" },
        #{ title: "F1b app on the daemon", detail: "app-side attach, reattach, detach and kill" },
        #{ title: "F2 state + inbox", detail: "per-pane agent state and a global inbox" },
        #{ title: "F3a control protocol", detail: "control socket served by the app" },
        #{ title: "F3b CLI + MCP", detail: "the CLI and MCP server surfaces" },
        #{ title: "F4 copy + search", detail: "copy mode, selection, scrollback search" },
        #{ title: "F5a layouts + SSH", detail: "master-stack, floating, arbitrary SSH panes" },
        #{ title: "F5b multi-client + images", detail: "two clients, kitty/sixel parsers and rendering" },
        #{ title: "Final", detail: "adversarial verification and the patch" },
    ],
};

fn repo() { "/work/goble" }
fn out() { "/work/out" }

fn impl_schema() {
    #{
        "type": "object",
        "required": ["committed", "summary", "checks"],
        "properties": #{
            "committed": #{ "type": "boolean" },
            "summary": #{ "type": "string" },
            "checks": #{ "type": "array", "items": #{ "type": "string" } },
            "files": #{ "type": "array", "items": #{ "type": "string" } },
            "left": #{ "type": "string" },
        },
    }
}

fn verdict_schema() {
    #{
        "type": "object",
        "required": ["passed", "failures", "evidence"],
        "properties": #{
            "passed": #{ "type": "boolean" },
            "failures": #{ "type": "array", "items": #{ "type": "string" } },
            "evidence": #{ "type": "string" },
        },
    }
}

fn house() {
    let h = "";
    h += "Repository: " + repo() + " (branch `terminal-first`). Work only there. ";
    h += "This is a Rust workspace; the app is a native wgpu/winit GUI but you must NEVER run `cargo run`, `cargo build`, `cargo install`, or launch any GUI binary. ";
    h += "Use `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt` and `rg` only. ";
    h += "Do not push, do not change git remotes, do not touch `origin`. Commit locally. ";
    h += "Read the relevant `.agents/` docs and the existing code before changing behavior. ";
    h += "Keep the workspace rule `unsafe_code = deny`; do not add `unsafe`, `#[allow(dead_code)]`, or stubs that silently do nothing. ";
    h += "Keep goble's moats intact: real command blocks (OSC 133/DCS hooks), the harness claim protocol that runs agent commands in the user's own shell, transcripts/tool calls/subagents, and reversible execution (`goble-replay` fork/replay/revert). ";
    h += "Make exactly one commit for the wave. If you are repairing, amend that same commit (`git commit --amend --no-edit`) so the wave stays one commit. ";
    h += "In your final answer use the JSON schema you were given; report the exact checks you ran and anything left undone.";
    h
}

fn impl_prompt(wave, body) {
    let p = house();
    p += "\n\nYou are the implementer for wave " + wave + ".\n\n";
    p += body;
    p += "\n\nDefinition of done for this turn: the code compiles with `cargo check` for the crates you touched, the wave's tests pass, and you have committed the result. If a check is red, fix it before committing; if something is genuinely impossible in this environment, say so in `left` and commit the best coherent state.";
    p
}

fn verify_prompt(wave, criteria) {
    let p = house();
    p += "\n\nYou are an independent, adversarial verifier for wave " + wave + ". You did not write this code. ";
    p += "Inspect the actual diff (`git -C " + repo() + " show --stat HEAD` and `git -C " + repo() + " show HEAD`) and the files it touches. ";
    p += "Run the acceptance checks yourself; do not trust the implementer's report. ";
    p += "A check that you could not run is a failure, not a pass. ";
    p += "Report `passed: true` only when every acceptance criterion is met with concrete evidence. Otherwise list each concrete failure in `failures` and put commands/output in `evidence`.\n\n";
    p += "Acceptance criteria:\n" + criteria;
    p
}

fn passed(v) {
    v != () && v.success && v.output != () && v.output.passed == true
}

fn failures_text(v) {
    let text = "";
    if v == () || !v.success || v.output == () {
        return "the verifier returned no usable verdict";
    }
    if v.output.failures != () {
        for f in v.output.failures {
            text += "- " + f + "\n";
        }
    }
    if text == "" && v.output.evidence != () {
        text = v.output.evidence;
    }
    if text == "" { text = "the verifier failed without details"; }
    text
}

fn report(label, r) {
    if r == () {
        log(label + ": no result");
        return;
    }
    if !r.success {
        log(label + ": agent failed");
        return;
    }
    if r.output != () && r.output.summary != () {
        log(label + ": " + r.output.summary);
    } else {
        log(label + ": done");
    }
}

// One implement -> verify -> one repair -> re-verify cycle. Returns true when
// the second (or first) verdict passes. A failed wave does not stop the run:
// later waves still land and the final report names what did not.
fn wave(wave_name, title, impl_body, criteria) {
    phase(title);
    let r = agent(impl_prompt(wave_name, impl_body),
        #{ label: "impl:" + wave_name, capability_mode: "all", output_schema: impl_schema() });
    report("impl " + wave_name, r);

    let v = agent(verify_prompt(wave_name, criteria),
        #{ label: "verify:" + wave_name, capability_mode: "execute", output_schema: verdict_schema() });
    if passed(v) {
        log(wave_name + ": verified");
        return true;
    }

    let failures = failures_text(v);
    log(wave_name + ": verification failed; repairing once");
    let repair = "";
    repair += "The independent verifier rejected wave " + wave_name + " with these concrete failures:\n\n" + failures + "\n\n";
    repair += "Fix the cause of every failure. Re-read the actual code; do not paper over a failure by deleting a test unless the test itself is wrong and you explain why. ";
    repair += "Amend the wave commit when done (`git commit --amend --no-edit`).";
    let rr = agent(impl_prompt(wave_name + " repair", repair),
        #{ label: "repair:" + wave_name, capability_mode: "all", output_schema: impl_schema() });
    report("repair " + wave_name, rr);

    let v2 = agent(verify_prompt(wave_name + " after repair", criteria),
        #{ label: "reverify:" + wave_name, capability_mode: "execute", output_schema: verdict_schema() });
    if passed(v2) {
        log(wave_name + ": verified after repair");
        return true;
    }
    log(wave_name + ": STILL FAILING\n" + failures_text(v2));
    false
}

fn main() {
    log("goble-terminal-first: repo " + repo());

    // ------------------------------------------------------------------ F0
    let f0 = "";
    f0 += "Remove the xrdp/remote-desktop product surface entirely. Goble becomes a terminal window manager with the harness integrated; the remote desktop was the wrong product.\n\n";
    f0 += "Do all of this:\n";
    f0 += "- Delete the crates `crates/goble-screen-core`, `crates/goble-screen-adapter`, `crates/goble-screen-sdk` (including `tests/rdp_e2e.rs`). They are workspace members through `crates/*`.\n";
    f0 += "- In `app/`: delete `src/screen/`; remove the `screen` module and every import. Remove screen state from `app/src/state/ui_state/screen.rs`, `app/src/state/types.rs`, `app/src/ui/screen.rs`, `app/src/ui/snapshot.rs`, `app/src/actions/make_actions.rs`, `app/src/actions/pane_ops.rs`, `app/src/root_view/events.rs`, `app/src/root_view/rebuild.rs`. Delete the screen tests (`app/src/integration_testing/screen_flow.rs`, `app/src/actions/screen_close_tests.rs`, `app/src/root_view/tests/screen_close.rs`, `app/src/root_view/tests/screen_handoff.rs`, `app/src/state/screen_link_tests.rs`) and their `[[test]]` entries in `app/Cargo.toml`.\n";
    f0 += "- Remove `DesktopState::{open,close}_remote_screen` and any `ScreenLink`/screen handoff fields from `crates/goble-desktop-service`; remove the `remote-screen` and `rdp` features and the screen SDK dependency from `crates/goble-desktop-service/Cargo.toml`, `crates/goble-desktop-native/Cargo.toml`, and the workspace dependency list if present.\n";
    f0 += "- Remove the `open_screen` harness tool and `ScreenHandoff`/`OPEN_SCREEN_TOOL` from `crates/goble-core/src/harness/{definitions,dispatch,handoff}.rs`, `crates/goble-harness-protocol/src/lib.rs`, `crates/goble-harness-internal/src/lib.rs`, `crates/goble-harness-types/src/lib.rs`, and their tests. Agents keep the shell, MCP, SSH and worker tools.\n";
    f0 += "- Remove the remote-desktop install path: `install_remote_desktop`, `REMOTE_DESKTOP_STEP`, xrdp/xorgxrdp/xfce4 packages and the `startwm.sh` pin in `crates/goble-core/src/provision.rs`; drop `--remote-desktop` from `crates/goble-cli/src/{cli,commands,worker}.rs` and the worker state. Keep the SSH install, worker install, and `goble-desktop-service` worker flow.\n";
    f0 += "- Remove `FrameView` from `crates/goble-ui` (`elements/frame_view.rs`, exports in `elements.rs`/`lib.rs`) and `inline_screen` from `crates/goble-ui/src/views/chat_view/{mod,builder,transcript,tests}.rs`; delete `app/src/ui/chat/screen`-related code if it only exists for the remote desktop.\n";
    f0 += "- Update `README.md`, `docs/PROJECT_STRUCTURE.md`, `docs/substrate-architecture.md`, `.agents/01-vision/README.md`, `.agents/01-vision/remote-features.md`, `.agents/01-vision/boundaries-and-deferred.md`, `.agents/04-agent-runtime/computer-use.md`, `.agents/TRACKER.md` and `.agents/RESOLVER.md`: remove the xrdp/remote-desktop direction, state the terminal-first direction in a short paragraph, and mark the removed tracker rows as removed. Keep SSH, worker panes and the remote daemon documented.\n";
    f0 += "- Do not remove anything else: SSH pane binding, worker/viewer panes, the remote daemon, blocks, the claim protocol, replay, voice, projects, crons.\n\n";
    f0 += "Acceptance: `rg -n \"xrdp|Xrdp|remote.desktop|RemoteDesktop|remote_desktop|goble.screen|goble-screen|ScreenHandoff|OPEN_SCREEN_TOOL|open_screen|remote-screen|FrameView|inline_screen\" --glob '!target'` returns no live code references; `cargo check --workspace` passes; `cargo test -p goble-core -p goble-desktop-service -p goble-ui` passes. Commit as `feat(terminal-first): remove the remote-desktop surface`.";
    let f0_criteria = "- No live xrdp/screen references remain (`rg` over the tracked tree).\n- The screen crates are gone and no manifest references them.\n- `cargo check --workspace` passes.\n- `cargo test -p goble-core -p goble-desktop-service -p goble-ui` passes.\n- The moats (blocks, claim protocol, transcript/subagents, replay) and SSH remain.";
    wave("F0", "F0 cut xrdp", f0, f0_criteria);

    // ------------------------------------------------------------------ F1
    let f1a = "";
    f1a += "Build the attachable PTY daemon as its own crate. Do not touch the app in this wave; the app integration lands in F1b.\n\n";
    f1a += "- Add `crates/goble-pty` with `src/lib.rs`, `src/protocol.rs`, `src/server.rs`, `src/client.rs`, `src/session.rs`, and a `goble-pty-daemon` binary (`src/bin/goble-pty-daemon.rs` or a `[[bin]]`). It owns `portable-pty`.\n";
    f1a += "- Socket path: `$GOBLE_PTY_SOCKET`, else `$XDG_RUNTIME_DIR/goble/pty.sock`, else `$HOME/.goble/pty.sock`. A pidfile sits next to it.\n";
    f1a += "- Newline-delimited JSON protocol. Requests: `hello {client_id}`, `list`, `create {pane_id,cwd,cols,rows,shell}`, `attach {session_id,from_seq}`, `detach {session_id}`, `input {session_id,bytes_b64}`, `resize {session_id,cols,rows}`, `kill {session_id}`, `signal {session_id,signal}`. Events: `session {session_id,pane_id,alive,cols,rows}`, `output {session_id,seq,bytes_b64}`, `exit {session_id,code}`, `error {message}`. Responses carry a request id.\n";
    f1a += "- The daemon keeps a bounded replay ring per session (at least 256 KiB, byte-exact) and broadcasts output to every attached client. Attach replays from `from_seq`. Multiple clients may attach at once. `detach` never kills; only `kill` (or an explicit close) kills. Raw bytes must pass through untouched so OSC 133/DCS shell-integration parsing still happens in the app.\n";
    f1a += "- Client auto-start: connect; on missing socket or refused connection, spawn the daemon with `--daemon` (setsid/double-fork, stdio to a log), wait for the socket with bounded backoff, then connect. `--foreground` for tests. A stale socket/pidfile must be cleaned safely.\n";
    f1a += "- Tests: protocol encode/decode; an integration test on a temp socket that creates a session, attaches client A, runs `printf`, detaches A, attaches B with `from_seq: 0`, sees the replayed output and the live session, then kills it; a multi-client broadcast test; a stale-socket/auto-start test.\n\n";
    f1a += "Acceptance: `cargo check -p goble-pty`; `cargo test -p goble-pty`; the survival and multi-client tests pass. Commit as `feat(pty): add the attachable PTY daemon`.";
    let f1a_criteria = "- `crates/goble-pty` exists with the protocol, server, client, session and daemon binary.\n- The protocol covers create/attach/detach/input/resize/kill/list and output replay.\n- A test proves a session survives client A detaching and is readable by client B.\n- A test proves two clients receive the same output.\n- `cargo test -p goble-pty` and `cargo check -p goble-pty` pass.";
    wave("F1a", "F1a PTY daemon", f1a, f1a_criteria);

    let f1b = "";
    f1b += "Run terminal panes through the F1a daemon. Do not redesign the daemon's protocol; if it is missing something, add the smallest compatible request/event and its test.\n\n";
    f1b += "- Turn `app/src/terminal/session` into a daemon client adapter. `spawn(cwd)` asks the daemon for a session and attaches; the reader thread feeds output events into the existing `Emulator`; writes/resizes go to the daemon; `Drop` detaches (does not kill). Add an explicit `kill` path for a pane close.\n";
    f1b += "- Remove `portable-pty` from `app/Cargo.toml`; `goble-pty` owns it. Keep `TerminalSession::failed`, the test constructors, and the no-PTY shape used by tests.\n";
    f1b += "- Persist the daemon session id in the pane's persisted state and reattach on app start (`TerminalRegistry::ensure_session` reuses an existing id). A pane whose daemon session is gone gets a fresh session, not a stale id.\n";
    f1b += "- If the daemon is unavailable, show the existing `TerminalSession::failed` error rather than silently spawning a local PTY.\n";
    f1b += "- Keep the shell integration script/bootstrap and the claim protocol working. The app still parses hooks from the replayed bytes; `PaneCommands` still answers tool calls.\n";
    f1b += "- Tests: app-side tests that `ensure_session` reuses a persisted id, that `drop_pane` detaches rather than kills, that a pane close kills, and that shell-integration events still reach the emulator from replayed bytes. Use an in-process daemon on a temp socket where the existing test harness allows it.\n\n";
    f1b += "Acceptance: `cargo check --workspace`; `cargo test -p goble-terminal`; `cargo test -p goble-desktop-service`; `cargo test -p goble-app --lib`. Commit as `feat(pty): run terminal panes through the daemon`.";
    let f1b_criteria = "- The app depends on `goble-pty` and no longer uses `portable-pty` directly.\n- Spawn/attach, input, resize, detach and kill are wired.\n- A persisted session id is reattached on app start; a missing session falls back cleanly.\n- `drop_pane` detaches and the explicit close kills.\n- Shell integration and the claim protocol still work.\n- `cargo check --workspace`, `cargo test -p goble-terminal`, `cargo test -p goble-desktop-service`, `cargo test -p goble-app --lib` pass.";
    wave("F1b", "F1b app on the daemon", f1b, f1b_criteria);

    // ------------------------------------------------------------------ F2
    let f2 = "";
    f2 += "Add a per-pane agent state and a global Inbox for approvals and questions.\n\n";
    f2 += "- Add `PaneAgentState { Unknown, Idle, Working, NeedsInput, Done, Errored }` in `app/src/state/ui_state/agents.rs` (or a small new module) and store it per pane in the pane runtime. Derive it from real events: `begin_turn` -> Working; `apply_command_proposed` and any ask-human/question event -> NeedsInput (find the real event names in `goble-desktop-service`); `finish_turn` -> Done on success and Errored on a failed turn/terminal error; a TUI agent in `TerminalMode::Agent` with a running command -> Working, at a prompt -> Idle. Never fabricate a state an event did not report. `finish_turn` must not erase an approval that is still pending.\n";
    f2 += "- Add `app/src/state/ui_state/inbox.rs` with `InboxItem { id, pane_id, space_id, conversation_id, kind, title, detail, created_at, acknowledged }` and kinds `Approval`, `Question`, `Done`, `Error`, `Agent`. UiState owns `inbox: Vec<InboxItem>` with `push_inbox` (dedupe by id and by conversation+kind), `ack_inbox`, `ack_all`, `unread_inbox_count`, `jump_to_inbox_item` (switch space + focus pane), and pruning when a pane closes. Hook it to command proposed, ask/question, turn finished/failed, subagent finished with failure, and a TUI agent needing input if a real signal exists. Unknown events stay Unknown.\n";
    f2 += "- UI: a topbar inbox control with the unread count and `Cmd+Shift+I`; an overlay listing items (newest first) with `j/k` selection, `Enter` jump, `a` ack, `A` ack all, `Esc` close. Reuse the existing overlay/palette patterns. The count must update live and the overlay must show empty/error states honestly.\n";
    f2 += "- Tests: state transitions for each event, dedupe, ack, jump (space + pane), prune on pane close, and a UI snapshot/build test for the inbox overlay. `cargo test -p goble-app --lib` plus `cargo test -p goble-app --test chat_flow` plus `cargo test -p goble-ui`.\n\n";
    f2 += "Acceptance: the state is event-derived, the Inbox is global and actionable, and the tests prove it. Commit as `feat(agents): per-pane state and a global inbox`.";
    let f2_criteria = "- `PaneAgentState` is per pane and changes only from real events.\n- The Inbox is global, dedupes, acks, prunes, and jumps to the right space and pane.\n- The topbar count and overlay are wired and tested.\n- `cargo test -p goble-app --lib`, `cargo test -p goble-app --test chat_flow`, `cargo test -p goble-ui` pass.\n- No fabricated states or placeholder strings.";
    wave("F2", "F2 state + inbox", f2, f2_criteria);

    // ------------------------------------------------------------------ F3
    let f3a = "";
    f3a += "Build the control protocol and serve it from the app. The CLI and MCP surface land in F3b; keep this wave's scope to the crate plus the app-side server.\n\n";
    f3a += "- Add `crates/goble-control` with typed JSON request/response types, a line-delimited Unix-socket server generic over a handler trait, and a client. Default socket: `$GOBLE_CONTROL_SOCKET`, else `$HOME/.goble/control.sock`.\n";
    f3a += "- Verbs: `panes.list`, `panes.new {space_id?,cwd?,kind?}`, `panes.split {pane_id,axis}`, `panes.close {pane_id}`, `panes.focus {pane_id}`, `panes.send {pane_id,text,enter?}`, `panes.resize {pane_id,cols,rows}`, `spaces.list`, `spaces.switch {space_id}`, `agents.state {pane_id?}`, `agents.prompt {pane_id,text}`, `inbox.list`, `inbox.ack {id?|all}`, `layout.get {space_id}`, `layout.set {space_id,kind}`, `pty.list`, `pty.attach {session_id}`, `pty.detach {session_id}`, `pty.kill {session_id}`. Return structured errors, never panics. Route `pty.*` to the F1 daemon; the rest to the app's UI state.\n";
    f3a += "- Serve the control socket from the app on a background thread. The app owns UI state on the winit thread. Check how `crates/goble-ui/src/platform/window.rs` runs the event loop and add the smallest safe seam (a winit user event, or a channel polled in `about_to_wait`/the root view) with a oneshot reply channel. A command must be safe when the window is closing. If the app is not running, a client reports a clear connection error.\n";
    f3a += "- Tests: codec roundtrip, server/client roundtrip with a mock handler, unknown verb and missing-pane structured errors, and a server test that proves a request receives a reply and the socket is removed/cleaned on shutdown.\n\n";
    f3a += "Acceptance: `cargo check --workspace`; `cargo test -p goble-control`; `cargo test -p goble-app --lib`. Commit as `feat(control): a control protocol served by the app`.";
    let f3a_criteria = "- `crates/goble-control` implements the verbs and a Unix-socket server/client.\n- The app serves the socket and mutates UI state through the event loop; unknown/missing targets return structured errors.\n- `cargo check --workspace`, `cargo test -p goble-control`, `cargo test -p goble-app --lib` pass.";
    wave("F3a", "F3a control protocol", f3a, f3a_criteria);

    let f3b = "";
    f3b += "Add the user-facing control surfaces on top of the F3a protocol: the CLI and the MCP server.\n\n";
    f3b += "- CLI: add `goble ctl <verb> [--json] [--socket PATH]` and subcommands in `crates/goble-cli`; keep existing commands working. Print structured errors on a missing app/socket.\n";
    f3b += "- MCP: add `goble mcp` or a `goble-mcp` binary implementing MCP over stdio JSON-RPC 2.0: `initialize`, `tools/list`, `tools/call`. Tools: `goble_list_panes`, `goble_new_pane`, `goble_split_pane`, `goble_close_pane`, `goble_send_input`, `goble_agent_state`, `goble_inbox`, `goble_ack_inbox`, `goble_switch_space`, `goble_layout`. No external MCP SDK is required; keep it small and tested.\n";
    f3b += "- Document the protocol and the tool names in `docs/control-plane.md` and link from README.\n";
    f3b += "- Tests: CLI parse, MCP `initialize`/`tools/list`/`tools/call` against a mock control client, and an end-to-end test against an in-process control server.\n\n";
    f3b += "Acceptance: `cargo check --workspace`; `cargo test -p goble-cli`; the MCP server test passes. Commit as `feat(control): add the CLI and MCP server`.";
    let f3b_criteria = "- `goble ctl` works and reports connection errors clearly.\n- The MCP server implements initialize/tools/list/tools/call with the named tools.\n- `cargo check --workspace`, `cargo test -p goble-cli` pass.\n- `docs/control-plane.md` documents the protocol and tools.";
    wave("F3b", "F3b CLI + MCP", f3b, f3b_criteria);

    // ------------------------------------------------------------------ F4
    let f4 = "";
    f4 += "Add copy mode, real selection, and scrollback search to the terminal pane.\n\n";
    f4 += "- Scrollback: expose the emulator's history (Alacritty grid scrollback) through `goble-terminal`: history length, a line accessor, and a search over history + the visible screen returning `{line, col, len, text}`. Use the existing `regex` dependency for an optional regex mode; the default is a literal, smart-case search.\n";
    f4 += "- Copy mode: a per-pane copy-mode state with a cursor (line, col), `h/j/k/l` and arrows, `g/G` top/bottom, `0/$` line ends, `v` visual selection, `y` yank and exit, `q`/Esc exit. `/` opens search from copy mode. Keep the terminal's normal input untouched when copy mode is off.\n";
    f4 += "- Selection: mouse drag selects a cell range; double-click selects a word, triple-click the line; `Cmd+C` copies. Draw the selection and search highlights in `app/src/ui/terminal/build.rs` with the existing palette, and auto-scroll to the active match. Use the existing clipboard path.\n";
    f4 += "- Search UI: `Cmd+F` opens a find bar scoped to the focused pane; typing updates matches, Enter/`n` next, `Shift+Enter`/`N` previous, Esc closes. Show `current/total`. Search must work while scrolled back and must not steal keys from a TUI agent unless the find bar is open.\n";
    f4 += "- Tests: selection math (word/line ranges, reverse drag), search across history and screen, smart case, no-match and regex-invalid states, key classification for copy mode and find bar. `cargo test -p goble-terminal` and `cargo test -p goble-app --lib`; `cargo check -p goble-app`.\n\n";
    f4 += "Acceptance: all three features are usable from the keyboard and mouse, and the tests cover the math and the states. Commit as `feat(terminal): copy mode, selection, and scrollback search`.";
    let f4_criteria = "- Scrollback search covers history + screen and returns correct matches.\n- Copy mode supports movement, visual selection, yank, and exit.\n- Mouse selection supports drag, double-click word, triple-click line, and Cmd+C.\n- The find bar shows current/total and handles no-match/invalid-regex.\n- Selection/search highlights are drawn and auto-scroll works.\n- Tests pass: `cargo test -p goble-terminal`, `cargo test -p goble-app --lib`.";
    wave("F4", "F4 copy + search", f4, f4_criteria);

    // ------------------------------------------------------------------ F5
    let f5a = "";
    f5a += "Add the window-manager layouts and arbitrary SSH host panes. Leave multi-client and image protocols to F5b.\n\n";
    f5a += "- Layouts: add `LayoutKind { Bsp, MasterStack, Floating }` to the space model and persist it. BSP is the existing binary split. Master-stack gives the first pane ~60% and stacks the rest. Floating gives every pane a persisted rect with move/resize and z-order (click raises). Add `Cmd+Shift+L` to cycle, a command-palette entry, and a `layout.set` control verb. Recompute on split/close/space switch.\n";
    f5a += "- Arbitrary SSH panes: add a host list to settings (`~/.goble/hosts.toml` or the existing settings config) with `name`, `host`, `user?`, `port?`, `identity?`. Add `Cmd+Shift+N` -> host picker -> new pane running `ssh -t` with the right args, bound through `TerminalRegistry::set_ssh_session` so the existing SSH chip and agent routing keep working. Persist the host on the pane and offer reconnect on exit. Keep the existing local SSH flow.\n";
    f5a += "- Tests: layout math for all three kinds (including split/close), SSH argv construction, host config parse, and a state test for persisting/switching the layout and the host. `cargo test -p goble-app --lib`, `cargo test -p goble-terminal`; `cargo check --workspace`.\n\n";
    f5a += "Acceptance: layouts are switchable and persisted; an SSH host pane can be created and stays bound. Commit as `feat(terminal): layouts and arbitrary SSH hosts`.";
    let f5a_criteria = "- All three layouts are switchable, persisted, and tested.\n- Arbitrary SSH host panes can be created, bound, persisted, and reconnected.\n- `cargo check --workspace`, `cargo test -p goble-app --lib`, `cargo test -p goble-terminal` pass.";
    wave("F5a", "F5a layouts + SSH", f5a, f5a_criteria);

    let f5b = "";
    f5b += "Add multi-client attach and kitty/sixel image support on top of F5a. This is the best-effort wave: land real parsers, cache and tests; state a limitation rather than faking rendering.\n\n";
    f5b += "- Multi-client: make the PTY daemon's attach path usable by more than one app instance and test broadcast + replay with two clients. A second client attaching to the same session must not steal or kill it. If the app already supports one daemon session per pane, add a test-only second client and the daemon-side behavior; a full second GUI instance is not required.\n";
    f5b += "- Kitty/sixel images: parse APC `ESC _ G ... ESC \\` (kitty) and DCS `ESC P ... q ... ESC \\` (sixel) in `goble-terminal`, emit a typed image event with id/format/width/height/payload, and add a bounded per-pane image cache. Render at least a real textured quad for PNG/RGBA kitty images through the existing wgpu pipeline; if sixel decoding needs a dependency, add a small, audited one or implement the 6-bit palette decoder. Placement must respect the cursor cell and scroll with the content. If you cannot finish a sub-part, land the parser + cache + tests and state the exact limitation in `left`; do not fake rendering.\n";
    f5b += "- Tests: two-client daemon broadcast, kitty/sixel parser fixtures, image cache eviction, and a placement test. `cargo test -p goble-terminal`, `cargo test -p goble-pty`, `cargo test -p goble-app --lib`; `cargo check --workspace`.\n\n";
    f5b += "Acceptance: two clients attach; the image parser has fixtures and real rendering where implemented. Commit as `feat(terminal): multi-client attach and image protocols`.";
    let f5b_criteria = "- Two clients attach to one session without stealing/killing it, with broadcast + replay tested.\n- Kitty and sixel parsers have fixtures and emit typed events; rendered images (where implemented) are real, and any limitation is stated.\n- `cargo check --workspace`, `cargo test -p goble-terminal`, `cargo test -p goble-pty`, `cargo test -p goble-app --lib` pass.";
    wave("F5b", "F5b multi-client + images", f5b, f5b_criteria);

    // --------------------------------------------------------------- Final
    phase("Final");
    let f_review = "";
    f_review += "You are on the final adversarial panel for the terminal-first adaptation in " + repo() + ".\n";
    f_review += "Run `git log --oneline` and inspect the whole diff against the run base in /work/.sandbox/base.txt. ";
    f_review += "Do not trust commit messages: read the code.\n\n";
    let review_jobs = [
        #{
            prompt: f_review + "Dimension: MOATS. Prove that the following still exist and work: real command blocks (OSC 133/DCS hooks, per-block output), the harness claim protocol that runs agent commands in the user's own shell (`app/src/terminal/claim.rs`), transcripts/tool calls/subagents, and reversible execution (`goble-replay` fork/replay/revert). Run the relevant tests. Report any regression as a failure with file/line evidence.",
            label: "final:moats",
            capability_mode: "execute",
            output_schema: verdict_schema(),
        },
        #{
            prompt: f_review + "Dimension: ARCHITECTURE. Prove: the app no longer owns PTYs and talks to the daemon; attach/detach and restart survival are real; per-pane agent state is event-derived; the Inbox is global and actionable; the control socket/CLI/MCP server are wired; copy/search are usable; layouts and SSH panes work. Check that no xrdp/screen code remains. Report concrete gaps as failures.",
            label: "final:architecture",
            capability_mode: "execute",
            output_schema: verdict_schema(),
        },
        #{
            prompt: f_review + "Dimension: GREEN BUILD. Run `cargo check --workspace` and `cargo test --workspace --no-fail-fast` (allow it to run; do not run a GUI binary). Also run `cargo clippy -p goble-app -p goble-pty -p goble-control -p goble-terminal --all-targets -- -D warnings` if it is reasonable; report any failure with the command and the first actionable error. A test that cannot run in this environment must be named, with the reason.",
            label: "final:build",
            capability_mode: "execute",
            output_schema: verdict_schema(),
        },
    ];
    let verdicts = parallel(review_jobs);
    let final_failures = [];
    let i = 0;
    for v in verdicts {
        if !passed(v) {
            final_failures.push("panel " + i.to_string() + ":\n" + failures_text(v));
        }
        i += 1;
    }

    let scribe = "";
    scribe += "Write the run report and the patch for the terminal-first adaptation in " + repo() + ".\n";
    scribe += "1. Read `" + out() + "/../.sandbox/base.txt` (the run base commit) and `git -C " + repo() + " log --oneline <base>..HEAD`.\n";
    scribe += "2. Write `" + out() + "/goble-terminal-first.patch` with `git -C " + repo() + " diff <base>..HEAD`.\n";
    scribe += "3. Write `" + out() + "/commits.txt` with the commit list.\n";
    scribe += "4. Write `/work/SUMMARY.md` with: what landed per wave (F0-F5), the exact test/check commands and their results, the final panel failures (verbatim), what remains optional/unfinished (especially sixel/kitty rendering), and where the patch is. Do not claim a wave passed if the panel or the build says otherwise.\n";
    scribe += "5. Do not push. Do not rewrite the commits.\n";
    scribe += "Return the summary text.\n";
    let s = agent(scribe, #{ label: "scribe", capability_mode: "all", output_schema: #{
        "type": "object",
        "required": ["summary"],
        "properties": #{ "summary": #{ "type": "string" } },
    }});
    if s != () && s.success && s.output != () && s.output.summary != () {
        log("scribe: " + s.output.summary);
    }

    let all_passed = final_failures.len() == 0;
    complete(#{
        summary: if all_passed { "terminal-first landed; final panel passed" } else { "terminal-first landed with panel failures; see /work/SUMMARY.md" },
        panel_failures: final_failures,
        patch: out() + "/goble-terminal-first.patch",
        report: "/work/SUMMARY.md",
    });
}

main();
