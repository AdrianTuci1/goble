use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::GobleConfig;
use crate::docs;
use crate::principal::PrincipalId;

/// Directories that every user's home has, regardless of whether their workspace
/// is local or remote: identity/auth, config, sessions, client logs, per-principal
/// context and bundled docs.
const BASE_DIRS: &[&str] = &[
    "sessions",
    "logs",
    "principals",
    "docs/user-guide",
    "relocations",
];

/// Directories that only materialize when the workspace runs **on this machine**
/// (local / self-as-worker): bundled tooling, worktrees, the thread server,
/// downloaded binaries, agent workspaces and user-installed plugins/skills/workflows.
/// A remote-only user's home stays minimal — a thin client holding identity + essentials.
const WORKSPACE_DIRS: &[&str] = &[
    "bundled/agents",
    "bundled/roles",
    "bundled/personas",
    "bundled/skills",
    "workspaces",
    "worktrees",
    "threads",
    "downloads",
    "bin",
    "completions",
    "vendor",
    "marketplace-cache",
    "plugins",
    "skills",
    "workflows",
];

/// Goble's per-machine workspace home directory. The home mirrors the `~/.grok`
/// layout so a machine/VM/cluster — which is one workspace — has a single hidden
/// folder holding config, docs, bundled tooling, sessions, worktrees, logs and
/// per-principal data.
pub fn home_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    Ok(home.join(".goble"))
}

/// A handle to the resolved workspace home. `locate()` resolves the default
/// `~/.goble`; tests can build one against an arbitrary root with `at()`.
pub struct GobleHome {
    root: PathBuf,
}

impl GobleHome {
    #[cfg(test)]
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn locate() -> Result<Self> {
        Ok(Self { root: home_dir()? })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store_path(&self) -> PathBuf {
        self.root.join("goble_store.sqlite")
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn threads_dir(&self) -> PathBuf {
        self.root.join("threads")
    }

    pub fn workspaces_dir(&self) -> PathBuf {
        self.root.join("workspaces")
    }

    pub fn principals_dir(&self) -> PathBuf {
        self.root.join("principals")
    }

    /// The user-guide docs directory. Seeded on first launch from `docs::USER_GUIDE`.
    pub fn docs_user_guide_dir(&self) -> PathBuf {
        self.root.join("docs/user-guide")
    }

    /// Every user's home. Always run: seeds identity/auth/config/sessions/logs so
    /// even a remote-only user has identity + essential data locally. Idempotent.
    pub fn ensure_base(&self) -> Result<()> {
        for d in BASE_DIRS {
            fs::create_dir_all(self.root.join(d))
                .with_context(|| format!("create home base dir {d}"))?;
        }
        self.seed_config_if_missing()?;
        self.seed_file_if_missing("README.md", "# Goble home\n\nUser home on this machine. Mirrors the `~/.grok` structure.\n")?;
        self.seed_file_if_missing(
            "version.json",
            &format!("{{\"version\": \"{}\"}}\n", env!("CARGO_PKG_VERSION")),
        )?;
        self.seed_file_if_missing("principal_id", &PrincipalId::default_user().0)?;
        self.seed_file_if_missing("auth.json", "{}\n")?;
        self.seed_docs()?;
        Ok(())
    }

    /// The workspace payload, only created when the workspace runs on this machine
    /// (local / self-as-worker). A remote-only user never materializes this.
    pub fn ensure_workspace(&self) -> Result<()> {
        for d in WORKSPACE_DIRS {
            fs::create_dir_all(self.root.join(d))
                .with_context(|| format!("create workspace dir {d}"))?;
        }
        Ok(())
    }

    /// Full local workspace home: base + workspace payload.
    pub fn ensure(&self) -> Result<()> {
        self.ensure_base()?;
        self.ensure_workspace()
    }

    fn seed_config_if_missing(&self) -> Result<()> {
        let path = self.config_path();
        if path.exists() {
            return Ok(());
        }
        let toml = GobleConfig::default().to_toml()?;
        fs::write(&path, toml).context("write default config.toml")?;
        Ok(())
    }

    fn seed_file_if_missing(&self, name: &str, contents: &str) -> Result<()> {
        let path = self.root.join(name);
        if path.exists() {
            return Ok(());
        }
        fs::write(&path, contents).with_context(|| format!("write {name}"))?;
        Ok(())
    }

    /// Seed `~/.goble/docs/user-guide/` from the embedded user guide. Only writes
    /// files that are missing so it never clobbers the user's edits.
    fn seed_docs(&self) -> Result<()> {
        let dir = self.docs_user_guide_dir();
        fs::create_dir_all(&dir)
            .with_context(|| format!("create docs dir {}", dir.display()))?;
        for (name, contents) in docs::USER_GUIDE {
            let path = dir.join(name);
            if path.exists() {
                continue;
            }
            fs::write(&path, contents)
                .with_context(|| format!("write user guide doc {name}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_scaffolds_tree_and_seeds_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = GobleHome::at(tmp.path().to_path_buf());
        home.ensure().unwrap();

        let store_marker = home.store_path();
        assert_eq!(store_marker.file_name().unwrap(), "goble_store.sqlite");
        assert!(home.config_path().exists());
        assert!(home.threads_dir().is_dir());
        assert!(home.principals_dir().is_dir());
        assert!(home.root().join("bundled/skills").is_dir());
        assert!(home.root().join("docs/user-guide").is_dir());
        assert!(home.root().join("README.md").exists());
        assert!(home.root().join("version.json").exists());
        assert!(home.root().join("principal_id").exists());
        assert!(home.root().join("auth.json").exists());

        // Config round-trips from the seeded TOML.
        let toml = fs::read_to_string(home.config_path()).unwrap();
        let parsed = GobleConfig::from_toml(&toml).unwrap();
        assert_eq!(parsed.version, 1);
    }

    #[test]
    fn ensure_base_is_minimal_for_remote_only_users() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = GobleHome::at(tmp.path().to_path_buf());
        home.ensure_base().unwrap();

        // Base is present for every user.
        assert!(home.config_path().exists());
        assert!(home.root().join("principal_id").exists());
        assert!(home.root().join("auth.json").exists());
        assert!(home.root().join("sessions").is_dir());
        assert!(home.root().join("principals").is_dir());
        assert!(home.root().join("docs/user-guide").is_dir());

        // The workspace payload is NOT materialized for a remote-only user.
        assert!(!home.root().join("bundled/skills").exists());
        assert!(!home.root().join("worktrees").exists());
        assert!(!home.root().join("threads").exists());
        assert!(!home.root().join("plugins").exists());
    }

    #[test]
    fn home_dir_resolves_to_dot_goble() {
        let path = home_dir().unwrap();
        assert!(path.ends_with(".goble"));
    }

    #[test]
    fn ensure_is_idempotent_and_preserves_existing_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = GobleHome::at(tmp.path().to_path_buf());
        home.ensure().unwrap();
        // Overwrite config with a marker and ensure() must not clobber it.
        fs::write(home.config_path(), "version = 99\n").unwrap();
        home.ensure().unwrap();
        let toml = fs::read_to_string(home.config_path()).unwrap();
        assert!(toml.contains("version = 99"));
    }

    #[test]
    fn ensure_base_seeds_user_guide_docs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = GobleHome::at(tmp.path().to_path_buf());
        home.ensure_base().unwrap();

        let dir = home.docs_user_guide_dir();
        assert!(dir.is_dir());
        for (name, _) in docs::USER_GUIDE {
            assert!(dir.join(name).exists(), "missing seeded doc {name}");
        }
        let content = fs::read_to_string(dir.join("01-getting-started.md")).unwrap();
        assert!(content.contains("# Getting Started"));
    }

    #[test]
    fn ensure_base_does_not_clobber_user_guide_edits() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = GobleHome::at(tmp.path().to_path_buf());
        home.ensure_base().unwrap();

        let doc = home.docs_user_guide_dir().join("07-mobile-access.md");
        fs::write(&doc, "# My custom edit\n").unwrap();
        home.ensure_base().unwrap();

        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content, "# My custom edit\n");
    }
}
