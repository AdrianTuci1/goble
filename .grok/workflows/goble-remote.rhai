// One detached Goble task, run by the grok harness.
//
// Contract, the same one the `harness-grok` profile uses in core-infra:
//
//   args.task        the task file written by the .modal control plane
//   /work/goble      the checkout, on branch `modal/<label>`
//   /work/SUMMARY.md the report `sandbox_summary` reads
//
// The workflow is one implementation pass: the agent edits, runs targeted
// checks, and commits locally. Nothing is pushed; the control plane exports the
// patch from the volume when the sandbox exits.

let meta = #{
    name: "goble-remote",
    description: "Run one scoped Goble task from args.task in the sandbox checkout",
    when_to_use: "Started by the .modal control plane with a task file.",
    phases: [
        #{ title: "Work", detail: "one agent implements, verifies and commits the task" },
    ],
};

fn trimmed(s) {
    if type_of(s) == "string" {
        s.trim();
        s
    } else {
        ""
    }
}

fn house() {
    let h = "";
    h += "You are working on Goble, a native Rust macOS app with its own immediate-mode UI (goble-ui, winit + wgpu) in a Cargo workspace. ";
    h += "The repository is /work/goble, on the branch the harness checked out. Nobody else writes to this checkout. ";
    h += "Reply in Romanian in the final summary; code, comments, commit messages, and docs stay in English. ";
    h += "Commit locally. Never push, never change a remote, never open a pull request. ";
    h += "The app is a native GUI: never run `cargo run`, `cargo build`, `cargo install`, or launch any GUI binary. ";
    h += "Use `cargo check`, targeted `cargo test -p <crate>`, `cargo clippy -p <crate> --all-targets`, `cargo fmt`, `bash -n`, and `rg`. ";
    h += "Run targeted tests only, never the full workspace suite, and bound every test command with `timeout 900`; if a test hangs, kill it, find the cause, and fix it. ";
    h += "Keep the workspace lint `unsafe_code = deny`. Do not add `unsafe`, `#[allow(dead_code)]`, or `#[allow(clippy::...)]` to silence a problem: fix the cause. ";
    h += "Do not reformat or rewrite files outside the change, and do not touch `target/` or generated artifacts. ";
    h += "Keep Goble's moats intact: real command blocks (OSC 133/DCS hooks), the harness claim protocol, transcripts/tool calls/subagents, and reversible execution (goble-replay fork/replay/revert). ";
    h += "Verify before reporting: a change is done only when the relevant targeted checks pass. State plainly what you could not verify and why. ";
    h += "Read the matching document under .agents/ before changing a subsystem. ";
    h
}

if args == () || args.task == () {
    pause("verification", "Lipsește args.task: control plane-ul scrie task-ul în /work/.sandbox/task-<label>.md.");
}
let task = trimmed(args.task);
if task == "" {
    pause("verification", "args.task e gol: nu am ce lucra.");
}

log("goble-remote: " + task.len().to_string() + " bytes de task");

// -----------------------------------------------------------------------------
// Work — one pass over the task
// -----------------------------------------------------------------------------
phase("Work");
let prompt = house();
prompt += "\n# Task\n\n";
prompt += task;
prompt += "\n\n# Before you finish\n\n";
prompt += "1. Run the targeted checks the task asks for; fix what fails instead of reporting around it.\n";
prompt += "2. Commit the result locally as one or more coherent commits. Leave the working tree clean.\n";
prompt += "3. Write /work/SUMMARY.md with the write_file tool, in Romanian, with exactly these sections. The file is outside the checkout: write it even when the task forbids changing repository files.\n";
prompt += "   - **Ce am făcut** — pe scurt, fără cod.\n";
prompt += "   - **Ce am verificat** — ce comandă ai rulat și ce a ieșit.\n";
prompt += "   - **Ce a rămas** — ce n-ai terminat și de ce.\n";
prompt += "   - **Ce ar trebui să vadă un om** — riscuri și decizii care merită confirmate.\n";
prompt += "4. Report the commit hashes and the checks you ran.\n";

let work = agent(prompt, #{
    label: "goble-remote",
    capability_mode: "all",
});

let ok = false;
let answer = "";
if work != () && work.success == true {
    ok = true;
    if type_of(work.output) == "string" {
        answer = trimmed(work.output);
    }
} else {
    log("goble-remote: agentul a eșuat");
}

complete(#{
    workflow: "goble-remote",
    ok: ok,
    summary_file: "/work/SUMMARY.md",
    answer: answer,
});
