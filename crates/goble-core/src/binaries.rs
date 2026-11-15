//! Where the binaries that ship beside the app are.
//!
//! A conversation that sets up Remote tells the agent which `goble-cli` to run.
//! The packaged layout puts the CLI beside the app, and a test binary sits one
//! directory deeper (`target/<profile>/deps/`), so both are checked before the
//! bare name, which a shell's `PATH` can still resolve.

use std::path::{Path, PathBuf};

/// The `goble-cli` to name in a command line.
pub fn goble_cli() -> PathBuf {
    match std::env::current_exe() {
        Ok(current) => resolve_from(&current, "goble-cli"),
        Err(_) => PathBuf::from(format!("goble-cli{}", std::env::consts::EXE_SUFFIX)),
    }
}

/// Resolve `name` from `current`'s directory or its parent, falling back to the
/// bare name.
pub fn resolve_from(current: &Path, name: &str) -> PathBuf {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let mut candidates = Vec::new();
    if let Some(dir) = current.parent() {
        candidates.push(dir.join(&file));
        if let Some(up) = dir.parent() {
            candidates.push(up.join(&file));
        }
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(&file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binary_beside_the_app_wins() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let app = dir.path().join("goble-app");
        let cli = dir
            .path()
            .join(format!("goble-cli{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&cli, "").expect("write the stand-in");
        assert_eq!(resolve_from(&app, "goble-cli"), cli);
    }

    #[test]
    fn a_test_binary_finds_the_one_directory_up() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let deps = dir.path().join("deps");
        std::fs::create_dir_all(&deps).expect("deps dir");
        let cli = dir
            .path()
            .join(format!("goble-cli{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&cli, "").expect("write the stand-in");
        assert_eq!(resolve_from(&deps.join("some_test"), "goble-cli"), cli);
    }

    #[test]
    fn nothing_found_leaves_the_bare_name() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let bare = PathBuf::from(format!("goble-cli{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(resolve_from(&dir.path().join("goble-app"), "goble-cli"), bare);
    }
}
