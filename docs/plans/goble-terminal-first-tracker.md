# Goble Terminal-First Tracker

**Goal:** Turn goble into a terminal window manager with the harness integrated: daemon-owned PTYs, per-pane agent state, a global inbox, a control plane, copy/search, layouts, and images, while keeping the moats.

**Source of truth:** the `terminal-first` branch in the Modal sandbox (`sb-Omhza2tEFTCNxmJYcApAmq`), built by the `goble-terminal-first-continue` Rhai workflow. Base commit: `9902e1c`.

**Status:** Stopped at the user's request. The sandbox is terminated and the volume persists. F0 through F5b plus R1-R5 are verified in the sandbox at `f6d1a33`, and the 1.8 MB patch from `9902e1c` is in `.grok/artifacts/terminal-first/`. It applies cleanly and `cargo check` passes locally. Local targeted tests hung on a worker-pane test and were stopped; the authoritative suite is the sandbox's 2,084 passing tests.

**Legend:** `[x]` done and verified · `[ ]` pending · 🔄 in progress

---

## F0 — Remove the remote-desktop surface — ✅ `7007d66`

- [x] Remove the screen crates and the app screen code/tests
- [x] Remove `FrameView`, `open_screen` / `ScreenHandoff`
- [x] Remove xrdp provisioning and the related CLI flags
- [x] Update docs, workflows, and `.agents` references that described xrdp as current
- [x] Workspace check and tests green after the cut

## F1a — Attachable PTY daemon — ✅ `6624a58`

- [x] New `crates/goble-pty` crate with a versioned protocol
- [x] Daemon owns the PTY; a session survives client detach
- [x] 512 KiB replay ring; attach from an arbitrary sequence number
- [x] Multiple clients attached at once; output broadcast to all
- [x] Create, attach, detach, input, resize, kill, signal
- [x] Auto-start, stale socket cleanup, pid file, log file
- [x] Integration tests: survival and replay, broadcast, stale socket

## F1b — App on the daemon — ✅ `9760b3a`

Terminal panes now run through the F1a daemon. The app connects to or starts the daemon, reuses a live persisted session or creates a fresh one with the app's shell-integration args, feeds replayed and live output into the existing emulator, sends input and resize over the socket, detaches on drop, and kills on pane close. `portable-pty` moved out of `app`; `goble-pty` owns it. Verification passed the workspace check, 98 terminal tests, 17+4 PTY tests, and the app library suite after the repair.

- [x] App starts or attaches to the daemon instead of owning the PTY
- [x] Detach, reattach, and kill from the app
- [x] Session id persisted and restored across app restart
- [x] Shell integration and claim protocol preserved end to end
- [x] App-level tests for attach/detach/restart

## F2 — Per-pane agent state and global inbox — ✅ `7a9b335`

Per-pane state is stored on `PaneRuntime` and driven by real events: turn start, command proposals, ask-user, turn finish, terminal errors, and TUI mode. The app-wide inbox dedupes, acks individually or all, counts unread, prunes on pane close, and jumps to the live pane. The topbar has a live unread badge, Cmd/Ctrl+Shift+I opens the overlay, and the desktop service forwards the real turn status and errors. Verified: 446 app lib tests, 20 inbox tests, 9 chat-flow tests, 477 UI tests, 58 desktop-service tests.

Known follow-ups found by the verifier (non-blocking): the populated overlay's footer can lay out below a 768-pt window and its list viewport can pass the dialog bottom; `on_command_decision`, `on_skip_ask`, and `on_stop` do not refresh `agent_state`.

- [x] Per-pane states: `working`, `needs_input`, `idle`, `done`, `errored`
- [x] State derived from harness events and PTY activity
- [x] Global inbox aggregates approvals and questions across panes
- [x] Jump from an inbox item to the pane that needs attention
- [x] Tests for state transitions and inbox aggregation

## F3a — Control protocol — ✅ `b64f90e`

A `goble-control` crate exposes 19 verbs over a Unix socket at `$GOBLE_CONTROL_SOCKET` or `~/.goble/control.sock`: panes, spaces, agents, inbox, layout, and pty operations. The socket is mode 0600, detects stale sockets, and returns structured errors for unknown verbs, bad parameters, missing panes/spaces/items, an unavailable PTY, and a closed window. The app serves it on the winit thread with a 250 ms idle redraw. Verified: 7 lib + 6 integration control tests, 8 app control tests, and 454 app lib tests.

- [x] Control socket served by the app
- [x] Requests for list, focus, input, split, create, close, and state
- [ ] JSON schema with version negotiation — the wire has no version field; verbs are pinned by round-trip tests, so adding a version is a follow-up
- [x] Tests for request/response and error paths

## F3b — CLI and MCP server — ✅ `b93d837`

`goble-cli ctl` speaks the control socket with `--json` support and a real end-to-end test against an in-process server. `goble-mcp` implements MCP 2025-06-18 over stdio with ten tools: list, new, split, close, send input, agent state, inbox, ack inbox, switch space, and layout. Malformed input, unknown methods, bad arguments, and a missing app all return structured errors. `docs/control-plane.md` documents the socket, wire format, 19 verbs, ten tools, and error codes. Verified: CLI lib + 9 tests, MCP 5 + 5 tests, control 7 + 6 tests, app control 8 tests, plus a manual stdio handshake and tool call.

- [x] CLI commands over the control socket
- [x] MCP server exposing the same operations as tools
- [x] Tests, including a real MCP client run — verified with a manual stdio JSON-RPC client; the official Node SDK client was not used

## F4 — Copy mode, selection, scrollback search — ✅ `9ac2cce`

Search covers history and the visible screen, supports smart case and regex with a visible invalid-regex error, shows `current/total`, wraps on Enter/Shift+Enter/n/N, and auto-scrolls to the match. Copy mode has full cursor movement, visual anchoring, yank, and exit on `q`/Escape. Mouse drag, double-click word, triple-click line, and Cmd+C work on both the grid and block surfaces; copy-mode selection is drawn. Verified: 137 terminal tests, 472 app lib tests, 6 focused UI tests, 479 UI tests.

- [x] Copy mode with keyboard navigation
- [x] Mouse and keyboard selection, copy to clipboard
- [x] Scrollback search with match navigation
- [x] Tests for selection, copy, and search

## F5a — Layouts and SSH panes — ✅ `4aa2a66`

`LayoutKind` adds BSP, master-stack, and floating layouts per space, cycled with Cmd+Shift+L and selectable from the palette and control protocol. Floating pane rects survive a switch to BSP, layouts persist across restart, and the existing maximize-pane zoom remains. `[[hosts]]` entries (name, host, user, port, identity) build `ssh -t` argv; a host picker and reconnect prompt open panes on arbitrary hosts, and the SSH binding persists and restores. Verified: 485 app lib tests, 137 terminal tests, 25 core host tests, control and MCP suites.

- [x] BSP, master-stack, and floating tiling
- [x] Zoom and layout switching
- [x] Panes on arbitrary SSH hosts
- [x] Tests for layout math and SSH pane lifecycle

## F5b — Multi-client and images — ✅ `f1bc697`

Two clients attach to one session without stealing or killing it; the second replays from sequence 0, both receive broadcasts, and detaching the second leaves the first alive. Client ids include the process id, so two app instances cannot share a subscription. The image module parses kitty (raw RGB/RGBA, PNG, chunked, `a=p`, `a=d`) and sixel into typed events with RGBA pixels, paints real `DrawImage` commands (including an image-only viewport), clears placements on alternate-screen changes and reset, and documents the unsupported transports, placement details, delete specs, and sixel limits. Verified: 154 terminal + 4 image + 17+5 PTY tests, 488 app lib tests.

- [x] Two clients attached to one session with consistent state
- [x] Kitty graphics protocol parser and rendering
- [x] Sixel parser and rendering
- [x] Tests for multi-client and image escape parsing

## Final — Verification and first report — ⚠️ panel found issues; repair running

The moats reviewer passed: command blocks, the claim protocol, transcripts/tool calls/subagents, and fork/replay/revert all survive, with 14 DCS envelopes from a real bash run and the full test set green. The architecture and build reviewers found issues that still need a repair pass:

- **Architecture — clipboard:** terminal yank and Cmd+C reach the system clipboard only on macOS; other platforms keep a thread-local fallback.
- **Architecture — packaging:** `scripts/release.sh` and the GitHub release workflow build only `goble-app`; the deb, tarball, and AppImage packaging omit `goble-pty-daemon`, `goble-cli`, and `goble-mcp`. A clean install has no daemon for terminal panes and no control surface.
- **Architecture — event attribution:** the initial pane's `conversation_id` stays empty, so `chat:*` handlers can apply agent state, approval cards, and inbox items to the wrong pane when focus changes mid-turn.
- **Build — clippy:** the workspace gate exits 101 on pre-existing `goble-core` lints; with `--no-deps -D warnings`, new violations appear in `goble-pty/server.rs`, `goble-terminal/search.rs`, `app/control/mod.rs`, `app/terminal/copy.rs`, `app/state/ui_state/spaces.rs`, `app/actions/make_actions.rs`, and `app/ui/panes.rs`.
- **Build — environment-only failures:** `menu_flow` fails on the sandbox's long cwd (passes from a short cwd), and `signer_contract` needs `chmod +x` on `packaging/sign-manifest.sh`.

- [x] Adversarial verification panel over the whole change set — the first panel found gaps; the R4 panel passed shipping, attribution, and build
- [x] `/work/SUMMARY.md` written — first report and the R5 rewrite
- [x] Full patch generated from `9902e1c` — 237 files, +19,072 / −9,491, `git apply --check` passes
- [x] Patch retrieved into the local repo — `.grok/artifacts/terminal-first/`
- [ ] Local `cargo check --workspace --all-targets` and tests green — `cargo check` passed (24.5 s); the targeted test run was stopped after 47 minutes with `root_view::tests::worker_pane::a_drop_and_return_re_attaches_to_the_same_session_id` hung past 60 s. The green 2,084-test run in the sandbox remains the authoritative result
- [ ] UI/interaction verified locally; note the native-headless limitation

## Final repair — R1-R5 — 🔄 running

A second Rhai workflow (`goble-terminal-final-repair`) is running in the same sandbox to close the panel's findings:

- [x] R1 — shipping and clipboard: `d6a4385`. Release builds all four binaries; the macOS bundle places them in `Contents/MacOS` and signs them; deb, tarball, AppImage, and the Windows installer install them and fail on a missing one; `resolve_daemon_exe` prefers the sibling; clipboard uses `pbcopy`, `wl-copy`/`xclip`/`xsel`, and `clip`/`Get-Clipboard` with a tested fallback. Verified with dry runs and simulated platforms; native macOS/Windows packaging was checked statically
- [x] R2 — attribution and inbox: `d0c16e6`. Every chat pane is bound to a conversation, a plain-Enter send records its id, and `chat:*` events route through `pane_for_chat_event`, so a turn started on pane A stays on A after focus moves. Approve/skip/stop refresh the state immediately; the overlay footer and list stay inside 1024x768, 768x600, and 600x400. 501 app lib + 480 UI tests pass. Residual risk: a restored layout whose active pane lost its saved conversation can adopt `selected_id` while another pane owns it, which can share a transcript
- [x] R3 — lints and docs: `e5daa35`. The scoped clippy gate with `--no-deps -D warnings` exits 0; `docs/control-plane.md` lists `bsp`, `master_stack`, and `floating`; `packaging/sign-manifest.sh` is 100755; `cargo check --workspace`, 501 app lib, and the terminal/pty/update/control/cli/mcp suites pass. Two integration targets stay red: `chat_flow` has five failures that predate R3 and track the automatic pane-to-conversation binding, and `menu_flow` fails on the long sandbox cwd
- [x] R4 — re-verification: passed after `f6d1a33`. Shipping, attribution, and build are green. `cargo test --workspace --no-fail-fast` runs 102 targets with 2084 passed, 0 failed, 8 ignored (all declared: six need a live DeepSeek key, one Docker systemd, one the MCP/V8 isolate). The scoped clippy gate exits 0, `cargo check --workspace` is green, and the moat suites (`goble-terminal` 161, `goble-replay` 17, claims 8) pass
- [x] R5 — report and patch: `f6d1a33`. `/work/SUMMARY.md` rewritten; the patch from `9902e1c` is 1.8 MB, 237 files, +19,072 / −9,491, and `git apply --check` passes in a clean worktree. The artifact is at `.grok/artifacts/terminal-first/goble-terminal-first.patch`

## Moats preserved (cross-cutting)

- [x] Real command blocks
- [x] Agent commands run in the user's own shell
- [x] Transcript, tool calls, and subagents
- [x] Reversibility: fork, replay, revert

---

**Update rule:** check an item only after its commit or verification exists. Record the commit hash next to the stage heading when the stage lands.
