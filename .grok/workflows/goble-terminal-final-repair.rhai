// Terminal-first final repair: close the gaps the final adversarial panel found
// in the continuation run. The repo is at /work/goble on branch `terminal-first`.
//
// Each repair wave lands as one new commit. Earlier wave commits are not amended.
// Nothing is pushed.

let meta = #{
    name: "goble-terminal-final-repair",
    description: "Repair the gaps from the terminal-first final panel: shipping, clipboard, pane attribution, inbox polish, new clippy lints, docs, then re-verify and regenerate the patch",
    when_to_use: "Run inside the Modal harness after the terminal-first continuation's final panel.",
    phases: [
        #{ title: "R1 shipping + clipboard", detail: "release and packaging ship the daemon and control clients; system clipboard works off macOS" },
        #{ title: "R2 attribution + inbox polish", detail: "stable per-pane event attribution; immediate state refresh; overlay fits a 768-pt window" },
        #{ title: "R3 lints + docs", detail: "clear the new clippy lints, document layout kinds, fix the script mode" },
        #{ title: "R4 re-verify", detail: "re-run the failing panel dimensions and release the result" },
        #{ title: "R5 report", detail: "regenerate the patch from 9902e1c and rewrite SUMMARY.md" },
    ],
};

fn repo() { "/work/goble" }
fn out() { "/work/out" }
fn run_base() { "9902e1c" }

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
    h += "Use `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt`, `bash -n`, and `rg` only. ";
    h += "Do not push, do not change git remotes, do not touch `origin`. Commit locally. ";
    h += "Do not amend commits from the F0-F5b waves; each repair wave is one new commit. ";
    h += "Keep the workspace rule `unsafe_code = deny`; do not add `unsafe`, `#[allow(dead_code)]`, or stubs that silently do nothing. ";
    h += "Do not add `#[allow(clippy::...)]` to silence a lint; fix the code. ";
    h += "Keep goble's moats intact: real command blocks (OSC 133/DCS hooks), the harness claim protocol, transcripts/tool calls/subagents, and reversible execution (`goble-replay` fork/replay/revert). ";
    h += "Bound every test command with `timeout 900`. If a test hangs, kill it, inspect the hang and fix it. ";
    h += "In your final answer use the JSON schema you were given; report the exact checks you ran and anything left undone.";
    h
}

fn impl_prompt(wave, body) {
    let p = house();
    p += "\n\nYou are the implementer for repair wave " + wave + ".\n\n" + body;
    p += "\n\nDefinition of done: the touched crates pass `cargo check`, the wave's tests pass, and you committed the result as one new commit. If a check is red, fix it before committing; if something is genuinely impossible in this environment, say so in `left` and commit the best coherent state.";
    p
}

fn verify_prompt(wave, criteria) {
    let p = house();
    p += "\n\nYou are an independent, adversarial verifier for repair wave " + wave + ". You did not write this code. ";
    p += "Inspect the actual diff (`git -C " + repo() + " show --stat HEAD` and `git -C " + repo() + " show HEAD`) and the files it touches. ";
    p += "Run the acceptance checks yourself; do not trust the implementer's report. A check you could not run is a failure. ";
    p += "Report `passed: true` only when every acceptance criterion is met with concrete evidence.\n\nAcceptance criteria:\n" + criteria;
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
    if r == () { log(label + ": no result"); return; }
    if !r.success { log(label + ": agent failed"); return; }
    if r.output != () && r.output.summary != () {
        log(label + ": " + r.output.summary);
    } else {
        log(label + ": done");
    }
}

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
    repair += "The independent verifier rejected repair wave " + wave_name + " with these concrete failures:\n\n" + failures + "\n\n";
    repair += "Fix the cause of every failure. Do not delete a test unless the test itself is wrong and you explain why. ";
    repair += "Commit the fix as one new commit (do not amend the previous repair commit).";
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
    log("goble-terminal-final-repair: repo " + repo());

    // ------------------------------------------------------------------ R1
    let r1 = "";
    r1 += "Close the two shipping gaps from the final panel.\n\n";
    r1 += "A. Packaging and release must ship `goble-app`, `goble-pty-daemon`, `goble-cli`, and `goble-mcp` together.\n";
    r1 += "- Update `scripts/release.sh`: build all four release binaries for the host (and both slices for a universal macOS build). Pass their paths to the macOS bundle and to the Linux packaging scripts. Keep the existing channel/version env behavior.\n";
    r1 += "- Update `packaging/macos/make-bundle.sh` so the daemon, CLI, and MCP binaries land in `Contents/MacOS/` next to the app binary (that is the sibling path `Client::resolve_daemon_exe` already checks). Keep the bundle layout valid.\n";
    r1 += "- Update `packaging/linux/make-deb.sh`, `make-tarball.sh`, and `make-appimage.sh` so all four binaries are installed into the same bin directory as the app. Fail loudly when one is missing instead of shipping a partial package.\n";
    r1 += "- Update `.github/workflows/release.yml`: build all four with `cargo build --release -p goble-app -p goble-pty-daemon -p goble-cli -p goble-mcp` (or four steps) and make sure the packaging inputs point at all four. Keep the existing signing/notarization flow working.\n";
    r1 += "- Check `crates/goble-pty/src/client.rs::resolve_daemon_exe`: it must find `goble-pty-daemon` as a sibling of the running app on every platform, then fall back to PATH and the env override. Add a unit test for the candidate order with a temp dir if the current shape allows it.\n";
    r1 += "- Verify with `bash -n` on every touched shell script and by `rg` that all four binary names appear in the release workflow and the three Linux packaging scripts. Also run `cargo test -p goble-update` and `cargo test -p goble-pty`.\n\n";
    r1 += "B. The system clipboard must work on Linux and Windows, not only macOS.\n";
    r1 += "- `app/src/clipboard.rs` currently uses `pbcopy`/`pbpaste` on macOS and a thread-local buffer elsewhere. Add real command-backed paths: Linux tries `wl-copy`/`wl-paste`, then `xclip -selection clipboard`, then `xsel --clipboard`; Windows uses `cmd /C clip` for write and `powershell -NoProfile -Command Get-Clipboard` for read. Keep the in-process fallback when no helper runs, and log the failure.\n";
    r1 += "- `app/src/actions/make_actions.rs::on_copy_terminal` (around line 818) currently shells out to `pbcopy` directly and only logs elsewhere. Route it through the same clipboard helper so terminal block copy works on every platform.\n";
    r1 += "- Make the command choice testable without invoking real clipboard tools: a pure function that returns the ordered candidate argv for a platform, plus tests for macOS, Linux, and Windows, and a fallback test where every command fails.\n";
    r1 += "- Acceptance: `cargo check --workspace`, `cargo test -p goble-app --lib` (clipboard and terminal copy tests), `cargo test -p goble-pty`, `cargo test -p goble-update`. Commit as `fix(terminal): ship the daemon and control clients; cross-platform clipboard`.\n";
    let r1_criteria = "";
    r1_criteria += "- `scripts/release.sh`, `.github/workflows/release.yml`, `packaging/macos/make-bundle.sh`, and the three Linux packaging scripts build or install all four binaries; `bash -n` passes on every touched script.\n";
    r1_criteria += "- `resolve_daemon_exe` prefers the sibling `goble-pty-daemon` and has a test for the candidate order.\n";
    r1_criteria += "- Clipboard write/read use real system commands on Linux and Windows and keep the fallback; `on_copy_terminal` uses the shared helper.\n";
    r1_criteria += "- `cargo check --workspace`, `cargo test -p goble-app --lib`, `cargo test -p goble-pty`, `cargo test -p goble-update` pass.";
    wave("R1", "R1 shipping + clipboard", r1, r1_criteria);

    // ------------------------------------------------------------------ R2
    let r2 = "";
    r2 += "Fix the pane-attribution bug and the remaining F2 state/overlay gaps.\n\n";
    r2 += "A. Stable per-pane event attribution.\n";
    r2 += "- `ensure_all_chat_pane_sessions` (`app/src/state/paths.rs:53-66`) leaves pane 1's `conversation_id` empty. Give every chat pane a conversation id at creation, including the initial pane.\n";
    r2 += "- A plain-Enter send (`app/src/actions/make_actions.rs:575-625`, `app/src/actions/prompt.rs:52`) uses the sidebar fallback without recording it. Record the id it actually uses on the pane runtime before sending.\n";
    r2 += "- `chat:*` handlers (`app/src/root_view/events.rs:26-31`; `app/src/state/ui_state/agents.rs`) resolve the pane with `pane_id_for_conversation(...).unwrap_or(active_pane_id)`. Make the conversation-to-pane binding authoritative: a `chat:*` event must go to the pane that started the turn even if focus moved. Keep the active-pane fallback only for events that genuinely carry no conversation id, and make that explicit.\n";
    r2 += "- Add a regression test with two panes: start a turn on pane A, switch focus to pane B, then deliver command-proposed, ask/question, and turn-finished events. Assert A's `PaneAgentState`, pending approval/question, and inbox items change and B's do not.\n\n";
    r2 += "B. F2 polish.\n";
    r2 += "- `on_command_decision`, `on_skip_ask`, and `on_stop` must refresh `agent_state` immediately (approve -> Working/Idle as the real event implies; skip -> Working; stop -> Idle), using the same derivation as the event path. Do not fabricate a state; reuse the existing rules.\n";
    r2 += "- The populated inbox overlay can lay its footer below a 768-pt window and its list viewport can pass the dialog bottom. Constrain the overlay to the viewport, make the list scroll within the available height, and keep the footer visible at 768 and 600. Remove `INBOX_PANEL_WIDTH` if unused, or use it.\n";
    r2 += "- Tests: state refresh after approve/skip/stop, overlay layout at 768x600 and 600x400, and the two-pane attribution test. Run `cargo test -p goble-app --lib` and `cargo test -p goble-ui`.\n\n";
    r2 += "Acceptance: `cargo check --workspace`, `cargo test -p goble-app --lib`, `cargo test -p goble-ui`. Commit as `fix(agents): bind events to the pane that started the turn`.\n";
    let r2_criteria = "";
    r2_criteria += "- Every chat pane, including pane 1, has a recorded conversation id; a plain-Enter send records the id it uses.\n";
    r2_criteria += "- A chat event for a turn started on pane A lands on A after focus moves to B; B stays unchanged, proven by a test.\n";
    r2_criteria += "- Approve, skip, and stop refresh `agent_state` immediately.\n";
    r2_criteria += "- The inbox overlay footer is visible and the list stays inside the dialog at 768 and 600 heights.\n";
    r2_criteria += "- `cargo check --workspace`, `cargo test -p goble-app --lib`, `cargo test -p goble-ui` pass.";
    wave("R2", "R2 attribution + inbox polish", r2, r2_criteria);

    // ------------------------------------------------------------------ R3
    let r3 = "";
    r3 += "Clear the new build-panel lints and documentation gaps.\n\n";
    r3 += "- Fix these new clippy violations with idiomatic code: `crates/goble-pty/src/server.rs:417` too_many_arguments; `crates/goble-terminal/src/screen/search.rs:23` derivable_impls; `app/src/control/mod.rs:98` manual_ok; `app/src/terminal/copy.rs:885` let-and-return; `app/src/state/ui_state/spaces.rs:507` let-else-to-?; `app/src/actions/make_actions.rs:990` collapsible_if; `app/src/ui/panes.rs:341` type_complexity. Re-run the linter and fix anything else it reports in the touched files.\n";
    r3 += "- Do not add `#[allow(clippy::...)]`. Group arguments into a struct where the signature is genuinely too wide.\n";
    r3 += "- `docs/control-plane.md:70-71` lists only `single|horizontal|vertical` for `layout.set`; the protocol also accepts `bsp`, `master_stack`, and `floating`. Fix the doc and any other stale verb list you find there.\n";
    r3 += "- `packaging/sign-manifest.sh` is recorded 100755 in git but arrives 0644 through the volume. Restore the executable bit in the index (`git update-index --chmod=+x packaging/sign-manifest.sh`) and commit the mode so `cargo test -p goble-update` passes.\n";
    r3 += "- The pre-existing `goble-core` lints are outside this run. The acceptance gate is scoped: `cargo clippy -p goble-app -p goble-pty -p goble-control -p goble-terminal -p goble-cli -p goble-mcp --all-targets --no-deps -- -D warnings` must exit 0. Record the pre-existing workspace lint debt in `left`.\n";
    r3 += "- Run `cargo check --workspace`, the scoped clippy gate, `cargo test -p goble-update`, and the tests for the files you touched.\n\n";
    r3 += "Acceptance: the scoped clippy gate exits 0, the docs list every layout kind, the script mode is fixed, and the targeted tests pass. Commit as `chore(terminal): clear the new lints and document the control surface`.\n";
    let r3_criteria = "";
    r3_criteria += "- The scoped clippy gate with `--no-deps -D warnings` exits 0 after the fixes.\n";
    r3_criteria += "- No new `#[allow(clippy::...)]` was added.\n";
    r3_criteria += "- `docs/control-plane.md` documents `bsp`, `master_stack`, and `floating` for `layout.set`.\n";
    r3_criteria += "- `packaging/sign-manifest.sh` is executable in git and `cargo test -p goble-update` passes.\n";
    r3_criteria += "- `cargo check --workspace` and the targeted tests pass.";
    wave("R3", "R3 lints + docs", r3, r3_criteria);

    // ------------------------------------------------------------------ R4
    phase("R4 re-verify");
    let head = "";
    head += "You are re-running the terminal-first final panel after repair waves R1-R3 in " + repo() + ". ";
    head += "Inspect the actual code and diff; do not trust commit messages. Do not run a GUI binary. ";
    head += "A check you could not run is a failure. Report `passed:true` only with concrete evidence.\n\n";
    let jobs = [
        #{
            prompt: head + "Dimension: SHIPPING AND CLIPBOARD. Prove that release and packaging build/install `goble-pty-daemon`, `goble-cli`, and `goble-mcp` alongside `goble-app` (read `scripts/release.sh`, `.github/workflows/release.yml`, `packaging/macos/make-bundle.sh`, `packaging/linux/*.sh`), that `resolve_daemon_exe` finds the sibling daemon, and that clipboard read/write use real Linux and Windows commands with a working fallback. Run `bash -n` on the touched scripts and the relevant tests.",
            label: "reverify:shipping",
            capability_mode: "execute",
            output_schema: verdict_schema(),
        },
        #{
            prompt: head + "Dimension: ATTRIBUTION AND INBOX. Prove that every chat pane including pane 1 has a conversation id, that a plain-Enter send records the id it uses, and that a `chat:*` event for a turn started on pane A lands on A after focus moves to B. Prove approve/skip/stop refresh `agent_state`, and that the inbox overlay footer and list stay inside 768- and 600-pt windows. Run the tests.",
            label: "reverify:attribution",
            capability_mode: "execute",
            output_schema: verdict_schema(),
        },
        #{
            prompt: head + "Dimension: GREEN BUILD. Run `cargo check --workspace`, `cargo test --workspace --no-fail-fast`, and `cargo clippy -p goble-app -p goble-pty -p goble-control -p goble-terminal -p goble-cli -p goble-mcp --all-targets --no-deps -- -D warnings`. Name every failure with the command and first actionable error. Treat environment-only failures (long sandbox cwd, a script mode) as failures unless the code fix makes them pass here. Also re-run the moats tests: `cargo test -p goble-terminal`, `cargo test -p goble-replay`, `cargo test -p goble-app --lib terminal::tests::claims`.",
            label: "reverify:build",
            capability_mode: "execute",
            output_schema: verdict_schema(),
        },
    ];
    let verdicts = parallel(jobs);
    let failures = [];
    let i = 0;
    for v in verdicts {
        if !passed(v) {
            failures.push("panel " + i.to_string() + ":\n" + failures_text(v));
        }
        i += 1;
    }

    if failures.len() > 0 {
        let all = "";
        for f in failures {
            all += f + "\n\n";
        }
        let fix = "";
        fix += "The re-verification panel rejected the repair with these concrete failures:\n\n" + all;
        fix += "\nFix the cause of every failure. Commit as one new commit. Do not amend R1-R3 commits.";
        let rr = agent(impl_prompt("R4 repair", fix),
            #{ label: "repair:R4", capability_mode: "all", output_schema: impl_schema() });
        report("repair R4", rr);

        phase("R4 re-verify after repair");
        let v1 = agent(jobs[0].prompt, #{ label: "reverify2:shipping", capability_mode: "execute", output_schema: verdict_schema() });
        let v2 = agent(jobs[1].prompt, #{ label: "reverify2:attribution", capability_mode: "execute", output_schema: verdict_schema() });
        let v3 = agent(jobs[2].prompt, #{ label: "reverify2:build", capability_mode: "execute", output_schema: verdict_schema() });
        if !passed(v1) { failures.push("shipping after repair:\n" + failures_text(v1)); }
        if !passed(v2) { failures.push("attribution after repair:\n" + failures_text(v2)); }
        if !passed(v3) { failures.push("build after repair:\n" + failures_text(v3)); }
    }

    // ------------------------------------------------------------------ R5
    phase("R5 report");
    let scribe = "";
    scribe += "Rewrite the run report and regenerate the patch for the terminal-first adaptation in " + repo() + ".\n";
    scribe += "1. Use the run base `" + run_base() + "` (F0 starts there). Confirm it is an ancestor of HEAD.\n";
    scribe += "2. Write `" + out() + "/goble-terminal-first.patch` with `git -C " + repo() + " diff " + run_base() + "..HEAD`. This patch must include F0, F1a, the docs cleanup, F1b-F5b, and R1-R4.\n";
    scribe += "3. Verify the patch applies cleanly: create a temporary worktree or clone at `" + run_base() + "` and run `git apply --check` with the patch, then remove the temporary worktree.\n";
    scribe += "4. Write `" + out() + "/commits.txt` with the full commit list from " + run_base() + ".\n";
    scribe += "5. Write the run summary to /work/SUMMARY.md and copy it to " + out() + "/SUMMARY.md. Include: what landed per wave, the exact commands and results, the re-verification outcome, the remaining optional/unfinished items (control protocol version negotiation, pre-existing clippy debt outside the run, formatting drift, images not observed on a GPU, GUI not launched), and where the patch is. Be honest about anything still red.\n";
    scribe += "6. Do not push. Do not rewrite commits.\n";
    scribe += "Return the summary text.\n";
    let s = agent(scribe, #{ label: "scribe", capability_mode: "all", output_schema: #{
        "type": "object",
        "required": ["summary"],
        "properties": #{ "summary": #{ "type": "string" } },
    }});
    if s != () && s.success && s.output != () && s.output.summary != () {
        log("scribe: " + s.output.summary);
    }

    complete(#{
        summary: if failures.len() == 0 { "terminal-first final repair landed; re-verification passed" } else { "terminal-first final repair landed with open panel failures; see /work/SUMMARY.md" },
        panel_failures: failures,
        patch: out() + "/goble-terminal-first.patch",
        report: "/work/SUMMARY.md",
    });
}

main();
