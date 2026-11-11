//! The harness catalog: every agent CLI goble can recognise, with the icon mark
//! it draws and the commands that launch it.
//!
//! The list and the command-matching rule are lifted from Paseo's provider
//! catalog (`packages/app/src/data/acp-provider-catalog.ts` and
//! `packages/protocol/src/terminal-profiles.ts`): a known provider keeps its
//! brand mark, and a command's base name names the harness. `tui` separates the
//! harnesses that paint an interactive screen from the ones that speak a
//! protocol over stdio: only the former take the pane's screen and its keys
//! (native-first).

/// One agent CLI goble recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessSpec {
    /// The catalog id; also the icon's name (`harness-<id>`).
    pub id: &'static str,
    /// The name a pane shows for it.
    pub name: &'static str,
    /// The atlas entry that draws its mark.
    pub icon: &'static str,
    /// Command base names that launch it.
    pub commands: &'static [&'static str],
    /// Whether it paints its own interactive screen.
    pub tui: bool,
    /// The command that reopens one of its own sessions, with `{session_id}`
    /// standing for the id. `None` for a harness that cannot be resumed from
    /// the command line.
    pub resume: Option<&'static str>,
}

impl HarnessSpec {
    /// The command that reopens `session_id`, or `None` when this harness keeps
    /// no resumable session on the command line.
    pub fn resume_command(&self, session_id: &str) -> Option<String> {
        let template = self.resume?;
        if session_id.trim().is_empty() {
            return None;
        }
        Some(template.replace("{session_id}", session_id.trim()))
    }
}

macro_rules! harness {
    ($id:literal, $name:literal, [], $tui:literal) => {
        harness!($id, $name, [], $tui, None)
    };
    ($id:literal, $name:literal, [$($command:literal),+ $(,)?], $tui:literal) => {
        harness!($id, $name, [$($command),+], $tui, None)
    };
    ($id:literal, $name:literal, [$($command:literal),+ $(,)?], $tui:literal, resume => $resume:literal) => {
        harness!($id, $name, [$($command),+], $tui, Some($resume))
    };
    ($id:literal, $name:literal, [$($command:literal),*], $tui:literal, $resume:expr) => {
        HarnessSpec {
            id: $id,
            name: $name,
            icon: concat!("harness-", $id),
            commands: &[$($command),*],
            tui: $tui,
            resume: $resume,
        }
    };
}

/// Every harness, the native one and the built-in marks first, then Paseo's ACP
/// catalog.
pub const HARNESSES: &[HarnessSpec] = &[
    harness!("goble", "Goble", [], false),
    harness!(
        "antigravity",
        "Antigravity",
        ["agy"],
        true,
        resume => "agy --conversation {session_id}"
    ),
    harness!("claude", "Claude Code", ["claude"], true, resume => "claude --resume {session_id}"),
    harness!("codex", "Codex", ["codex"], true, resume => "codex resume {session_id}"),
    harness!(
        "copilot",
        "GitHub Copilot",
        ["copilot", "github-copilot"],
        true
    ),
    harness!("kiro", "Kiro", ["kiro-cli", "kiro"], true),
    harness!("minimax", "MiniMax", ["minimax"], true),
    harness!("minimax-code", "MiniMax Code", ["mcode"], true),
    harness!("minion-code", "Minion Code", ["minion-code"], false),
    harness!("omp", "OMP", ["omp"], true, resume => "omp --session {session_id}"),
    harness!("opencode", "OpenCode", ["opencode"], true, resume => "opencode --session {session_id}"),
    harness!("pi", "Pi", ["pi"], true, resume => "pi --session {session_id}"),
    harness!("aider", "Aider", ["aider"], true),
    harness!("gemini", "Gemini CLI", ["gemini", "gemini-cli"], true),
    harness!("grok", "Grok", ["grok"], true, resume => "grok --resume {session_id}"),
    harness!("hermes", "Hermes", ["hermes"], true, resume => "hermes --resume {session_id}"),
    harness!("qwen-code", "Qwen Code", ["qwen", "qwen-code"], true),
    harness!("kimi", "Kimi Code CLI", ["kimi"], true),
    harness!(
        "agoragentic-acp",
        "Agoragentic",
        ["agoragentic", "agoragentic-mcp"],
        false
    ),
    harness!("amp-acp", "Amp", ["amp", "amp-acp"], false),
    harness!("auggie", "Auggie CLI", ["auggie"], false),
    harness!(
        "autohand",
        "Autohand Code",
        ["autohand", "autohand-acp"],
        false
    ),
    harness!("cline", "Cline", ["cline"], false),
    harness!("codebuddy-code", "Codebuddy Code", ["codebuddy"], false),
    harness!("codewhale", "CodeWhale", ["codewhale"], false),
    harness!("cortex-code", "Cortex Code", ["cortex"], false),
    harness!(
        "corust-agent",
        "Corust Agent",
        ["corust-agent", "corust-agent-acp"],
        false
    ),
    harness!("crow-cli", "crow-cli", ["crow-cli", "crow"], false),
    harness!("cursor", "Cursor", ["cursor-agent", "cursor"], true),
    harness!(
        "deepagents",
        "DeepAgents",
        ["deepagents", "deepagents-acp"],
        false
    ),
    harness!("devin", "Devin CLI", ["devin"], false),
    harness!("dimcode", "DimCode", ["dimcode"], false),
    harness!("dirac", "Dirac", ["dirac", "dirac-cli"], false),
    harness!("factory-droid", "Factory Droid", ["droid"], false),
    harness!(
        "fast-agent",
        "fast-agent",
        ["fast-agent", "fast-agent-acp"],
        false
    ),
    harness!("fx", "fx", ["fx"], false),
    harness!("gjc", "Gajae Code", ["gjc"], false),
    harness!("glm-acp-agent", "GLM Agent", ["glm-acp-agent"], false),
    harness!("goose", "goose", ["goose"], true),
    harness!("hoplite", "Hoplite", ["hoplite"], false),
    harness!("junie", "Junie", ["junie"], false),
    harness!("kilo", "Kilo", ["kilo"], false),
    harness!("mistral-vibe", "Mistral Vibe", ["vibe", "vibe-acp"], false),
    harness!("nova", "Nova", ["nova"], false),
    harness!("poolside", "Poolside", ["pool"], false),
    harness!("qoder", "Qoder CLI", ["qodercli", "qoder"], false),
    harness!("sigit", "siGit Code", ["sigit"], false),
    harness!("stakpak", "Stakpak", ["stakpak"], false),
    harness!("traecli", "TRAE CLI", ["traecli"], false),
    harness!("vtcode", "VT Code", ["vtcode"], false),
];

/// The catalog entry with `id`, if the catalog has one.
pub fn harness_by_id(id: &str) -> Option<&'static HarnessSpec> {
    let id = id.trim().to_ascii_lowercase();
    HARNESSES.iter().find(|harness| harness.id == id)
}

/// The harness a conversation runs on, named by a harness id or by the model
/// provider the store recorded for it.
///
/// A conversation created around a provider instead of a CLI agent (`xai`,
/// `anthropic`, `openai`, `google`, ...) still resolves to that provider's
/// harness, so its sidebar mark and its pane's bar name the same thing. An
/// empty key is the native harness.
pub fn harness_for(key: &str) -> Option<&'static HarnessSpec> {
    let key = key.trim().to_ascii_lowercase();
    let id = match key.as_str() {
        "" => "goble",
        "anthropic" => "claude",
        "openai" | "openrouter" => "codex",
        "xai" => "grok",
        "google" => "gemini",
        "alibaba" => "qwen-code",
        "moonshot" => "kimi",
        "zhipu" => "glm-acp-agent",
        "amazon" => "kiro",
        "augment" => "auggie",
        "cognition" => "devin",
        "mistral" => "mistral-vibe",
        "bytedance" => "traecli",
        "jetbrains" => "junie",
        other => other,
    };
    harness_by_id(id)
}

/// The harness a command line launches, by the base name of its program.
///
/// Wrapper programs (`sudo`, `command`, `exec`, `env`, `npx`, `bunx`, `uvx`) and
/// their flags and `VAR=value` assignments are skipped, so `npx -y codex` and
/// `/usr/local/bin/claude` name the same harness as the bare command. Package
/// specifiers are normalised to their last path segment without a version, so a
/// scoped package (`@augmentcode/auggie@0.36.0`) still matches `auggie`.
pub fn detect_harness(command: &str) -> Option<&'static HarnessSpec> {
    for token in command.split_whitespace().take(8) {
        if is_wrapper(token) || token.starts_with('-') || is_assignment(token) {
            continue;
        }
        if let Some(harness) = harness_by_command(token) {
            return Some(harness);
        }
    }
    None
}

/// Whether `token` is a wrapper that runs the program after it.
fn is_wrapper(token: &str) -> bool {
    matches!(
        token,
        "sudo" | "command" | "exec" | "env" | "npx" | "bunx" | "uvx" | "pnpm" | "yarn"
    )
}

/// Whether `token` is a shell variable assignment (`FOO=1`).
fn is_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(key, _)| {
        !key.is_empty()
            && key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

/// The catalog entry whose command base name is `token`'s.
fn harness_by_command(token: &str) -> Option<&'static HarnessSpec> {
    let base = command_base(token);
    HARNESSES
        .iter()
        .find(|harness| harness.commands.iter().any(|command| *command == base))
}

/// The program name of a command token: its last path segment, a trailing
/// `.exe`/`.cmd` dropped, and any `@version` suffix removed.
fn command_base(token: &str) -> String {
    let segment = token
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(token)
        .to_ascii_lowercase();
    let segment = segment
        .strip_suffix(".exe")
        .or_else(|| segment.strip_suffix(".cmd"))
        .unwrap_or(&segment);
    segment
        .split_once('@')
        .map_or(segment, |(name, _)| name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_id_has_its_own_icon_name() {
        for harness in HARNESSES {
            assert_eq!(harness_by_id(harness.id).map(|h| h.id), Some(harness.id));
            assert!(
                crate::harness::catalog::HARNESSES
                    .iter()
                    .filter(|other| other.id == harness.id)
                    .count()
                    == 1,
                "{} appears twice",
                harness.id
            );
        }
    }

    #[test]
    fn a_resumable_harness_reopens_its_own_session() {
        let session = "0192f3a4-1111-2222-3333-444455556666";
        for (id, command) in [
            (
                "antigravity",
                "agy --conversation 0192f3a4-1111-2222-3333-444455556666",
            ),
            (
                "claude",
                "claude --resume 0192f3a4-1111-2222-3333-444455556666",
            ),
            ("codex", "codex resume 0192f3a4-1111-2222-3333-444455556666"),
            ("grok", "grok --resume 0192f3a4-1111-2222-3333-444455556666"),
            ("omp", "omp --session 0192f3a4-1111-2222-3333-444455556666"),
            (
                "opencode",
                "opencode --session 0192f3a4-1111-2222-3333-444455556666",
            ),
            ("pi", "pi --session 0192f3a4-1111-2222-3333-444455556666"),
            (
                "hermes",
                "hermes --resume 0192f3a4-1111-2222-3333-444455556666",
            ),
        ] {
            let spec = harness_by_id(id).unwrap_or_else(|| panic!("{id} is in the catalog"));
            assert_eq!(
                spec.resume_command(session).as_deref(),
                Some(command),
                "{id} reopens its session with its own command"
            );
        }
    }

    #[test]
    fn a_harness_without_a_session_store_has_no_resume_command() {
        const RESUMABLE: [&str; 8] = [
            "antigravity",
            "claude",
            "codex",
            "grok",
            "omp",
            "opencode",
            "pi",
            "hermes",
        ];
        for harness in HARNESSES {
            if RESUMABLE.contains(&harness.id) {
                continue;
            }
            assert_eq!(
                harness.resume, None,
                "{} cannot be reopened from the command line",
                harness.id
            );
        }
    }

    #[test]
    fn a_blank_session_id_reopens_nothing() {
        let spec = harness_by_id("claude").expect("claude is in the catalog");
        for id in ["", " ", "   ", "\t\n"] {
            assert_eq!(
                spec.resume_command(id),
                None,
                "{id:?} names no session"
            );
        }
    }

    #[test]
    fn a_session_id_replaces_the_placeholder_whole() {
        let spec = harness_by_id("claude").expect("claude is in the catalog");
        let id = "0192f3a4-1111-2222-3333-444455556666";
        assert_eq!(
            spec.resume_command(&format!("  {id}  ")).as_deref(),
            Some("claude --resume 0192f3a4-1111-2222-3333-444455556666"),
            "the id is trimmed and substituted as one argument"
        );
        assert!(
            !spec
                .resume_command(id)
                .is_some_and(|command| command.contains("{session_id}")),
            "the placeholder does not survive"
        );
    }

    #[test]
    fn a_resumable_harness_paints_its_own_screen() {
        for harness in HARNESSES.iter().filter(|harness| harness.resume.is_some()) {
            assert!(
                harness.tui,
                "{} is resumed into a terminal pane, so it must be a TUI",
                harness.id
            );
        }
    }

    #[test]
    fn commands_name_their_harness_through_wrappers_and_paths() {
        assert_eq!(detect_harness("codex").map(|h| h.id), Some("codex"));
        assert_eq!(detect_harness("npx -y codex").map(|h| h.id), Some("codex"));
        assert_eq!(
            detect_harness("/usr/local/bin/claude").map(|h| h.id),
            Some("claude")
        );
        assert_eq!(detect_harness("sudo -E goose").map(|h| h.id), Some("goose"));
        assert_eq!(
            detect_harness("npx -y @augmentcode/auggie@0.36.0 --acp").map(|h| h.id),
            Some("auggie")
        );
        assert_eq!(detect_harness("ls -la").map(|h| h.id), None);
    }
}
