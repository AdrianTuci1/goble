let meta = #{
    name: "goble-continue",
    description: "Advance the Goble reversible-execution substrate to 100% system functionality: thread per-project/medium identity, durable Project/Session/Task persistence, then the app/ per-project observability and medium-selector UI. Voice stays model-only. Fails closed on any broken build.",
    phases: [
        #{ title: "Implement", detail: "one agent per stage, bounded retry, incremental edits" },
        #{ title: "Verify", detail: "independent agent runs cargo check + test, fail-closed" },
    ],
    when_to_use: "System + screen + UI functionality, voice deprecated for now. Runs listed stages sequentially; stop at the first that does not verify green.",
};

let verified_schema = #{
    "type": "object",
    "required": ["verified", "logs"],
    "properties": #{
        "verified": #{ "type": "boolean" },
        "logs": #{ "type": "string" },
    },
};

fn impl_brief() {
    let s = "You are implementing one stage of the Goble Rust workspace at /Users/adrian.tucicovencogmail.com/Projects/goble.\n";
    s += "Conventions: edition 2021, rust-version 1.86, workspace deps as `name = { workspace = true }`, micro-crate split (types <- protocol <- runtime).\n";
    s += "A crate in crates/ must NOT depend on app/, goble-ui, goble-core, wgpu, or winit.\n";
    s += "READ the relevant files with read_file/list_dir/grep BEFORE writing, and mirror existing structure + module-level //! doc style.\n";
    s += "Run real cargo commands and show real output; never claim success without a green result.\n";
    s += "Before declaring done, run `cargo check --workspace --all-targets` and `cargo test -p <the-crate>` and require BOTH exit 0; if a compile error or failing test appears, FIX it and re-run.\n";
    s += "If the target already exists and is complete, run its tests, reply 'already-done', and do NOT rewrite it.\n";
    s += "Keep the scope minimal and self-contained; add focused unit tests. Do NOT touch voice (goble-voice / STT-TTS) — it stays as-is.\n";
    s += "Work in SMALL increments: apply one file/change at a time and run `cargo check` after each; do NOT try to emit a whole multi-file change in one long response (that truncates).\n";
    s
}

fn verify_brief(target) {
    let s = "You are independently verifying a stage of the Goble Rust workspace at /Users/adrian.tucicovencogmail.com/Projects/goble.\n";
    s += "Run `cargo check --workspace --all-targets` and `cargo test -p " + target + "`. Show the tail of each command's real output.\n";
    s += "Set verified=true ONLY if the workspace check compiles with no errors AND the stage's tests pass (no failing tests).\n";
    s += "If anything fails (compile error, failing test), set verified=false and paste the relevant error text into logs. Do not be lenient.\n";
    s
}

let stages = [];

// 1) Thread project/medium identity through the daemon execution records + trace events.
let p = impl_brief();
p += "STAGE: thread per-project/medium identity through goble-daemon execution records and trace events.\n";
p += "Read crates/goble-daemon/src/state.rs, crates/goble-daemon-protocol/src/lib.rs, and crates/goble-harness-types (ProjectId, SessionId, MediumId, MediumKind, HarnessTurn) first.\n";
p += "HarnessTurn already carries project_id/medium_id. Add typed `project_id` + `medium_id` fields to ExecutionRecord (crates/goble-daemon/src/state.rs) and populate them from the turn where a session is built/recorded. Add the same `project_id`/`medium_id` to DaemonEvent::TraceStarted and TraceFinished in crates/goble-daemon-protocol, emit them from spawn_stream, and update every exhaustive match (map_event, any DaemonEvent consumers).\n";
p += "Add a focused unit test asserting the ExecutionRecord and TraceFinished carry the turn's project_id/medium_id. Ensure `cargo test -p goble-daemon` and `cargo test -p goble-daemon-protocol` are green.\n";
stages.push(#{ name: "daemon-project-identity", target: "goble-daemon", prompt: p });

// 2) Durable Project/Session/Task persistence entities.
p = impl_brief();
p += "STAGE: add durable Project/Session/Task entities to goble-persistence and wire them into the desktop composition root.\n";
p += "Read crates/goble-persistence/src/lib.rs (SCHEMA, CheckpointStore, CheckpointSink) and crates/goble-desktop-service/src/state.rs (open_default, daemon_state) first.\n";
p += "In goble-persistence extend SCHEMA with tables for projects (project_id PK, directory, created_at), sessions (session_id PK, project_id, medium_id, created_at), and tasks (task_id PK, session_id, trigger, status, created_at). Add mirror methods (upsert/list/load/delete) on CheckpointStore, with unit tests using in_memory(). Keep the existing checkpoint table/methods intact.\n";
p += "Wire the new load path into crates/goble-desktop-service/src/state.rs::open_default alongside the existing restore_checkpoint loop (seed projects/sessions/tasks on startup). Ensure `cargo test -p goble-persistence` is green.\n";
stages.push(#{ name: "persistence-entities", target: "goble-persistence", prompt: p });

// 3) app/ per-project observability domain + panel.
p = impl_brief();
p += "STAGE: add the per-project observability domain to the native GUI (app/).\n";
p += "Read app/src/ai/ (the domain template: mod.rs, state.rs, actions.rs), app/src/root_view.rs, app/src/ui/mod.rs, app/src/ui/crons.rs (the 'what's running' template), and app/src/features.rs first.\n";
p += "Create app/src/projects/ (mod.rs + state.rs + actions.rs) following the ai/ template: a ProjectsState (Clone) with from_desktop(&DesktopState)/refresh/mock, listing sessions grouped per project (using the project/medium identity now available on executions, falling back to the 'default' project when absent) plus per-project 'what's running' status. Add ProjectsSnapshot + ProjectsActions to app/src/ui/mod.rs, register `pub mod projects;` in app/src/lib.rs, wire projects_state into root_view.rs (build + snapshot + actions + pass to build_ui), and add an `AppTab::Projects` variant with a build_main branch + a topbar button + an on_* action.\n";
p += "Write an integration test under app/src/integration_testing/ asserting the projects panel renders the per-project list and a running/scheduled status. Ensure `cargo test -p goble-app` is green.\n";
stages.push(#{ name: "app-projects-domain", target: "goble-app", prompt: p });

// 4) app/ medium-selector domain + routing mapping.
p = impl_brief();
p += "STAGE: add the medium-selector domain to the native GUI (app/) and map routing onto the turn's medium/project identity.\n";
p += "Read app/src/ai/ (domain template), app/src/daemon/state.rs, crates/goble-desktop-service/src/state.rs (run_chat_turn, set/get chat workspace routing), and crates/goble-harness-types (MediumId, MediumKind) first.\n";
p += "Create app/src/media/ (mod.rs + state.rs + actions.rs) following the ai/ template: a MediaState (Clone) with a list of mediums (Local/VM/RemoteXrdp/Browser/Container) and selected_medium, + MediaSnapshot/MediaActions in ui/mod.rs, register in lib.rs, wire into root_view, and add a medium selector surface (a rail or a topbar control).\n";
p += "Ensure that when a turn is run, the selected medium maps onto `HarnessTurn.medium_id` (and project_id) instead of always defaulting to 'local'/'default' — thread it through run_chat_turn / DaemonModel::run_turn. Add a unit test asserting the turn is built with the selected medium_id. Ensure `cargo test -p goble-app` and `cargo test -p goble-desktop-service` are green.\n";
stages.push(#{ name: "app-media-domain", target: "goble-app", prompt: p });

// 5) Real screen adapter (capture+control) + wire a registry so the crate is not orphaned.
p = impl_brief();
p += "STAGE: create crate crates/goble-screen-adapter (real capture+control adapter) and wire a ScreenRegistry so it is actually populated.\n";
p += "Read crates/goble-screen-core/src/lib.rs (ScreenCapturer, ScreenController, ScreenFrame, ScreenRegistry, PlatformBackend), crates/goble-desktop-service/src/state.rs (DesktopState), and the workspace Cargo.toml first.\n";
p += "Add the crate as a workspace member (crates/* is globbed). It must NOT depend on app/ or goble-core. Implement ScreenCapturer + ScreenController for a real local source: on macOS `capture()` spawns `screencapture`/`screencapture -x -t png` and decodes into an RGBA8 ScreenFrame using the workspace `image` crate (`{ workspace = true }`); a mock/synthetic adapter provides frames on other platforms. Provide a factory returning `Arc<dyn ScreenCapturer>`/`Arc<dyn ScreenController>`.\n";
p += "Wire a ScreenRegistry into crates/goble-desktop-service (own it on DesktopState, register the local adapter behind a feature/cfg, expose an accessor) so the capturer is reachable and the crate is NOT orphaned. Add unit tests (synthetic frame path always; PNG-decode path behind cfg(target_os=macos)). Ensure `cargo test -p goble-screen-adapter` and `cargo test -p goble-desktop-service` are green.\n";
stages.push(#{ name: "screen-adapter", target: "goble-screen-adapter", prompt: p });

// 6) app/ screen domain (broadcast + computer-use).
p = impl_brief();
p += "STAGE: add the screen domain to the native GUI (app/) for broadcast + computer-use.\n";
p += "Read app/src/ai/ (domain template), app/src/projects/ (a recently added domain to mirror), app/src/root_view.rs, app/src/ui/mod.rs, and crates/goble-screen-core (ScreenCapturer/ScreenController) first.\n";
p += "Create app/src/screen/ (mod.rs + state.rs + actions.rs) following the ai/ template: a ScreenState (Clone) with broadcast (live capture via ScreenRegistry through desktop-service) and computer-use (click/type/scroll) flags, plus a capturable source list. Add ScreenSnapshot + ScreenActions to app/src/ui/mod.rs, register `pub mod screen;` in app/src/lib.rs, wire screen_state into root_view (build + snapshot + actions + pass to build_ui), and add a `screen` surface (an AppTab variant or a sheet).\n";
p += "Write an integration test under app/src/integration_testing/ asserting the screen panel renders and a broadcast/computer-use toggle is wired. Ensure `cargo test -p goble-app` is green.\n";
stages.push(#{ name: "app-screen-domain", target: "goble-app", prompt: p });

// 7) Real sandbox isolation (bubblewrap subprocess, no unsafe) + enforce hardened flags.
p = impl_brief();
p += "STAGE: make goble-sandbox actually isolate — implement a bubblewrap backend and enforce the hardened flags.\n";
p += "Read crates/goble-sandbox/src/lib.rs (Sandbox trait, SandboxProfile, SandboxLevel, HardenedFlags, NoopSandbox, SandboxGuard, run_in_sandbox) and crates/goble-sandbox/src/backends/ first.\n";
p += "Implement a `BwrapSandbox` (Linux target) whose prepare() builds a `bwrap` subprocess command carrying the hardened flags: `--unshare-net` for no_network, `--ro-bind`/`--remount-ro` for read_only_fs, `--die-with-parent` and `--unshare-pid` for no_new_privs, and best-effort seccomp for disable_ptrace (note what bwrap cannot express). Because the workspace denies `unsafe`, spawn the `bwrap` executable (no raw syscalls). Make SandboxGuard::run actually run the prepared command through the sandbox wrapper when level >= Hardened (and keep allow-list enforcement at AllowList). Wire sandbox_for() to pick BwrapSandbox on Linux for Hardened profiles.\n";
p += "Add unit tests asserting the constructed bwrap command carries the expected hardened flags for a Hardened profile (arg construction; no need for bwrap installed). Ensure `cargo test -p goble-sandbox` is green.\n";
stages.push(#{ name: "sandbox-real", target: "goble-sandbox", prompt: p });

// 8) Wire the goble-workflow engine into goble-daemon.
p = impl_brief();
p += "STAGE: wire the goble-workflow engine into goble-daemon so workflows actually run.\n";
p += "Read crates/goble-workflow/src/lib.rs (Workflow, WorkflowEngine, WorkflowHostRequest, StepExecutor), crates/goble-daemon/src/port.rs + state.rs (DaemonPort, run), and crates/goble-harness-types (Trigger) first.\n";
p += "Add to goble-daemon: a `StepExecutor` that runs a step by invoking a harness (via the daemon run seam) and returns StepOutcome from the run result; a `WorkflowHost` that journals WorkflowHostRequests and, on a Trigger, restores/builds the Workflow and runs it through WorkflowEngine, persisting the WorkflowRun; and surface it on DaemonPort (a `run_workflow` method) with a DaemonRequest variant in goble-daemon-protocol. Keep it framework-agnostic (no goble-core).\n";
p += "Add unit tests (a MockStep/harness stub) asserting a workflow runs steps in order and a failing step marks the run failed. Ensure `cargo test -p goble-daemon` and `cargo test -p goble-workflow` are green.\n";
stages.push(#{ name: "workflow-daemon-wiring", target: "goble-daemon", prompt: p });

// 9) Replay settlement primitives (select/apply/release/discard).
p = impl_brief();
p += "STAGE: add settlement primitives (select/apply/release/discard) to goble-replay and surface them on DaemonPort.\n";
p += "Read crates/goble-replay/src/lib.rs (ReplayLedger: rewind_to, fork, checkpoint, restore, replay, settle_turn*, TurnStatus) and crates/goble-daemon/src/port.rs + state.rs (DaemonPort rewind/fork/replay/checkpoints) first.\n";
p += "Add to ReplayLedger: `select(at)` (mark a checkpoint turn as the selected candidate), `apply(at)` (commit a selected candidate), `release(at)` (drop a pending candidate without rewinding), `discard(at)` (throw away a candidate and its tail), consistent with the index-aware ledger. Surface them on DaemonPort with DaemonRequest variants in goble-daemon-protocol, and update any exhaustive matches.\n";
p += "Add unit tests for select/apply/release/discard semantics. Ensure `cargo test -p goble-replay` and `cargo test -p goble-daemon` are green.\n";
stages.push(#{ name: "replay-settlement", target: "goble-replay", prompt: p });

let only = if args == () { () } else { args.stage };
let start_at = if args == () { () } else { args.start_at };
let retries = if args != () && args.retries != () && args.retries >= 1 { args.retries } else { 2 };
let started = if start_at == () { true } else { false };

for st in stages {
    if only != () && st.name != only { continue; }
    if !started {
        if st.name == start_at { started = true; } else { continue; }
    }

    phase("Implement: " + st.name);
    let done = false;
    let tries = 0;
    while tries < retries && !done {
        let imp = agent(
            st.prompt,
            #{ label: "implement:" + st.name, capability_mode: "all" },
        );
        if imp != () && imp.success { done = true; }
        tries += 1;
    }
    if !done {
        complete(#{ outcome: "failed", stage: st.name, reason: "implement agent did not complete after " + retries.to_string() + " attempt(s)" });
    }

    phase("Verify: " + st.name);
    let ver = agent(
        verify_brief(st.target),
        #{ label: "verify:" + st.name, capability_mode: "execute", output_schema: verified_schema },
    );
    let ok = ver != () && ver.success && ver.output != () && ver.output.verified == true;
    if !ok {
        let logs = if ver == () || ver.output == () { "verify agent did not complete" } else { ver.output.logs };
        complete(#{ outcome: "failed", stage: st.name, logs: logs });
    }

    log("Stage " + st.name + " implemented and verified green.");
}

complete(#{ outcome: "all-done", stages_run: stages.len(), budget: budget() });
