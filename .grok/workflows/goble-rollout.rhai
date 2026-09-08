let meta = #{
    name: "goble-rollout",
    description: "Follow the repos, propose a clean change-set for a Goble subsystem, implement it sequentially directly in the repo (write + cargo check), then adversarially verify. Re-run per area and rewrite as progress lands.",
    when_to_use: "Use to advance the Goble daemon/BYOH/voice/screen refactor one subsystem at a time, matching the grok-build + warp-new architecture.",
    phases: [
        #{ title: "Assess", detail: "scan current state + reference patterns" },
        #{ title: "Plan", detail: "single plan agent proposes the change-set" },
        #{ title: "Implement", detail: "sequential agent per item edits the repo + runs cargo" },
        #{ title: "Verify", detail: "one skeptic per implemented item" },
    ],
};

let area = if args == () { () } else { args.area };
if area == () { pause("verification", "Pass args.area — the subsystem to advance (e.g. daemon-split, harness-byoh, voice, screen-computer-use, projects-ui, replay, sandbox-os)."); }
let mode = if args == () || args.mode == () { "full" } else { args.mode };
let goble_root = if args == () || args.goble_root == () { "." } else { args.goble_root };
let grok_ref = if args == () || args.grok_ref == () { "../grok-build" } else { args.grok_ref };
let warp_ref = if args == () || args.warp_ref == () { "../warp-new" } else { args.warp_ref };
let arch_doc = if args == () || args.arch_doc == () { "docs/PROJECT_STRUCTURE.md" } else { args.arch_doc };

let assessment_schema = #{
    "type": "object", "required": ["current_state", "gap", "files"],
    "properties": #{
        "current_state": #{ "type": "string" },
        "gap": #{ "type": "string" },
        "files": #{ "type": "array", "items": #{ "type": "string" } },
    },
};
let change_item_schema = #{
    "type": "object", "required": ["id", "title", "summary", "files", "acceptance", "risk"],
    "properties": #{
        "id": #{ "type": "string" },
        "title": #{ "type": "string" },
        "summary": #{ "type": "string" },
        "files": #{ "type": "array", "items": #{ "type": "string" } },
        "acceptance": #{ "type": "array", "items": #{ "type": "string" } },
        "risk": #{ "type": "string" },
    },
};
let plan_schema = #{
    "type": "object", "required": ["items"],
    "properties": #{ "items": #{ "type": "array", "maxItems": 4, "items": change_item_schema } },
};
let status_schema = #{
    "type": "object", "required": ["item_id", "status", "detail"],
    "properties": #{
        "item_id": #{ "type": "string" },
        "status": #{ "type": "string" },
        "detail": #{ "type": "string" },
    },
};
let verdict_schema = #{
    "type": "object", "required": ["item_id", "passes", "reason", "evidence"],
    "properties": #{
        "item_id": #{ "type": "string" },
        "passes": #{ "type": "boolean" },
        "reason": #{ "type": "string" },
        "evidence": #{ "type": "string" },
    },
};

fn join_str(arr, sep) {
    let out = "";
    if arr == () { return out; }
    let idx = 0;
    for el in arr {
        if idx > 0 { out += sep; }
        out += el;
        idx += 1;
    }
    return out;
}

// ---- Assess ----
phase("Assess");
log("Assessing " + area + " across " + goble_root + " (" + grok_ref + ", " + warp_ref + ")");
let scans = parallel([
    #{
        prompt: "Read the SHIPPED code (not docs) in " + goble_root + " for the subsystem '" + area + "'. Report: current_state (what exists, with real file paths), gap (what the grok-build/warp-new architecture requires but is missing), files (the concrete files that implement it today). Use read-only tools and read the actual source. Empty files is valid only after you have really looked.",
        label: "assess:current",
        capability_mode: "read-only",
        agent_type: "explore",
        output_schema: assessment_schema,
    },
    #{
        prompt: "Find the reference pattern for '" + area + "' in " + grok_ref + " (micro-crate, types/protocol/runtime split, composition-root binary, wire-protocol harness seam, feature flags). Report what Goble should mirror: current_state (what grok-build does, concrete crate/file paths), gap (the thing Goble lacks), files (grok-build files that are the template). Read the source, do not answer from memory.",
        label: "assess:grok-build",
        capability_mode: "read-only",
        agent_type: "explore",
        output_schema: assessment_schema,
    },
    #{
        prompt: "Find the reference pattern for '" + area + "' in " + warp_ref + " (thick-app domain module in app/src/<domain>/, init(ctx) + ViewHandle/ModelHandle, FeatureFlag gating, persistence layer). Report what Goble should mirror: current_state (concrete paths), gap, files. Read the source.",
        label: "assess:warp-new",
        capability_mode: "read-only",
        agent_type: "explore",
        output_schema: assessment_schema,
    },
]);

let assessment_lines = "";
let i = 0;
for s in scans {
    if s != () && s.success && s.output != () {
        assessment_lines += "[scan " + i + "] state: " + s.output.current_state + "\n  gap: " + s.output.gap + "\n";
    } else {
        assessment_lines += "[scan " + i + "] FAILED\n";
    }
    i += 1;
}

// ---- Plan ----
phase("Plan");
let plan_jobs = [ #{
    prompt: "You are the architect. Read " + goble_root + "/" + arch_doc + " and the assess notes below, then propose the cleanest change-set to advance '" + area + "' in " + goble_root + ", following the grok-build + warp-new architecture (new focused crates, no goble-core bloat, app/<domain>/ for UI, feature flags, wire-protocol harness seam). Order the items so the workspace stays green after each. For each item give id, title, summary, files (concrete paths, new or existing), acceptance (testable criteria incl. `cargo check --workspace --all-targets`), risk. At most 4 items. Cover: crate boundaries, dependency direction (crates never depend on app/), and what proves done.\n\nASSESS NOTES:\n" + assessment_lines,
    label: "plan:" + area,
    capability_mode: "read-only",
    output_schema: plan_schema,
} ];
let plan_result = agent(plan_jobs[0].prompt, #{ label: plan_jobs[0].label, capability_mode: "read-only", output_schema: plan_schema });
let items = [];
let plan_text = "";
if plan_result != () && plan_result.success && plan_result.output.items != () {
    items = plan_result.output.items;
    let j = 0;
    for it in items {
        j += 1;
        plan_text += "[" + j + "] " + it.id + ": " + it.title + " (" + it.files.len().to_string() + " files)\n";
    }
} else {
    complete(#{ report: "Plan failed; here is the assessment:\n" + assessment_lines, path: "scratch/goble-rollout-report.md" });
}
log("Plan proposed " + items.len().to_string() + " change items");

if mode == "assess" {
    complete(#{ report: "ASSESS-" + area + "\n" + assessment_lines + "\nPLAN\n" + plan_text, path: "scratch/goble-rollout-report.md" });
}

// ---- Implement ---- (sequential, in the real repo, with shell so agents run cargo)
phase("Implement");
let impl_results = [];
for it in items {
    let prompt = "Implement this change item DIRECTLY IN THE REPO (not a worktree) at " + goble_root + ", following the grok-build + warp-new architecture. Item: " + it.id + " — " + it.title + ". Summary: " + it.summary + ". Create/edit the real files in the working tree: " + join_str(it.files, ", ") + ". You have a shell tool, so after editing run `cargo check --workspace --all-targets` (and `cargo test -p <crate>` for a crate you added) and confirm your files via `git status --short`. Acceptance: " + join_str(it.acceptance, "; ") + ". Report status (done|failed) and detail including the exact cargo result. Do not merely describe the change — the files must exist on disk under " + goble_root + ".";
    let r = agent(prompt, #{ label: "implement:" + it.id, capability_mode: "all", agent_type: "general-purpose", output_schema: status_schema });
    impl_results.push(r);
}
let impl_lines = "";
let k = 0;
for r in impl_results {
    if r != () && r.success && r.output != () {
        impl_lines += r.output.item_id + ": " + r.output.status + " — " + r.output.detail + "\n";
    } else {
        impl_lines += "item[" + k + "]: FAILED\n";
    }
    k += 1;
}

// ---- Verify ----
phase("Verify");
let verify_jobs = [];
let m = 0;
for it in items {
    let outcome = if m < impl_results.len() && impl_results[m] != () && impl_results[m].success { impl_results[m].output } else { () };
    let detail = if outcome != () { outcome.detail } else { "no implementation result" };
    let prompt = "Adversarially verify the change item '" + it.id + "' (" + it.title + ") in " + goble_root + ". Read the actual code that changed and check against the acceptance criteria: " + join_str(it.acceptance, "; ") + ". Also confirm dependency direction (crates never depend on app/). You have a shell tool: gather concrete evidence with `git status --short` and `cargo check --workspace --all-targets` (or `cargo check -p <crate>`), and read the real files on disk. Claimed work: " + detail + ". Set passes=true ONLY with evidence you personally observed (files on disk AND the relevant cargo check/test passing). Otherwise passes=false. Report reason + evidence. Do NOT edit any files.";
    verify_jobs.push(#{
        prompt: prompt,
        label: "verify:" + it.id,
        capability_mode: "all",
        agent_type: "general-purpose",
        output_schema: verdict_schema,
    });
    m += 1;
}
let verdicts = parallel(verify_jobs);
let verify_lines = "";
let n = 0;
for v in verdicts {
    if v != () && v.success && v.output != () {
        verify_lines += v.output.item_id + ": passes=" + v.output.passes.to_string() + " — " + v.output.reason + "\n";
    } else {
        verify_lines += "verify[" + n + "]: no usable evidence\n";
    }
    n += 1;
}

let report = "GOBLE ROLLOUT: " + area + "\n\n== ASSESS ==\n" + assessment_lines + "\n== PLAN ==\n" + plan_text + "\n== IMPLEMENT ==\n" + impl_lines + "\n== VERIFY ==\n" + verify_lines;
let report_id = write_scratch_file("goble-rollout-report.md", report);
complete(#{ report: report, path: report_id, area: area });
