//! What this installation has accepted, and what it last asked about.
//!
//! The file is `~/.goble/update-state.json`, and it is written by the install
//! path only after the new build reaches its healthy marker (see
//! [`crate::install::UpdatePaths::mark_healthy`]). A version that was installed
//! and never became healthy is recorded here as failed, so the client can stop
//! offering it and the UI can say what happened.
//!
//! ```json
//! {
//!   "installed_version": "0.2.0",
//!   "installed_epoch": 7,
//!   "last_check": "2026-10-01T09:00:00Z",
//!   "failed_version": "0.2.1",
//!   "failed_epoch": 8
//! }
//! ```

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::client::UpdateError;

/// The installed epoch a channel release is compared against when the state
/// file does not say: a first install of a released build.
pub const UNKNOWN_EPOCH: u64 = 0;

/// The persisted update state of this installation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    /// The version running now, recorded once it was healthy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    /// The publication epoch that version came from.
    #[serde(default)]
    pub installed_epoch: u64,
    /// When the client last asked the release host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<DateTime<Utc>>,
    /// A version that was installed and did not reach its healthy marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_version: Option<String>,
    #[serde(default)]
    pub failed_epoch: u64,
}

impl UpdateState {
    /// `<home>/update-state.json`.
    pub fn path_in(home: &Path) -> PathBuf {
        home.join("update-state.json")
    }

    /// `~/.goble/update-state.json`, or `None` when the home directory cannot
    /// be found.
    pub fn default_path() -> Option<PathBuf> {
        crate::home_dir().map(|home| Self::path_in(&home))
    }

    /// Read the state, or the empty state when there is no file yet.
    ///
    /// A file that cannot be parsed is reported and replaced by the empty
    /// state: a broken bookkeeping file must not stop a working build from
    /// checking for updates.
    pub fn load(path: &Path) -> Result<Self, UpdateError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(UpdateError::Io { path: path.to_path_buf(), reason: err.to_string() }),
        };
        match serde_json::from_str(&text) {
            Ok(state) => Ok(state),
            Err(err) => {
                log::warn!(
                    "{} is not readable as update state ({err}); starting from an empty state",
                    path.display()
                );
                Ok(Self::default())
            }
        }
    }

    /// Write the state, replacing the file atomically.
    pub fn save(&self, path: &Path) -> Result<(), UpdateError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| UpdateError::Io { path: parent.to_path_buf(), reason: err.to_string() })?;
        }
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".tmp");
        let temporary = PathBuf::from(temporary);
        let text = serde_json::to_string_pretty(self)
            .map_err(|err| UpdateError::Io { path: path.to_path_buf(), reason: err.to_string() })?;
        std::fs::write(&temporary, text + "\n")
            .map_err(|err| UpdateError::Io { path: temporary.clone(), reason: err.to_string() })?;
        std::fs::rename(&temporary, path)
            .map_err(|err| UpdateError::Io { path: path.to_path_buf(), reason: err.to_string() })
    }

    /// The epoch a check compares a channel release against.
    pub fn installed_epoch(&self) -> u64 {
        self.installed_epoch
    }

    pub fn installed_version(&self) -> Option<&str> {
        self.installed_version.as_deref()
    }

    /// Record that a version is installed and healthy. This is the only place
    /// `installed_version`/`installed_epoch` are written, so a build that was
    /// downloaded but never ran cannot claim to be installed.
    pub fn confirm_install(&mut self, version: impl Into<String>, epoch: u64) {
        self.installed_version = Some(version.into());
        self.installed_epoch = epoch;
        self.failed_version = None;
        self.failed_epoch = 0;
    }

    /// Record that a version was installed and rolled back.
    pub fn record_failed(&mut self, version: impl Into<String>, epoch: u64) {
        self.failed_version = Some(version.into());
        self.failed_epoch = epoch;
    }

    /// Record that the client asked the release host now.
    pub fn record_check(&mut self, at: DateTime<Utc>) {
        self.last_check = Some(at);
    }

    /// Whether a version is the one this installation already failed on.
    pub fn failed_even_after(&self, version: &str, epoch: u64) -> bool {
        self.failed_version.as_deref() == Some(version) && self.failed_epoch == epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_the_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let state = UpdateState::load(&dir.path().join("update-state.json")).unwrap();
        assert_eq!(state, UpdateState::default());
        assert_eq!(state.installed_epoch(), UNKNOWN_EPOCH);
        assert!(state.installed_version().is_none());
    }

    #[test]
    fn the_state_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = UpdateState::path_in(dir.path());

        let mut state = UpdateState::default();
        state.record_check(Utc::now());
        state.confirm_install("0.2.0", 7);
        state.save(&path).unwrap();

        let read = UpdateState::load(&path).unwrap();
        assert_eq!(read.installed_version(), Some("0.2.0"));
        assert_eq!(read.installed_epoch(), 7);
        assert_eq!(read.last_check, state.last_check);
        assert!(read.failed_version.is_none());
    }

    #[test]
    fn only_a_healthy_install_is_recorded_as_installed() {
        let mut state = UpdateState::default();
        state.record_failed("0.2.1", 8);

        // A failed install leaves the installed pair alone.
        assert_eq!(state.installed_epoch(), 0);
        assert_eq!(state.failed_version.as_deref(), Some("0.2.1"));
        assert!(state.failed_even_after("0.2.1", 8));
        assert!(!state.failed_even_after("0.2.1", 9));

        state.confirm_install("0.2.0", 7);
        assert_eq!(state.installed_epoch(), 7);
        assert!(state.failed_version.is_none());
    }

    #[test]
    fn an_unreadable_state_file_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = UpdateState::path_in(dir.path());
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(UpdateState::load(&path).unwrap(), UpdateState::default());
    }

    #[test]
    fn the_default_path_ends_in_the_goble_home() {
        let path = UpdateState::path_in(Path::new("/home/ana/.goble"));
        assert_eq!(path, PathBuf::from("/home/ana/.goble/update-state.json"));
    }
}
