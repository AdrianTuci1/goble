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
    fn detection_rejects_shell_commands() {
        assert_eq!(TuiAgent::detect("ls"), None);
        assert_eq!(TuiAgent::detect("git status"), None);
        assert_eq!(TuiAgent::detect("cd /tmp"), None);
        assert_eq!(TuiAgent::detect(""), None);
    }
