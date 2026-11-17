use super::*;

#[test]
fn detects_known_tui_agents_by_first_token() {
    assert_eq!(TuiAgent::detect("codex"), Some(TuiAgent::Codex));
    assert_eq!(TuiAgent::detect("claude"), Some(TuiAgent::Claude));
    assert_eq!(TuiAgent::detect("gemini --model x"), Some(TuiAgent::Gemini));
    assert_eq!(TuiAgent::detect("opencode"), Some(TuiAgent::OpenCode));
    assert_eq!(TuiAgent::detect("cursor-agent"), Some(TuiAgent::Cursor));
    assert_eq!(TuiAgent::detect("cursor"), Some(TuiAgent::Cursor));
    assert_eq!(TuiAgent::detect("aider"), Some(TuiAgent::Aider));
    assert_eq!(TuiAgent::detect("pi"), Some(TuiAgent::Pi));
    assert_eq!(TuiAgent::detect("grok"), Some(TuiAgent::Grok));
    assert_eq!(TuiAgent::detect("hermes"), Some(TuiAgent::Hermes));
    assert_eq!(TuiAgent::detect("agy"), Some(TuiAgent::Antigravity));
    assert_eq!(
        TuiAgent::detect("agy --dangerously-skip-permissions"),
        Some(TuiAgent::Antigravity)
    );
}

#[test]
fn detection_skips_wrappers_and_paths() {
    assert_eq!(TuiAgent::detect("sudo codex"), Some(TuiAgent::Codex));
    assert_eq!(TuiAgent::detect("npx codex"), Some(TuiAgent::Codex));
    assert_eq!(TuiAgent::detect("env FOO=1 codex"), Some(TuiAgent::Codex));
    assert_eq!(
        TuiAgent::detect("/usr/local/bin/claude"),
        Some(TuiAgent::Claude)
    );
    assert_eq!(TuiAgent::detect("bunx gemini"), Some(TuiAgent::Gemini));
}

/// Whether a line turns into a harness's agent view is read from the program
/// the line runs: a harness named as an argument is text the shell handles.
#[test]
fn launching_reads_the_program_the_line_runs() {
    assert_eq!(TuiAgent::launched_by("codex"), Some(TuiAgent::Codex));
    assert_eq!(
        TuiAgent::launched_by("codex --full-auto"),
        Some(TuiAgent::Codex)
    );
    assert_eq!(TuiAgent::launched_by("sudo npx codex"), Some(TuiAgent::Codex));
    assert_eq!(TuiAgent::launched_by("env FOO=1 claude"), Some(TuiAgent::Claude));
    assert_eq!(
        TuiAgent::launched_by("/usr/local/bin/hermes --resume x"),
        Some(TuiAgent::Hermes)
    );
    assert_eq!(TuiAgent::launched_by("echo codex"), None);
    assert_eq!(TuiAgent::launched_by("man claude"), None);
    assert_eq!(TuiAgent::launched_by("grep -r claude src"), None);
    assert_eq!(TuiAgent::launched_by("ls"), None);
    assert_eq!(TuiAgent::launched_by(""), None);
}

#[test]
fn every_catalog_harness_is_recognised_by_its_command() {
    use goble_core::harness::HARNESSES;
    for spec in HARNESSES {
        if spec.id == "goble" {
            continue;
        }
        let command = spec
            .commands
            .first()
            .expect("a CLI harness names a command");
        let detected = TuiAgent::detect(command)
            .unwrap_or_else(|| panic!("{} ({command}) is not recognised", spec.id));
        assert_eq!(detected.id(), spec.id, "{command}");
        assert_eq!(detected.label(), spec.name, "{command}");
    }
}

#[test]
fn a_protocol_harness_leaves_the_screen_to_the_pane() {
    let auggie = TuiAgent::detect("auggie").expect("auggie is in the catalog");
    assert_eq!(auggie.id(), "auggie");
    assert!(
        !auggie.owns_screen(),
        "an ACP server does not paint a screen"
    );
    let codex = TuiAgent::detect("codex").expect("codex is in the catalog");
    assert!(codex.owns_screen(), "codex's TUI owns the pane");
}

#[test]
fn detection_rejects_shell_commands() {
    assert_eq!(TuiAgent::detect("ls"), None);
    assert_eq!(TuiAgent::detect("git status"), None);
    assert_eq!(TuiAgent::detect("cd /tmp"), None);
    assert_eq!(TuiAgent::detect(""), None);
}

#[test]
fn the_program_to_check_skips_wrappers_flags_and_assignments() {
    use crate::terminal::agents::program_to_check;

    assert_eq!(program_to_check("codex"), Some("codex"));
    assert_eq!(program_to_check("codex --full-auto"), Some("codex"));
    assert_eq!(program_to_check("sudo -E codex"), Some("codex"));
    assert_eq!(program_to_check("env FOO=1 BAR=2 claude"), Some("claude"));
    assert_eq!(program_to_check("command exec aider"), Some("aider"));
    assert_eq!(
        program_to_check("/usr/local/bin/claude --resume x"),
        Some("/usr/local/bin/claude")
    );
    assert_eq!(program_to_check(""), None);
    assert_eq!(program_to_check("sudo -E"), None);
}

/// A package runner fetches the package it names, so it is the program the
/// check looks for: `npx -y codex` is installed when `npx` is.
#[test]
fn a_package_runner_is_the_program_to_check() {
    use crate::terminal::agents::program_to_check;

    assert_eq!(program_to_check("npx -y codex"), Some("npx"));
    assert_eq!(program_to_check("bunx gemini"), Some("bunx"));
    assert_eq!(program_to_check("uvx aider"), Some("uvx"));
    assert_eq!(program_to_check("pnpm dlx pi"), Some("pnpm"));
    assert_eq!(program_to_check("yarn codex"), Some("yarn"));
}

#[test]
fn a_program_is_installed_when_its_file_is_executable() {
    assert!(
        command_installed("/bin/sh"),
        "an absolute path to a real executable is installed"
    );
    assert!(
        !command_installed("/definitely/not/a/real/binary-xyz"),
        "a path nothing lives at is not installed"
    );
    assert!(
        !command_installed("definitely-not-a-real-binary-xyz"),
        "a bare name nobody on PATH provides is not installed"
    );
    assert!(
        command_installed("env FOO=1 /bin/sh"),
        "the wrapper is skipped and the path is checked"
    );
}

#[cfg(unix)]
#[test]
fn a_path_is_installed_only_when_it_is_an_executable_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("create a temp dir");
    let program = dir.path().join("a-harness");
    std::fs::write(&program, "#!/bin/sh\n").expect("write the program");
    let name = program.to_str().expect("a path");

    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
        .expect("make it executable");
    assert!(command_installed(name), "an executable file is installed");

    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o644))
        .expect("drop the execute bits");
    assert!(
        !command_installed(name),
        "a file with no execute bit cannot be run"
    );
    assert!(
        !command_installed(dir.path().to_str().expect("a path")),
        "a directory is not a program"
    );
}
