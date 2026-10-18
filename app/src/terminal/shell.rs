use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Child process / PTY session
// ---------------------------------------------------------------------------

/// The shell to spawn: `$SHELL` when set, else `/bin/zsh`.
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
}

/// Resolve a pane's working directory: the per-session path when it exists,
/// else the process cwd (a sane fallback so a shell always launches).
pub fn resolve_cwd(cwd: &str) -> PathBuf {
    if !cwd.is_empty() {
        let p = Path::new(cwd);
        if p.is_dir() {
            return p.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// The name a shell was invoked as, without its path: `bash`, `zsh`, ...
fn shell_name(shell: &str) -> &str {
    Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(shell)
}

/// A per-session scratch directory for the integration script, so two panes
/// never share one. Removed when the session drops.
fn integration_dir() -> Option<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("goble-shell-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// How the daemon should start one pane's shell.
///
/// The integration script is still the app's: bash is pointed at it with
/// `--rcfile`, zsh with `$ZDOTDIR` plus the shim's own variable. The daemon
/// only receives a program, its arguments and its environment, so PTY bytes
/// (and the hooks inside them) remain the app's to interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShellLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Scratch directory to remove when the session drops.
    pub integration_dir: Option<PathBuf>,
}

/// Build a pane's shell launch, pointing a supported shell at its
/// shell-integration script. bash is given `--rcfile`; zsh reads
/// `$ZDOTDIR/.zshrc`. Each script then reads the user's own rc itself, because
/// both mechanisms replace the file the shell would normally read. An
/// unsupported shell is spawned unchanged and degrades to a plain terminal.
pub(super) fn shell_command(shell: &str) -> ShellLaunch {
    let program = shell.to_string();
    let name = shell_name(shell);
    if name.starts_with("bash") {
        let Some(dir) = integration_dir() else {
            return ShellLaunch {
                program,
                args: Vec::new(),
                env: Vec::new(),
                integration_dir: None,
            };
        };
        let script = dir.join("goble.bashrc");
        if std::fs::write(&script, goble_terminal::integration::BASH).is_err() {
            return ShellLaunch {
                program,
                args: Vec::new(),
                env: Vec::new(),
                integration_dir: None,
            };
        }
        ShellLaunch {
            program,
            args: vec![
                "--rcfile".to_string(),
                script.display().to_string(),
            ],
            env: Vec::new(),
            integration_dir: Some(dir),
        }
    } else if name.starts_with("zsh") {
        let Some(dir) = integration_dir() else {
            return ShellLaunch {
                program,
                args: Vec::new(),
                env: Vec::new(),
                integration_dir: None,
            };
        };
        if std::fs::write(dir.join(".zshrc"), goble_terminal::integration::ZSH).is_err() {
            return ShellLaunch {
                program,
                args: Vec::new(),
                env: Vec::new(),
                integration_dir: None,
            };
        }
        if std::fs::write(dir.join(".zshenv"), goble_terminal::integration::ZSH_ENV).is_err() {
            return ShellLaunch {
                program,
                args: Vec::new(),
                env: Vec::new(),
                integration_dir: None,
            };
        }
        let mut env = Vec::new();
        // Hand the script the directory of the user's own zshrc; ZDOTDIR now
        // points at ours, so it cannot find it on its own.
        if let Some(original) = std::env::var_os("ZDOTDIR")
            .filter(|value| !value.is_empty())
            .or_else(|| std::env::var_os("HOME"))
        {
            env.push((
                "GOBLE_ORIG_ZDOTDIR".to_string(),
                original.to_string_lossy().into_owned(),
            ));
        }
        env.push(("ZDOTDIR".to_string(), dir.display().to_string()));
        ShellLaunch {
            program,
            args: Vec::new(),
            env,
            integration_dir: Some(dir),
        }
    } else {
        ShellLaunch {
            program,
            args: Vec::new(),
            env: Vec::new(),
            integration_dir: None,
        }
    }
}
