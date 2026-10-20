// Implement the remote direction (.agents/02, 03, 05, 06/remote-terminal-renderer.md)
// as a long-running task: one tracker item at a time, each one adversarially verified with
// the repo's own test commands.
//
// Standing rules encoded in every prompt: never build/run the goble-app binary, one item per
// agent, no placeholders, no commits, tracker updated only with proof, and say what could not
// be verified instead of claiming it.

let meta = #{
    name: "remote-rollout",
    description: "Implement the remote direction in dependency waves - the key passthrough to a paired worker, the SSH panel and its session chip, the cloud-agent viewer session - one tracker item at a time, each adversarially verified with the repo's own test commands.",
    when_to_use: "Use to advance the remote items in .agents/02-first-run-and-routing/remote-bootstrap.md, .agents/03-workspace-model/shared-secrets-and-toml.md, .agents/05-execution-router-and-targets/runtime-targets.md and .agents/06-renderer/remote-terminal-renderer.md. Never builds or runs the GUI binary; leaves every change uncommitted for review.",
    phases: [
        #{ title: "Wave 0: Testability", detail: "the worker's own tests run, CI covers the worker, and the install script is exercised in a container" },
        #{ title: "Wave 1: The keys reach the worker", detail: "the PushSecrets producer that is missing" },
        #{ title: "Wave 3: The SSH panel", detail: "parse the command, name the host, enter agent mode on the same session" },
        #{ title: "Wave 4: The cloud agent", detail: "a viewer session with no local shell, re-attach, environment and model" },
        #{ title: "Wave 5: Follow-up", detail: "the install's data root" },
        #{ title: "Wave 6: Follow-up", detail: "the leaks the regression check found - a viewer session left alive by a closed tab, an undrained pipe in the always-on worker test, and the data-root guard's string comparison" },
        #{ title: "Wave 7: Follow-up", detail: "the surface drift the wave-6 regression check found (a pane that returns to the shell keeps drawing the agent surface)" },
        #{ title: "Integration", detail: "full suite plus adversarial review of the whole diff" },
    ],
};

// ---------------------------------------------------------------- args

let waves_arg = if args == () { "all" } else if args.waves == () { "all" } else { args.waves };
// `waves: "ids:K1,C2"` runs only those items; `passed: "K1,C1"` seeds their deps as
// already verified so a resumed run does not re-implement what already passed.
let passed_arg = if args == () || args.passed == () { "" } else { args.passed };
// Every role runs on one model by default: the implementers and repairs that write, and the
// verifiers and the review panel that only read and run one command. The slug list is read
// once at startup, so an edit to ~/.grok/config.toml only takes effect after a restart.
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
        "not_verified": #{ "type": "string" },
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
    s += "- NEVER build, run or install the goble-app GUI binary: no `cargo build`, no `cargo run`, no `cargo install`, never launch the app. The repository owner explicitly forbids it. Permitted: `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt --check`, `rustfmt`, `git status`, `git diff`, `grep`, `bash -n`.\n";
    s += "- Resolve EXACTLY ONE item. No drive-by refactors, no fixing other items, no placeholders, no `TODO: rest is left`.\n";
    s += "- Read before writing: `.agents/GUIDE.md`, the owning doc named in the item, and the real code behind every file:line the item cites. Do not guess APIs.\n";
    s += "- Verify with the item's own command and paste its real output. Never claim anything you did not observe.\n";
    s += "- There is no provisioned remote host here and no network peer: a path that needs a live VPS or a paired worker CANNOT be exercised in this run. Prove such an item with unit and integration tests over the protocol and the state, then say plainly in `not_verified` what only a real host could settle. Do not describe an untested path as working.\n";
    s += "- Exception, and only for the testability items (wave 0): a Docker daemon IS available on this machine and may be used to run a fixture container (`--privileged` is available for a systemd-as-PID-1 host). That is a local test fixture, never a requirement placed on the user's host: the install script must keep carrying no container runtime and no language runtime, and the worker stays a binary installed over SSH.\n";
    s += "- This is a UI repo: for anything a user sees, a drawn-command or element test is the proof available, and the pixels stay unverified. Do not weaken or delete an existing test to make a change pass - if a test now encodes the old behaviour, rewrite it and say why in your report.\n";
    s += "- Do not run `git commit`, `git push`, or any command that rewrites history. Leave the working tree dirty for review.\n";
    s += "- Finish by updating `.agents/TRACKER.md` (flip the item's row to `[x]` ONLY if your acceptance command passed just now; otherwise leave it `[~]` with the reason) and adding one row to the `Recently resolved` table in `.agents/RESOLVER.md` with the exact command and its result. Only one RESOLVER row per item, at the top of the table.\n";
    return s;
}

fn impl_prompt(it, rules, root) {
    let p = "Implement ONE tracker item in the Rust repo at " + root + ".\n\n";
    p += "ITEM: " + it.ids + " - " + it.title + "\n";
    p += "Owning design doc (read it first): .agents/" + it.doc + "\n";
    p += "Likely files: " + it.files + "\n";
    p += "Acceptance (the definition of done): " + it.verify + "\n\n";
    p += "WHAT TO DO:\n" + it.goal + "\n\n";
    p += rules;
    p += "\nReport: id, status (done|partial|failed), files_changed, every command you ran, evidence (the summary line you actually saw), not_verified, tracker_updated.";
    return p;
}

fn verify_prompt(it, claim, root) {
    let p = "You are an ADVERSARIAL VERIFIER for one tracker item in the Rust repo at " + root + ". Assume the implementer is wrong until you prove otherwise with your own eyes.\n\n";
    p += "ITEM: " + it.ids + " - " + it.title + " (design doc .agents/" + it.doc + ")\n";
    p += "The acceptance criterion it must satisfy: " + it.verify + "\n";
    p += "The implementer claims: " + claim + "\n\n";
    p += "DO THIS: inspect the real tree (`git status --short`, `git diff`), read the changed files on disk, then run the acceptance command yourself and read its output. ";
    p += "Check each clause of the acceptance criterion separately and name the ones that are not met. ";
    p += "Then try to break it: a test that passes vacuously, a claim with no command behind it, a path the item names that was left untouched, a neighbouring behaviour that the change broke, a weakened or deleted assertion. ";
    p += "This repo has no live remote host: an item that needs one must still be proven by tests over the protocol and the state, and its `not_verified` note must be honest - treat an unverified claim presented as working as a failure. ";
    p += "NEVER build or run the GUI binary. Do NOT edit anything and do NOT commit. ";
    p += "Set passes=true only if you ran the acceptance command and saw it pass AND every clause of the criterion is met by code you read. Otherwise passes=false and say exactly what is missing.";
    return p;
}

fn repair_prompt(it, rules, root, reason, evidence) {
    let p = "A verifier REFUSED your work on one tracker item in the Rust repo at " + root + ". Fix exactly what it says, nothing else.\n\n";
    p += "ITEM: " + it.ids + " - " + it.title + " (design doc .agents/" + it.doc + ")\n";
    p += "Acceptance criterion: " + it.verify + "\n";
    p += "The verifier's reason: " + reason + "\n";
    p += "The verifier's evidence: " + evidence + "\n\n";
    p += "Re-read the code the verifier names and close the gap it found. Do not start a different item and do not re-do work it already accepted.\n\n";
    p += rules;
    p += "\nReport: id, status, files_changed, commands, evidence, not_verified, tracker_updated.";
    return p;
}

// ---------------------------------------------------------------- goals

let g_k1 = "Seed a paired worker with the keys it is missing, and leave the keys it already holds alone. The decision recorded in `.agents/03-workspace-model/shared-secrets-and-toml.md` is that a remote worker's vault IS that workspace's vault and is authoritative there: the keys stay on the worker. That path already exists — `goble secret set --worker … --url … --name llm_api_key --value …` (`crates/goble-cli/src/commands.rs:221`) sends `DesktopMessage::SetVaultSecret`, the worker handles it at `crates/goblin-worker/src/websocket.rs:201` by writing into the file vault at `/var/goblin/vault.json`, which is loaded at startup (`crates/goblin-worker/src/main.rs:158`), and `llm_factory::default_provider_factory` reads `llm_api_key` back out of it (`crates/goblin-worker/src/llm_factory.rs`). What is missing is ONLY the first turn on a freshly provisioned worker, whose vault is empty: `DesktopMessage::PushSecrets` is handled at `crates/goblin-worker/src/websocket.rs:138` (each secret into the store and the file vault, then saved) but nothing in the tree produces it, so a new worker's turn fails with `no llm_api_key secret available`. Make exactly two changes. (1) A producer on the client, in `crates/goble-desktop-service/src/worker_manager.rs`: `WorkerClient::connect` writes `PairRequest` and then spawns the reader task that calls `state_clone.handle_worker_message`. The push fires after the worker answers `WorkerMessage::Paired` — never before, and never as part of the install (the install script is a file on the host's disk). (2) The receiving side must NOT overwrite a name it already holds. The worker's vault is the authority for that workspace, so a client whose local key differs must not silently change what the remote workspace authenticates with; put the rule on the receiving side so it holds no matter who pushes. Carry only what the worker resolves — `llm_api_key` and the base_url/provider it needs, plus the workspace's enabled `[mcp_servers]` entries through the existing `PushMcpServers` — and never the whole vault. Do NOT make a local `config.toml` edit rewrite a key stored on a worker: changing a remote key stays the explicit `goble secret set` act. Read the payload value out of the same place a local turn resolves its key (`crates/goble-desktop-service/src/state/llm.rs` and `refresh_settings` are where the app reads `config.toml`; do not invent a second reader). A value must never be logged, rendered, or written into a transcript. Prove it against the real code: a `WorkerClient::connect` run against a mock WebSocket server (the pattern already exists in `worker_manager.rs`'s own `test_worker_client_connect_mock`, and the message shape the worker accepts is visible in `crates/goblin-worker/tests/runtime_isolation_e2e.rs`) observes a `PushSecrets` carrying `llm_api_key` with the local value arriving after `Paired` and nothing before it; and driven with a secret the worker already holds, the worker's handler leaves its stored value unchanged. Update the tracker row and the doc's passthrough section to match what you built.";






let g_s1 = "Parse the `ssh` command in the rich input and bind the session to its host. The user types `ssh server` (or an alias from `~/.ssh/config`) and the session must know where it is. The reference reads the target from the command line rather than from an escape sequence: warp-new matches `INTERACTIVE_SSH = ^ssh\\s+` plus its `gcloud`/`eb`/`doctl` variants on the submitted line and renders the host in its block prompt row. For us the work is: recognise a submitted command that opens an interactive SSH session, extract the target (host, optional user, optional port; tolerate `-p`, `-l`, `user@host`, and a bare alias), resolve an alias through `~/.ssh/config` - which this tree already parses, `goble-core::ssh_hosts::SshHosts` with `SshHost { alias, .. }`, read for the Settings page at `app/src/ui/settings.rs` - and bind it to the session so the UI can name it. Find where a submitted shell command is observed: the pane's terminal input path is the shell-side counterpart of the agent's rich input, and `app/src/ui/terminal/tests.rs` already has `cmd_enter_activates_harness_mode_on_a_shell_pane`, so start by reading `app/src/ui/terminal.rs` and `app/src/actions/` to find the submit path rather than inventing one. Put the parser in `goble-core` where it is testable without a UI (there is a precedent for a small scanner in `crates/goble-core/src/ssh_hosts.rs`; follow its style) and keep the UI change thin. Rules: a command that merely CONTAINS the word ssh is not a session (`sudo ssh ...`, a string in an argument, a `grep`); `ssh -V`, `scp`, `sftp` and `ssh -o ... host true` run a command and are not interactive sessions; the binding is cleared by `exit` or by the next command that returns to a local shell if you can observe it, and it must not be guessed. Do not break the existing ssh_hosts tests. `cargo test -p goble-core -p goble-app`: an interactive `ssh host`, `ssh user@host`, `ssh -p 2222 host` and an `~/.ssh/config` alias each bind and yield the expected host/user/port; `scp`, `ssh -V` and a command containing `ssh` as an argument do not.";

let g_s2 = "Name the remote session above the composer. A session bound to a host (S1) must say so, so a remote session is never mistaken for a local one. The reference draws a chip naming `user@host` above the input, fed by the parsed target rather than by an escape sequence. In this tree the composer lives in `crates/goble-ui/src/elements/chat_composer/` and the agent pane's strip and transcript are in `crates/goble-ui/src/views/chat_view/`; the app builds the pane's composer in `app/src/ui/chat/composer.rs` and `app/src/ui/chat/agent.rs`, and the pane's own context pills (directory, branch, attach) are built in `crates/goble-ui/src/elements/context_pill.rs` / the composer's `context_children`. Add the session chip there, driven by the bound session, not by a new global. Rules: a local session draws NO chip (the absence is the signal, and a chip on every session would be noise); the chip names the user and host as the session has them (from `user@host`, or the alias with its resolved address when an alias was used and both are known); it must not be a bordered card (this repo draws agent-adjacent chrome as flat rows and pills, not boxes); and it must disappear when the binding is cleared. Do not reflow the composer's existing rows to make room - the design of the input is settled and other tests pin its insets. `cargo test -p goble-ui -p goble-app`: a bound session draws the chip with the expected text, an unbound one draws none, and clearing the binding removes it.";

let g_s3 = "Let `Cmd+Enter` move a shell pane on an SSH session into terminal + agent mode on the SAME session. This is the load-bearing interaction of the whole direction and the reference's own rule: warp-new's mode switch is an input mode on one live session (`SetInputModeAgent`, bound to Cmd-I/Cmd-Enter) - it never restarts the PTY and never opens a second connection, and the pane keeps whatever the shell already printed. This tree already has part of the shape: `app/src/ui/terminal/tests.rs` has `cmd_enter_activates_harness_mode_on_a_shell_pane` and `goble-desktop-service` has a BYOH/daemon-turn path for an agent turn on a shell pane, so READ those before writing anything. The item's job: on a shell pane bound to an SSH session, `Cmd+Enter` enters agent mode without dropping the connection - the same PTY, the same ssh process, no replayed banner and no reconnect - and the pane's content is preserved. Getting back out must work too, and must not kill the SSH session either: find the existing exit path (warp-new exits on Esc/Ctrl-C with a short confirmation window, and guards both directions with `AlreadyInAgentView` and `LongRunningCommand`) and use what this app already has rather than inventing a second mechanism. Guards that must hold: `Cmd+Enter` on a pane already in agent mode does not push a second mode; a long-running command may refuse the switch, but if it does, it must say so rather than doing nothing. Then the agent turn that runs in that mode executes commands ON the remote (that is what the SSH session is), so read how the harness's shell tool is routed to a pane today before assuming it works - if routing an agent command through an SSH-backed PTY is a separate gap, do NOT build it here: implement the mode switch, and record precisely what the agent's `run_command` does on an SSH-backed pane today, whether or not that is the right answer. Evidence you must show: the ssh process id (or the PTY's own identity) is the same before and after the switch, and the scrollback from before the switch is still there. `cargo test -p goble-app`: the switch happens, the session's identity is unchanged, the pane's earlier content survives, and the double-entry and long-command guards behave.";

let g_s4 = "Keep a plain shell pane agent-free. The user's ask has two halves: a pane with an SSH connection is a terminal, and only `Cmd+Enter` makes it an agent session. So a pane that is neither in agent mode nor bound to a session must be exactly a shell - no agent transcript, no harness round trip, no agent chrome drawn over it - and an SSH-bound pane in terminal mode must behave the same way (it is still just a terminal, with a chip naming where it is). Find where the app decides a pane's content shape (`app/src/ui/terminal.rs` and the pane/root view construction in `app/src/ui/panes.rs` and `app/src/root_view/`) and make that decision read the pane's mode rather than being implied by the pane's kind, then pin it with tests. What must NOT change: the existing agent pane's behaviour, and the existing shell pane's terminal behaviour. Also make sure the mode is a property of the pane that survives the per-frame rebuild the way this framework requires - read how the app stores per-pane state (`PaneRuntime` in `app/src/state/types.rs`) instead of adding a parallel store. `cargo test -p goble-ui -p goble-app`: a shell pane in terminal mode builds no agent surface and dispatches no agent action, an SSH-bound pane in terminal mode still builds none, and after S3's switch the agent surface is there.";

let g_a1 = "Build the cloud-agent pane as a viewer session with no local shell. When a conversation is routed to a remote worker, the pane must stop pretending there is anything local behind it. Today `app/src/daemon/state.rs` has a `DaemonModel.remote` that is always `None` and remote turns error by design, and the pane is built as an ordinary local pane. The reference's own shape (warp-new's `AmbientAgent` tab, `app/src/terminal/view/ambient_agent/`) is a pane created deferred - no local shell, no local PTY - that connects to a session and is fed entirely by the remote stream; when the connection drops the pane shows that, it does not become a shell. Build that shape here: a pane whose content is the streamed conversation and whose backing is the worker connection, with the honest states the reference distinguishes - connecting, attached, detached (with a way back), and failed. Read `goble-desktop-service/src/worker_manager.rs` (`WorkerClient::connect`, and the reader task that calls `handle_worker_message`) for what a connection can and cannot do today, and `crates/goble-core/src/protocol.rs` for the messages that exist. Do NOT invent a transport; use the mTLS/WebSocket client that is there, and if a piece of what the pane needs has no message yet, say so precisely and leave that piece out rather than stubbing it. This item is the pane and its states, not the selectors (the next item) and not re-attach (the one after). `cargo test -p goble-app`: a remote-routed conversation builds a pane with no local terminal attached, a dropped connection moves the pane to its detached state and says so, and a detach never falls back to a local shell.";

let g_a2 = "Re-attach to a running remote session instead of starting over. This is why the direction exists: the run lives on the worker, so closing the laptop or putting it to sleep must not lose it, and on waking the client rejoins what is still running. The machinery is already in the daemon protocol - `DaemonRequest::{Snapshot, Replay, Checkpoints, Rewind, Fork}` and the `Savepoint*`/`Rewind`/`Fork` events in `crates/goble-daemon-protocol/src/lib.rs` - so the work is on the client: when a remote pane opens on a conversation that already has a session, ask for its state and replay what the client missed instead of starting a new turn or an empty transcript; when the connection is dropped and comes back, do the same. Read `crates/goble-desktop-service/src/state/turns.rs` and `chats.rs` to see how a turn and a conversation are keyed, and `translator.rs` for how daemon events become app events, before choosing the entry point. Two rules: the transcript the client shows after a re-attach must be the session's own, not a locally reconstructed guess; and a conversation with no live session must say so rather than silently starting a fresh one in its place. `cargo test -p goble-desktop-service -p goble-app`: an opening on a session with history replays it, a drop-and-return re-attaches to the same session id, and a conversation with no session reports that instead of starting one.";

let g_a3 = "Choose the environment and the model for a cloud-agent conversation, at submit time. The reference puts both next to the send control in the composer and remembers the last pair per session; the environment decides which `MediumKind` the work runs on (`crates/goble-harness-types/src/lib.rs` already has `MediumKind::{Local, Vm, Remote, Browser, Container}` and `MediumId`) and the model is passed through from the local choice - resolved locally and sent, exactly as the reference does. In this tree the composer's footer controls live in `crates/goble-ui/src/elements/chat_composer/` (read how the existing directory/branch selectors and their popups are built - there is a settled idiom here and a `PopupMenu` with a hover tray in `crates/goble-ui/src/elements/popup_menu.rs`), the app's composer wiring is `app/src/ui/chat/composer.rs` and `agent.rs`, and the per-conversation routing already exists as `WorkspaceRouting` persisted on the chats table. Add the two selectors for a remote-routed conversation, persist the last choice per conversation, and make the submit carry both. Rules: a LOCAL conversation shows neither selector (the direction's split is that the remote pane is the one with an environment to choose); the model choice must actually be the one the worker uses, so read how a turn's model is resolved on the worker before wiring the field - `crates/goblin-worker/src/llm_factory.rs` builds the provider from the worker's own secrets and `LLM_PROVIDER`/`LLM_MODEL` env, so a per-turn model chosen in the client has no path to the worker yet. If that path does not exist, do NOT fake it: implement the environment selector, and for the model either add the field to the turn request through the existing protocol (if that is a small, honest change) or report precisely what is missing and leave the model half registered as `[~]` in the tracker. `cargo test -p goble-ui -p goble-app -p goble-desktop-service`: a remote conversation draws both controls or the one you could honestly wire, a local one draws neither, the choice persists per conversation, and the submitted turn carries what was chosen.";

let g_t1 = "Make the worker's own end-to-end tests actually run. `crates/goblin-worker/tests/worker_e2e.rs:14` and `crates/goblin-worker/tests/runtime_isolation_e2e.rs:32` carry `#[ignore = \"E2E: requires a built `goblin` binary (set GOBLIN_BIN); not run in the default suite\"]`, and both fall back to `/root/goble/target/release/goblin` (`worker_e2e.rs:19`, `runtime_isolation_e2e.rs:37`) — a path that exists on no dev machine here, so these tests are not skipped, they are unpassable as written. The binary they need is declared in the same package (`crates/goblin-worker/Cargo.toml`, `[[bin]] name = \"goblin\"`), which is exactly what `env!(\"CARGO_BIN_EXE_goblin\")` is for: cargo builds the binary and populates that variable for the owning package's integration tests. Read both test files first and say what each one actually needs, then remove both `#[ignore]` attributes, replace the `GOBLIN_BIN`/absolute-path lookup with `env!(\"CARGO_BIN_EXE_goblin\")`, and make the tests pass in the default suite on macOS. `runtime_isolation_e2e` may genuinely need something this repo does not build (a full MCP/V8 isolate runtime): if an assertion cannot pass here, split that assertion into its own `#[ignore]`d test with an explicit reason and a documented command, and keep everything else running un-ignored. Do NOT re-ignore a whole file to make the suite green and do NOT weaken or delete an assertion. Run the suite and paste the summary line.";

let g_t2 = "Give the worker a CI job, now that T1 makes its suite runnable. Read `.github/workflows/ci.yml` end to end first: it has exactly one job, `terminal / conformance` (`runs-on: ubuntu-latest`), path-filtered to `crates/goble-terminal/**`, `vendor/alacritty_terminal/**`, `scripts/fetch-alacritty-terminal.sh` and the workflow file, and it restores the vendored emulator before running `cargo nextest run -p goble-terminal --profile ci`. Add a second job in the same file, in the same style and reusing the existing job's Rust/nextest setup steps rather than inventing a parallel setup, whose path filter names every crate the worker links. Do not guess that list: read `crates/goblin-worker/Cargo.toml` and follow its path dependencies (`goble-core`, `goble-daemon`, `goble-daemon-client`, `goble-daemon-protocol`, `goble-harness-internal`, `goble-harness-runtime`, `goble-harness-types`, `goble-workflow`, and whatever those pull in) and write the filter from what you read. Do not narrow or delete an existing job's filter. If `.config/nextest.toml` has no profile the worker job can use, read the file before inventing a profile name; if the job needs no profile, do not add one. Prove it three ways: run the exact command the new job runs, in this environment, and paste its summary line; confirm the YAML parses (a Python YAML load is enough, or `actionlint` if it is installed); and show `git diff .github/workflows/ci.yml` so both jobs are visible. Say plainly in `not_verified` that the `ubuntu-latest` runner itself is not exercised until a push.";

let g_t3 = "Verify the generated install script by actually executing it on a systemd host in a container. `generate_install_script` (`crates/goble-core/src/provision.rs`) emits a script that runs `apt-get`, creates the `goblin` group and user, writes the env file and the systemd unit, then `systemctl daemon-reload`, `enable` and `restart goblin.service`; today it is only ever asserted on as text (`a_string_containing`, `find(\"systemctl restart goblin.service\")`), so nothing has ever run it. Build two things. (1) A container fixture: a Dockerfile for a systemd-as-PID-1 host (a Debian/Ubuntu base plus `systemd` and `systemd-sysv`, `CMD [\"/sbin/init\"]`). Read what is already under `deploy/goblin/` first — the Dockerfile there is a stale k8s artifact (distroless, `--mode=cluster`, copies three of `crates/*` so the workspace cannot even parse) and is NOT a base to extend. (2) One documented command that runs the container with `--privileged --cgroupns=host --tmpfs /run --tmpfs /run/lock -v /sys/fs/cgroup:/sys/fs/cgroup:rw`, generates the install script through the Rust API, copies the script and the `WorkerBundle` TLS files in, runs the script as root, then asserts `systemctl is-active goblin.service` is `active` and `curl http://127.0.0.1:8787/health` answers `{\"status\":\"Online\"}`. The unit's `ExecStart` is `$INSTALL_PATH/goblin --bind 0.0.0.0:8787 …`, so a real Linux `goblin` binary must be at that path when the service starts: build it inside the container from the mounted repo (`-v \"$PWD\":/src`, a Rust image, `cargo build -p goblin-worker --release` — that is the worker, not the GUI, so it is allowed) or take it from an env var pointing at a prebuilt one; prefer whichever actually runs here, and note that a cross-compile to `aarch64-unknown-linux-gnu` on this macOS host is unlikely to be set up, so building in the container is the honest default. Docker is running locally and the base images are arm64, matching this host. If the service genuinely cannot start here, resolve everything up to and including `systemctl enable` and state in `not_verified` exactly what stayed unproven — do NOT put a stub script at the install path and call the service verified. Put the command where a human can re-run it (a script under `scripts/`, or an `#[ignore]`d test whose doc comment carries the container command) and make that same command appear in `.agents/TRACKER.md` and in `.agents/10-platform-and-performance/testing-and-ci.md`.";




let g_d1 = "Deliver D1 — the install's data root becomes explicit and the recursive `chown` is guarded. In `crates/goble-core/src/provision.rs`, `generate_install_script` emits `DATA_ROOT=\"$(dirname \"$WORKSPACE_ROOT\")\"` and then `chown -R goblin:goblin \"$INSTALL_PATH\" \"$DATA_ROOT\"`. Two defects, both found by hand while a verifier drove the T3 container fixture: (a) `workspace_root` is a public field of `ProvisionConfig`, so a caller passing a shallow path (e.g. `/srv`) makes that `chown -R` target a whole system directory — the guard is the caller's good behaviour and nothing else; (b) the derivation is not what the worker actually uses. `crates/goblin-worker/src/main.rs` defaults its task store to `/var/goblin/tasks.db` and its vault to `/var/goblin/vault.json` as fixed absolutes, and the generated systemd unit passes neither `--task-store` nor `--vault-path`, so with a non-default `workspace_root` the service starts against a data root it is not writing to and flaps (`is-active` = `activating`, `Error: Permission denied` or `Unable to open the database file`) — reproduced by hand both ways. Fix both: make the data root an explicit installed value rather than a `dirname` of a public field, pass it to the unit as `--task-store`/`--vault-path` so the worker really uses it, and refuse to `chown -R` an empty path, `/`, or anything without a directory level of its own (fail loudly, do not silently skip). Read the whole `generate_install_script` and `ProvisionConfig` before editing, and check every in-tree caller of `provision_worker`/`ProvisionConfig` so the default stays what those callers get today. Prove it with the wave-0 fixture the T3 item built: the documented container command must still show `systemctl is-active goblin.service` = `active` and `{\"status\":\"Online\"}` from `/health` at 8787; then repeat it with a non-default `workspace_root` and show the service still comes up (that is the clause the old code fails). Add the test that bites (a unit test over the generated script refusing a shallow root, and the container run) and update the `.agents/02-first-run-and-routing/remote-bootstrap.md` task and the D1 row in `.agents/TRACKER.md`.";


let g_a4 = "Deliver A4 — a viewer pane that is gone leaves no session behind. `UiState::pane_workers` maps a pane id to its viewer session, and nothing but `close_worker_pane` ever removes an entry: `self.pane_workers.remove(&pane_id)` appears once in the tree (`app/src/state/ui_state/worker.rs`), reached only from `adopt_routing`'s local-routing branch. Both pane teardown sites leave the session in place — the pane close in `app/src/actions/make_actions.rs` (which removes `pane_sessions`, `pane_runtime`, `pane_hover`, `sub_agent_views` and the pty) and `close_space_at` in `app/src/actions/pane_ops.rs`. `worker_panes` filters by worker id alone, so `worker_status_serving` and `worker_connection_dropped` keep re-attaching and re-detaching pane ids whose panes and tabs no longer exist, on every worker status event, forever. Read `app/src/state/ui_state/worker.rs`, the two teardown sites, `app/src/ui/worker.rs` and `app/src/root_view/tests/worker_pane.rs` first. Remove the session at both teardown sites (one rule, called from both — do not copy the cleanup), and make sure a pane that is destroyed while its session is still attached does not leave the worker's status path holding a dead id. Prove it: a test that opens a viewer pane, closes its tab, and asserts `pane_workers` holds no entry for that pane and that a following worker status event touches no dead pane id; and a test that the pane close path does the same. The A1/A2 tests in `app/src/root_view/tests/worker_pane.rs` must stay green. Say in `not_verified` that no live worker exists here, so the status path is exercised over the app's real event bus, not against a running worker.";

let g_t5 = "Deliver T5 — the worker's always-on end-to-end test drains both pipes. `crates/goblin-worker/tests/worker_e2e.rs` was un-ignored into the default suite and into the new CI job by T1, and it still pipes the spawned worker's stdout (`.stdout(std::process::Stdio::piped())`) and never reads it: only stderr is taken. The worker's tracing output goes to stdout — the sibling test written in the same rollout says so in a comment and drains both pipes (`crates/goblin-worker/tests/runtime_isolation_e2e.rs`). A child that fills an undrained pipe blocks, and nextest's timeout then fails the test that is now always on, and the CI job with it; the report that found this called it latent, reachable once a run emits roughly a pipe buffer of logs. Fix `worker_e2e.rs` to drain stdout the way the sibling does (a reader task or a shared sink, not a blocking read that changes the test's timing), and leave the test's assertions exactly as they are. Prove it: `cargo nextest run -p goblin-worker` or `cargo test -p goblin-worker` exits 0 with the same counts as before your change, and show the diff so a reviewer can see only the draining changed. If nextest is not installed and there is no network to install it, use `cargo test -p goblin-worker` and say so.";

let g_d2 = "Deliver D2 — close the `chown_guarded` bypass the data-root work left open. `generate_install_script` (`crates/goble-core/src/provision.rs`) now derives no data root from `dirname` and routes every `chown -R` through the script's own `chown_guarded`, which refuses `/`, an empty path, a bare top level such as `/srv`, and a relative path — but it compares unnormalised strings, so `/srv//`, `//srv` and any path carrying a `..` component slip past it and still produce `chown -R goblin:goblin /srv`. `workspace_root` is the one field of `ProvisionConfig` with no Rust-side validation, so the shell guard is the only line of defence and it is weaker than it reads. Read the whole `generate_install_script`, `validate_data_root` and `chown_guarded` first, plus every in-tree caller. Normalise before comparing (collapse duplicate separators, resolve `.` and `..` textually — do not touch the filesystem, the path may not exist), reject anything that resolves to `/`, to an empty string or to fewer than two components, and keep the shell guard as the second line rather than the only one. If `workspace_root` cannot be validated sensibly in Rust without breaking the in-tree callers, say so and keep the guard alone, but make the guard correct. Prove it with a unit test over the generated script — or over the guard function, if you extract one — covering `/srv`, `/srv//`, `//srv`, `/srv/../srv`, `/` and an empty string, each asserted to be refused loudly (`exit 1`, never a silent skip), and re-run the wave-0 systemd container command (`cargo test -p goble-core --test install_systemd_e2e -- --ignored --nocapture`) to show the normal path still installs and the service still comes up Active with `/health` answering. If the Docker daemon or the network is unavailable, say exactly that in `not_verified` and keep the unit-test half.";

let g_u1 = "Deliver U1 — a pane that goes back to the shell really goes back. The remote rollout moved a pane's surface decision to `PaneControls::harness_mode` (the per-pane flag `Cmd+Enter` flips): `app/src/ui/terminal/build.rs` reads `state.pane_controls.get(&pane_id).map(|controls| controls.harness_mode).unwrap_or(false)` and draws the agent chat when it is true. Several readers still ask the pane's *view* what surface it is drawing, and the two answers disagree in exactly one state, because the caller whose own comment says the pane returns to the shell never clears the flag: `UiState::bind_active_pane_conversation` (`app/src/state/ui_state/session.rs`) only runs `if self.sub_agent_views.remove(&self.active_pane_id).is_some() { self.pane_controls_mut(self.active_pane_id).view = BlockView::Terminal; }`, so `harness_mode` stays true and the pane keeps drawing the agent surface while `pane_view` says `Terminal`. That state is reachable: an agent-mode terminal pane with a sub-agent child view open, then a sidebar conversation switch or an agent delete (both call `bind_active_pane_conversation`). Two live consequences, found by the rollout's regression check and pinned by no test: (1) `app/src/root_view/element.rs`'s `if matches!(state.pane_view(pane_id), BlockView::Agent { .. })` is false in the drift state, so the root declines the Cmd/Ctrl+F chord and never mounts the shell's own handler (`app/src/ui/terminal/input.rs`'s `toggle_hovered_filter`/`toggle_global_filter`) — the whole-transcript filter chord is dead while the agent transcript is on screen, where at HEAD `view == Agent` was the surface's own answer and it worked; (2) `app/src/actions/make_actions.rs`'s `!cmd` path does `match state.pane_controls(pane_id).view { BlockView::Agent { conversation_id } => Some(conversation_id), BlockView::Terminal => None }`, so a command typed into the agent surface runs with no conversation owner. Fix it by making ONE flag the source of truth for which surface a pane is drawing, clearing it in the caller that returns a pane to the shell, and auditing every other reader of `pane_view` and of `pane_controls(..).view` so no third reader disagrees. Do NOT patch the two readers and leave the flag lying — that is how this drift happened. Read `app/src/state/ui_state/session.rs`, `app/src/ui/terminal/build.rs`, `app/src/root_view/element.rs`, `app/src/actions/make_actions.rs` and `app/src/ui/terminal/input.rs` first, and grep the tree for `pane_view(` and for `.view` on pane controls to find every reader before you choose the fix. Prove it: after a conversation switch that returns a pane to the shell, the pane draws the shell surface, the Cmd/Ctrl+F chord is handled with the shell's handler mounted, and a `!cmd` carries the pane's conversation owner — and all three hold after an agent delete too. The existing mode-switch tests (`app/src/terminal/tests/ssh_session.rs` and the harness-mode tests) must stay green. Say in `not_verified` that the pixels were never seen — no GUI was built or launched — so the chord and the surface are proven by dispatched events and the state they leave.";


// ---------------------------------------------------------------- waves

let waves = [
    #{ n: 0, title: "Testability", phase_title: "Wave 0: Testability", items: [
        item("T1", "T1", "The worker's end-to-end tests actually run", "10-platform-and-performance/testing-and-ci.md",
            "crates/goblin-worker/tests/worker_e2e.rs, crates/goblin-worker/tests/runtime_isolation_e2e.rs, crates/goblin-worker/Cargo.toml",
            [], g_t1,
            "cargo nextest run -p goblin-worker: the previously ignored worker E2E tests run and pass, with no `#[ignore]` left in worker_e2e.rs and no hardcoded `/root/goble/...` fallback path in either file"),
        item("T2", "T2", "The worker has a CI job", "10-platform-and-performance/testing-and-ci.md",
            ".github/workflows/ci.yml, crates/goblin-worker/Cargo.toml",
            ["T1"], g_t2,
            "the new job's exact command passes when run in this environment, the YAML parses, and every crate the worker links is named in the job's path filter"),
        item("T3", "T3", "The install script runs on a real systemd host", "10-platform-and-performance/testing-and-ci.md",
            "crates/goble-core/src/provision.rs, deploy/goblin/",
            [], g_t3,
            "the documented container command runs the generated install script and shows `systemctl is-active goblin.service` = active plus `{\"status\":\"Online\"}` from `/health` at 8787, or the item is left [~] with the exact failure"),
    ]},
    #{ n: 1, title: "The keys reach the worker", phase_title: "Wave 1: The keys reach the worker", items: [
        item("K1", "K1", "The keys a worker lacks are seeded once", "03-workspace-model/shared-secrets-and-toml.md",
            "crates/goble-desktop-service/src/worker_manager.rs, crates/goble-desktop-service/src/state/llm.rs, crates/goblin-worker/src/websocket.rs, crates/goble-core/src/protocol.rs",
            [], g_k1,
            "cargo test -p goble-desktop-service -p goblin-worker: a real WorkerClient::connect against a mock WebSocket server observes a PushSecrets carrying `llm_api_key` after `Paired` and nothing before it, and a pushed name the worker already holds is not overwritten"),
    ]},
    #{ n: 3, title: "The SSH panel", phase_title: "Wave 3: The SSH panel", items: [
        item("S1", "S1", "The ssh command binds the session to its host", "06-renderer/remote-terminal-renderer.md",
            "crates/goble-core/src/ssh_hosts.rs, app/src/ui/terminal.rs, app/src/actions/",
            [], g_s1,
            "cargo test -p goble-core -p goble-app: ssh host, ssh user@host, ssh -p 2222 host and an ~/.ssh/config alias each bind with the right host/user/port; scp, ssh -V and a command containing `ssh` as an argument do not"),
        item("S2", "S2", "The session chip names the host", "06-renderer/remote-terminal-renderer.md",
            "crates/goble-ui/src/elements/chat_composer/, app/src/ui/chat/composer.rs",
            ["S1"], g_s2,
            "cargo test -p goble-ui -p goble-app: a bound session draws the chip with the expected text, an unbound one draws none, and clearing the binding removes it"),
        item("S4", "S4", "A plain pane stays agent-free", "06-renderer/remote-terminal-renderer.md",
            "app/src/ui/terminal.rs, app/src/ui/panes.rs, app/src/state/types.rs",
            [], g_s4,
            "cargo test -p goble-ui -p goble-app: a shell pane in terminal mode builds no agent surface and dispatches no agent action, and an SSH-bound one behaves the same in terminal mode"),
        item("S3", "S3", "Cmd+Enter enters agent mode on the same session", "06-renderer/remote-terminal-renderer.md",
            "app/src/ui/terminal.rs, app/src/ui/terminal/tests.rs, app/src/actions/",
            ["S1", "S4"], g_s3,
            "cargo test -p goble-app: the switch happens, the ssh session's identity is unchanged before and after, the pane's earlier content survives, and the double-entry and long-command guards hold"),
    ]},
    #{ n: 4, title: "The cloud agent", phase_title: "Wave 4: The cloud agent", items: [
        item("A1", "A1", "A viewer pane with no local shell", "05-execution-router-and-targets/runtime-targets.md",
            "app/src/daemon/state.rs, app/src/ui/panes.rs, crates/goble-desktop-service/src/worker_manager.rs",
            [], g_a1,
            "cargo test -p goble-app: a remote-routed conversation builds a pane with no local terminal, a dropped connection moves it to its detached state, and a detach never falls back to a local shell"),
        item("A2", "A2", "Re-attach instead of starting over", "05-execution-router-and-targets/runtime-targets.md",
            "crates/goble-desktop-service/src/state/turns.rs, crates/goble-desktop-service/src/state/chats.rs, crates/goble-desktop-service/src/state/translator.rs",
            ["A1"], g_a2,
            "cargo test -p goble-desktop-service -p goble-app: an open on a session with history replays it, a drop-and-return re-attaches to the same session id, and a conversation with no session says so"),
        item("A3", "A3", "Environment and model at submit time", "05-execution-router-and-targets/runtime-targets.md",
            "crates/goble-ui/src/elements/chat_composer/, app/src/ui/chat/composer.rs, app/src/ui/chat/agent.rs",
            ["A1"], g_a3,
            "cargo test -p goble-ui -p goble-app -p goble-desktop-service: a remote conversation draws the controls you could honestly wire and a local one draws neither, the choice persists per conversation, and the submitted turn carries it"),
    ]},
    #{ n: 5, title: "Follow-up: the install's data root", phase_title: "Wave 5: Follow-up (D1 guards the install)", items: [
        item("D1", "D1", "The install's data root is explicit and the chown is guarded", "02-first-run-and-routing/remote-bootstrap.md",
            "crates/goble-core/src/provision.rs",
            [], g_d1,
            "the wave-0 systemd container command still shows `systemctl is-active goblin.service` = active and `/health` answering, and it also comes up with a non-default workspace_root, which the old dirname-derived data root fails"),
    ]},
    #{ n: 6, title: "Follow-up: the leaks the regression check found", phase_title: "Wave 6: Follow-up (one lifecycle leak, one undrained pipe, one guard bypass)", items: [
        item("A4", "A4", "A viewer pane that is gone leaves no session behind", "05-execution-router-and-targets/runtime-targets.md",
            "app/src/state/ui_state/worker.rs, app/src/actions/make_actions.rs, app/src/actions/pane_ops.rs, app/src/root_view/tests/worker_pane.rs",
            [], g_a4,
            "cargo test -p goble-app: after a tab holding a viewer pane is closed, pane_workers holds no entry for that pane and a following worker status event touches no dead pane id"),
        item("T5", "T5", "The worker's always-on E2E test drains both pipes", "10-platform-and-performance/testing-and-ci.md",
            "crates/goblin-worker/tests/worker_e2e.rs",
            [], g_t5,
            "cargo test -p goblin-worker (or nextest, if installed): exit 0 with the same counts as before the change, and the diff shows only the draining"),
        item("D2", "D2", "The chown guard cannot be bypassed", "02-first-run-and-routing/remote-bootstrap.md",
            "crates/goble-core/src/provision.rs",
            [], g_d2,
            "a unit test refuses /srv, /srv//, //srv, /srv/../srv, / and an empty string loudly, and the wave-0 systemd container command still shows the service Active with /health answering"),
    ]},
    #{ n: 7, title: "Follow-up: the surface drift", phase_title: "Wave 7: Follow-up (a pane that goes back to the shell)", items: [
        item("U1", "U1", "A pane that goes back to the shell really goes back", "06-renderer/agent-tui.md",
            "app/src/state/ui_state/session.rs, app/src/ui/terminal/build.rs, app/src/root_view/element.rs, app/src/actions/make_actions.rs, app/src/ui/terminal/input.rs",
            [], g_u1,
            "cargo test -p goble-app -p goble-ui: after a conversation switch and after an agent delete the pane draws the shell surface, the Cmd/Ctrl+F chord is handled, and a `!cmd` carries the pane's conversation owner"),
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

log("Remote rollout starting; waves=" + waves_arg + ", max_repairs=" + max_repairs.to_string());
log("Model routing: writing = " + write_shown + ", reading/verifying = " + read_shown);
if passed_arg != "" { log("Treating as already verified (deps satisfied): " + passed_arg); }
log("Standing rules: no `cargo build`/`cargo run`/`cargo install` for the GUI, no commits, and no remote host exists here - an item that needs one is proven by tests over the protocol and must say what stays unverified.");

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

phase("Wave 5: Integration");
log("Integration: full suite plus adversarial review of the whole diff");

let suite_prompt = "Run the repository's full verification suite for the Rust repo at " + root + " and report exactly what you observe. Run, in this order: `cargo test --workspace --no-fail-fast`, then `cargo check --workspace --all-targets`, then `cargo fmt --check` (report only files this rollout touched if the tree already had unrelated formatting drift). ";
suite_prompt += "NEVER run `cargo build`, `cargo run` or `cargo install` for the app binary - that is forbidden in this repository. ";
suite_prompt += "Report lens='full-suite', ok=true only if every command you ran exited successfully (or the only failure is pre-existing drift you can demonstrate on an untouched file - say which), with the exact summary lines as evidence, plus findings listing any failing test with its name and message.";

let regression_prompt = "Adversarially review the WHOLE uncommitted diff of the Rust repo at " + root + " (`git status --short`, `git diff`) for regressions against what the repository already did. ";
regression_prompt += "Hunt for the failure mode where a change works in isolation but breaks an existing surface: a caller left un-updated, a test weakened or deleted rather than fixed, an assumption about ordering or ownership another module still relies on, a public API changed without its call sites. ";
regression_prompt += "This rollout touches protocol types on both ends (the desktop service and the worker) and pane construction, so check the far end of every changed contract and every place a pane is built. Read the real code. ";
regression_prompt += "Do NOT edit anything and do NOT commit. Never build or run the GUI binary. ";
regression_prompt += "Report lens='regressions', ok=false if you find a concrete regression, with findings naming file and line plus evidence quoting the code.";

let direction_prompt = "Audit the uncommitted diff of the Rust repo at " + root + " against the direction the remote work is supposed to follow. The rules, all of them stated in `.agents/`: ";
direction_prompt += "(1) the host runs NO GUI of ours and the install carries no language runtime, no container runtime and no third-party agent runtime - the worker binary is the whole payload; ";
direction_prompt += "(2) a secret is referenced by NAME and never carried as a value, so no password or key may appear in an event payload, a log line, a tool result or a transcript; ";
direction_prompt += "(3) the agent's own terminal chrome is flat rows and pills, never bordered cards; ";
direction_prompt += "(4) entering agent mode never restarts the shell or the SSH connection. ";
direction_prompt += "Read the changed code and its call sites and flag every violation with the file and the offending construct. ";
direction_prompt += "Do NOT edit anything and do NOT commit. Report lens='direction', ok=false if any rule is violated anywhere in the diff.";

let honesty_prompt = "Audit the bookkeeping of this rollout in the Rust repo at " + root + ". Read `.agents/TRACKER.md`, the `Recently resolved` table in `.agents/RESOLVER.md`, and the uncommitted diff. ";
honesty_prompt += "For every item the tracker now marks `[x]`, find the test named as its proof, then RUN that test (`cargo test -p <crate> <test_name>`) and confirm it exists and passes. ";
honesty_prompt += "A `[x]` whose test does not exist, does not pass, or does not actually exercise the claim is a false claim - report it. ";
honesty_prompt += "Then check the honest half: every item in this rollout has a path a real remote host would settle (a paired worker over a network, the pixels) and no such host exists in this environment, so the item's report and its RESOLVER row must SAY so. An item presented as fully verified with no such note is a finding. ";
honesty_prompt += "Also confirm no agent committed anything (`git log --oneline -3` should show no new commits from this run) and that the forbidden `cargo build`/`cargo run`/`cargo install` were not used. ";
honesty_prompt += "Report lens='tracker-honesty', ok=true only if every `[x]` is backed by a test you ran and saw pass and every unverifiable path is declared, with findings naming each false or unproven claim.";

let checks = parallel([
    with_model(#{ prompt: suite_prompt, label: "integ:suite", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema }, read_model),
    with_model(#{ prompt: regression_prompt, label: "integ:regressions", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema }, read_model),
    with_model(#{ prompt: direction_prompt, label: "integ:direction", capability_mode: "all", agent_type: "general-purpose", output_schema: check_schema }, read_model),
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

let report = "# Remote rollout\n\n";
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
report += "\n## What this environment cannot settle\n\n";
report += "- There is no provisioned host, no paired worker and no network peer here. Every item that names a live path - a key actually arriving at a running worker, a reconnect after a real drop - is proven by tests over the protocol and the app state, and the item's own note must say what remains unproven. None of these should be read as working end to end on a real VPS.\n";
report += "- The pixels: the standing rule forbids building or running the GUI binary, so the session chip and the agent-mode switch are proven by drawn commands, dispatched events and the state they leave, never seen.\n";
report += "- The mode switch against a real interactive shell and a real SSH connection: the PTY's identity before and after is asserted in tests, not observed against a live host.\n";
report += "- The wave-0 container is a fixture, not the user's VPS. A green install-script run proves the script executes and the service starts on a Debian-like arm64 host; it does not prove a particular host's distro, firewall or preinstalled packages.\n";

let report_id = write_scratch_file("remote-rollout-report.md", report);
log("Remote rollout finished: verified=" + verified.to_string() + ", failed=" + failed.to_string() + ", skipped=" + skipped.to_string());
complete(#{ path: report_id, report: report, verified: verified, failed: failed, skipped: skipped });
