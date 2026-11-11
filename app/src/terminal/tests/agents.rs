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
