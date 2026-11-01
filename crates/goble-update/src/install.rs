//! What to do with a verified artifact, per platform.
//!
//! Planning is separate from doing. [`plan_install`] is a pure decision over
//! the platform, the running executable and the artifact, so the interesting
//! cases — "this is a Homebrew install", "this bundle is not where I think it
//! is", "a package manager owns this file" — are testable without touching a
//! real installation.
//!
//! Applying is deliberately dumb: a rename, a spawned vendor installer, or a
//! shell script that waits for this process to exit. No privileged helper, no
//! daemon, no unsafe code.
//!
//! # Reversible
//!
//! Before anything is replaced, the current copy goes into
//! `~/.goble/updates/previous/` and a `pending` marker names the version being
//! installed. A build that stays up for [`HEALTHY_AFTER`] writes `healthy` and
//! clears `pending`; a build that never gets there is undone at the next start
//! by [`UpdatePaths::recover`], which puts the copy back and records the failed
//! version in `~/.goble/update-state.json`. The original is copied, never
//! moved, so a failed install cannot leave the user without a binary.
//!
//! # Known limitation
//!
//! The macOS bundle swap is not atomic. It moves the old bundle aside and puts
//! the new one in place, which leaves a window where `Goble.app` does not
//! exist. macOS offers an atomic swap (`renamex_np` with `RENAME_SWAP`), but it
//! requires `unsafe`, which this workspace denies. The window is two renames
//! wide and the script reports what it did to a log file; see
//! `.agents/07-observability/` for the follow-up.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::client::StagedArtifact;
use crate::state::UpdateState;

/// How long a freshly installed build must stay up before it is called healthy.
/// A build that cannot survive its own startup is exactly the build the
/// rollback exists for.
pub const HEALTHY_AFTER: Duration = Duration::from_secs(60);

/// Extra flags the Windows installer understands, kept in one place because the
/// updater and the packaging script both depend on them.
pub const WINDOWS_SILENT_FLAGS: [&str; 4] = ["/SILENT", "/NORESTART", "/NOCLOSEAPPLICATIONS", "/update=1"];

/// The machine an update is being planned for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallContext {
    pub os: String,
    /// The running executable, used to find the installation it belongs to.
    pub current_exe: PathBuf,
    /// `$APPIMAGE`, when this build is running from one.
    pub appimage: Option<PathBuf>,
    /// Apple team id; when set, the mounted bundle must be signed by it.
    pub macos_team_id: Option<String>,
}

impl InstallContext {
    /// Read the platform from the process environment.
    pub fn detect() -> Self {
        Self {
            os: std::env::consts::OS.to_owned(),
            current_exe: std::env::current_exe().unwrap_or_default(),
            appimage: std::env::var_os("APPIMAGE").map(PathBuf::from),
            macos_team_id: std::env::var("GOBLE_APPLE_TEAM_ID").ok(),
        }
    }

    pub fn for_os(os: impl Into<String>, current_exe: impl Into<PathBuf>) -> Self {
        Self {
            os: os.into(),
            current_exe: current_exe.into(),
            appimage: None,
            macos_team_id: None,
        }
    }
}

/// How an artifact would be installed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallPlan {
    /// Mount the disk image, check the bundle's signature, and swap it in.
    ReplaceAppBundle { dmg: PathBuf, target: PathBuf, team_id: Option<String> },
    /// Replace the AppImage file in place.
    ReplaceAppImage { staged: PathBuf, target: PathBuf },
    /// Run the vendor installer, which handles the swap itself.
    RunInstaller { installer: PathBuf },
    /// This install belongs to something else; the user runs a command.
    Manual { command: String, hint: String },
}

impl InstallPlan {
    /// One line a UI can show before asking the user to continue.
    pub fn description(&self) -> String {
        match self {
            Self::ReplaceAppBundle { target, .. } => {
                format!("Replace {} and restart", target.display())
            }
            Self::ReplaceAppImage { target, .. } => {
                format!("Replace {} and restart", target.display())
            }
            Self::RunInstaller { installer } => {
                format!("Run the installer {} and restart", installer.display())
            }
            Self::Manual { command, hint } => format!("Run `{command}` to update. {hint}"),
        }
    }

    /// Whether applying this plan restarts the app, or leaves it to the user.
    pub fn needs_restart(&self) -> bool {
        !matches!(self, Self::Manual { .. })
    }
}

/// What applying did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The update is in place or in progress. The caller should exit.
    Applied { restart_required: bool },
    /// Nothing was touched.
    NeedsUser { command: String, hint: String },
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("this artifact ({name}) cannot be installed on {os}")]
    WrongArtifact { name: String, os: String },
    #[error("{path} could not be replaced: {reason}")]
    Io { path: PathBuf, reason: String },
    #[error("could not start {program}: {reason}")]
    Spawn { program: String, reason: String },
    #[error("there is no previous copy of {path} to fall back to")]
    NoBackup { path: PathBuf },
    #[error("the update markers could not be read: {path}: {reason}")]
    Marker { path: PathBuf, reason: String },
    #[error("the update state could not be written: {0}")]
    State(String),
}

/// Where the update markers live: `<goble home>/updates`.
///
/// | File | Meaning |
/// | --- | --- |
/// | `pending` | a version was staged and swapped in, and has not proved itself |
/// | `healthy` | the staged version ran for [`HEALTHY_AFTER`] |
/// | `previous/<name>` | the copy of what the swap replaced |
/// | `downloads/` | staged artifacts, before they are installed |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePaths {
    root: PathBuf,
}

impl UpdatePaths {
    pub fn under(goble_home: impl AsRef<Path>) -> Self {
        Self { root: goble_home.as_ref().join("updates") }
    }

    /// `~/.goble/updates`, or an error when there is no home directory.
    pub fn detect() -> Result<Self, InstallError> {
        crate::home_dir().map(Self::under).ok_or_else(|| InstallError::Io {
            path: PathBuf::from("~/.goble/updates"),
            reason: "the home directory could not be determined".to_owned(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where downloads are staged before they are verified and installed.
    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    pub fn previous_dir(&self) -> PathBuf {
        self.root.join("previous")
    }

    pub fn pending_path(&self) -> PathBuf {
        self.root.join("pending")
    }

    pub fn healthy_path(&self) -> PathBuf {
        self.root.join("healthy")
    }

    /// `<goble home>/update-state.json`, beside the marker directory.
    pub fn state_path(&self) -> PathBuf {
        self.root
            .parent()
            .map(UpdateState::path_in)
            .unwrap_or_else(|| UpdateState::path_in(&self.root))
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy_path().is_file()
    }

    /// What is waiting to prove itself, if anything.
    pub fn pending(&self) -> Result<Option<PendingInstall>, InstallError> {
        let path = self.pending_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(InstallError::Marker { path, reason: err.to_string() });
            }
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|err| InstallError::Marker { path, reason: err.to_string() })
    }

    /// Write the pending marker. Called before the swap, so a build that never
    /// reaches its healthy marker can be undone.
    pub fn write_pending(&self, pending: &PendingInstall) -> Result<(), InstallError> {
        std::fs::create_dir_all(&self.root).map_err(|err| io_error(&self.root, err))?;
        let path = self.pending_path();
        let text = serde_json::to_string_pretty(pending)
            .map_err(|err| InstallError::Marker { path: path.clone(), reason: err.to_string() })?;
        let mut bytes = text.into_bytes();
        bytes.push(b'\n');
        write_atomic(&path, &bytes)
    }

    /// Write the healthy marker and clear the pending one.
    ///
    /// This is the moment an install counts. Until it happens the update state
    /// keeps naming the previous version; [`Self::recover`] undoes the swap when
    /// the app is started again without it.
    pub fn mark_healthy(&self) -> Result<Option<PendingInstall>, InstallError> {
        std::fs::create_dir_all(&self.root).map_err(|err| io_error(&self.root, err))?;
        let healthy = self.healthy_path();
        let at = Utc::now().to_rfc3339();
        write_atomic(&healthy, format!("{at}\n").as_bytes())?;

        let pending = self.pending()?;
        if pending.is_some() {
            let path = self.pending_path();
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(io_error(&path, err)),
            }
        }
        Ok(pending)
    }

    /// Copy what is about to be replaced into `previous/`, and return the copy's
    /// path. The copy is what a rollback restores; the original is never moved,
    /// so a failure here leaves the running build untouched.
    pub fn backup(&self, target: &Path) -> Result<PathBuf, InstallError> {
        let name = target
            .file_name()
            .ok_or_else(|| InstallError::NoBackup { path: target.to_path_buf() })?;
        let previous_dir = self.previous_dir();
        std::fs::create_dir_all(&previous_dir).map_err(|err| io_error(&previous_dir, err))?;
        let destination = previous_dir.join(name);

        // Backing up a path that is already the backup would delete it first.
        if destination == target {
            return Ok(destination);
        }
        match std::fs::remove_dir_all(&destination) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                let _ = std::fs::remove_file(&destination);
            }
        }

        if target.is_dir() {
            copy_tree(target, &destination)?;
        } else if target.is_file() {
            std::fs::copy(target, &destination).map_err(|err| io_error(target, err))?;
        } else {
            return Err(InstallError::NoBackup { path: target.to_path_buf() });
        }

        if !destination.exists() {
            return Err(InstallError::NoBackup { path: target.to_path_buf() });
        }
        Ok(destination)
    }

    /// Undo a swap that never reached its healthy marker.
    ///
    /// Called at startup, before the UI exists, so a build that cannot run its
    /// own window is not the build the user is left with. The failed version is
    /// recorded in the update state.
    pub fn recover(&self) -> Result<LocalRollback, InstallError> {
        let Some(pending) = self.pending()? else {
            return Ok(LocalRollback::None);
        };

        // A healthy marker means the swap was accepted; the pending file is
        // then a leftover from a crash between the two writes.
        if self.is_healthy() {
            let path = self.pending_path();
            let _ = std::fs::remove_file(path);
            return Ok(LocalRollback::None);
        }

        let previous = match pending.previous.clone() {
            Some(previous) if previous.exists() => previous,
            _ => {
                // Nothing to put back: keep the failed version visible instead
                // of pretending the update succeeded.
                self.record_failure(&pending)?;
                let _ = std::fs::remove_file(self.pending_path());
                return Ok(LocalRollback::NothingToRestore {
                    failed_version: pending.version,
                    failed_epoch: pending.epoch,
                });
            }
        };

        restore(&previous, &pending.target)?;

        let _ = std::fs::remove_file(self.healthy_path());
        let _ = std::fs::remove_file(self.pending_path());
        self.record_failure(&pending)?;

        Ok(LocalRollback::Restored {
            failed_version: pending.version,
            failed_epoch: pending.epoch,
            target: pending.target,
        })
    }

    fn record_failure(&self, pending: &PendingInstall) -> Result<(), InstallError> {
        let path = self.state_path();
        let mut state = UpdateState::load(&path).map_err(|err| InstallError::State(err.to_string()))?;
        state.record_failed(pending.version.clone(), pending.epoch);
        state.save(&path).map_err(|err| InstallError::State(err.to_string()))
    }
}

/// The update that is staged and waiting to prove itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingInstall {
    pub version: String,
    pub epoch: u64,
    /// The path the swap replaces.
    pub target: PathBuf,
    /// The copy of what was there before, under `previous/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<PathBuf>,
    /// `app-bundle` or `appimage`.
    pub kind: String,
}

impl PendingInstall {
    pub fn for_appimage(version: impl Into<String>, epoch: u64, target: PathBuf) -> Self {
        Self {
            version: version.into(),
            epoch,
            target,
            previous: None,
            kind: "appimage".to_owned(),
        }
    }

    pub fn for_bundle(version: impl Into<String>, epoch: u64, target: PathBuf) -> Self {
        Self {
            version: version.into(),
            epoch,
            target,
            previous: None,
            kind: "app-bundle".to_owned(),
        }
    }

    pub fn with_previous(mut self, previous: Option<PathBuf>) -> Self {
        self.previous = previous;
        self
    }
}

/// What [`UpdatePaths::recover`] did at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalRollback {
    /// There was no unfinished install.
    None,
    /// The previous copy is back in place; the failed version is recorded.
    Restored { failed_version: String, failed_epoch: u64, target: PathBuf },
    /// An install never became healthy, and there was no copy to restore.
    NothingToRestore { failed_version: String, failed_epoch: u64 },
}

impl LocalRollback {
    /// Whether the app should hand over to the restored build.
    pub fn restart_required(&self) -> bool {
        matches!(self, Self::Restored { .. })
    }

    pub fn message(&self) -> Option<String> {
        match self {
            Self::None => None,
            Self::Restored { failed_version, .. } => Some(format!(
                "Goble {failed_version} did not start cleanly, so the previous version was restored"
            )),
            Self::NothingToRestore { failed_version, .. } => Some(format!(
                "Goble {failed_version} did not start cleanly, and there was no previous copy to restore"
            )),
        }
    }
}

/// Undo an install that never reached its healthy marker, using the default
/// `~/.goble/updates` directory.
///
/// This is the call the app makes at startup, before the UI exists: a build
/// that swaps itself in and cannot run its own window is undone by the next
/// launch, which puts the previous copy back and hands over to it.
pub fn rollback_local() -> Result<LocalRollback, InstallError> {
    UpdatePaths::detect()?.recover()
}

/// What an install is for: where the markers live, and the version and epoch
/// that are being put in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub paths: UpdatePaths,
    pub version: String,
    pub epoch: u64,
}

impl InstallRequest {
    pub fn new(paths: UpdatePaths, version: impl Into<String>, epoch: u64) -> Self {
        Self { paths, version: version.into(), epoch }
    }
}

/// Decide how to install a staged artifact.
pub fn plan_install(
    context: &InstallContext,
    staged: &StagedArtifact,
) -> Result<InstallPlan, InstallError> {
    let staged_path = staged.path.clone();
    let name = staged.file_name();
    let extension = staged_path.extension().map(|ext| ext.to_string_lossy().to_lowercase());

    match context.os.as_str() {
        "macos" if extension.as_deref() == Some("dmg") => {
            match bundle_root(&context.current_exe) {
                Some(target) => Ok(InstallPlan::ReplaceAppBundle {
                    dmg: staged_path,
                    target,
                    team_id: context.macos_team_id.clone(),
                }),
                None => Ok(InstallPlan::Manual {
                    command: format!("open '{}'", staged_path.display()),
                    hint: "This copy is not inside a Goble.app bundle, so it cannot replace itself; drag the app out of the disk image.".to_owned(),
                }),
            }
        }
        "linux" => {
            if let Some(appimage) = &context.appimage {
                return Ok(InstallPlan::ReplaceAppImage {
                    staged: staged_path,
                    target: appimage.clone(),
                });
            }
            Ok(package_manager_plan(&staged_path, extension.as_deref()))
        }
        "windows" if matches!(extension.as_deref(), Some("exe") | Some("msi")) => {
            Ok(InstallPlan::RunInstaller { installer: staged_path })
        }
        _ => Ok(InstallPlan::Manual {
            command: format!("open '{}'", staged_path.display()),
            hint: format!("Install {name} by hand; this build cannot replace itself on {}.", context.os),
        }),
    }
}

/// A `.deb`, `.rpm` or `.AppImage` that someone else owns.
fn package_manager_plan(staged: &Path, extension: Option<&str>) -> InstallPlan {
    let path = staged.display().to_string();
    match extension {
        Some("deb") => InstallPlan::Manual {
            command: format!("sudo apt install '{path}'"),
            hint: "The system package manager owns this installation, so it stays in charge of it."
                .to_owned(),
        },
        Some("rpm") => InstallPlan::Manual {
            command: format!("sudo dnf install '{path}'"),
            hint: "The system package manager owns this installation, so it stays in charge of it."
                .to_owned(),
        },
        Some("appimage") => InstallPlan::Manual {
            command: format!("chmod +x '{path}' && ./'{}'", staged.file_name().unwrap_or_default().to_string_lossy()),
            hint: "This copy is not running as an AppImage, so it cannot replace itself.".to_owned(),
        },
        _ => InstallPlan::Manual {
            command: format!("open '{path}'"),
            hint: "Install this archive by hand.".to_owned(),
        },
    }
}

/// The `*.app` bundle a path belongs to, if any.
pub fn bundle_root(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|ancestor| ancestor.extension().is_some_and(|extension| extension == "app"))
        .map(Path::to_path_buf)
}

impl InstallPlan {
    /// Do it, keeping what is replaced recoverable.
    ///
    /// The current copy goes into `previous/` and the pending marker names the
    /// version being installed *before* anything is swapped, so a build that
    /// never reaches its healthy marker can be undone by
    /// [`UpdatePaths::recover`]. The original is only ever copied, never moved,
    /// so a failed install leaves the running build where it is.
    pub fn apply(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError> {
        match self {
            Self::ReplaceAppImage { staged, target } => {
                let previous = request.paths.backup(target)?;
                let pending =
                    PendingInstall::for_appimage(&request.version, request.epoch, target.clone())
                        .with_previous(Some(previous));
                request.paths.write_pending(&pending)?;
                if let Err(err) = replace_file(staged, target) {
                    // Nothing was swapped, so there is nothing to undo.
                    let _ = std::fs::remove_file(request.paths.pending_path());
                    return Err(err);
                }
                Ok(InstallOutcome::Applied { restart_required: true })
            }
            Self::RunInstaller { installer } => {
                let mut command = Command::new(installer);
                command.args(WINDOWS_SILENT_FLAGS);
                if let Some(directory) = installer.parent() {
                    command.arg(format!("/DIR={}", directory.display()));
                }
                command.spawn().map_err(|err| InstallError::Spawn {
                    program: installer.display().to_string(),
                    reason: err.to_string(),
                })?;
                Ok(InstallOutcome::Applied { restart_required: true })
            }
            Self::ReplaceAppBundle { dmg, target, team_id } => {
                let previous = request.paths.backup(target)?;
                let pending =
                    PendingInstall::for_bundle(&request.version, request.epoch, target.clone())
                        .with_previous(Some(previous));
                request.paths.write_pending(&pending)?;
                let script = bundle_swap_script(dmg, target, std::process::id(), team_id.as_deref());
                spawn_script(&script)?;
                Ok(InstallOutcome::Applied { restart_required: true })
            }
            Self::Manual { command, hint } => {
                Ok(InstallOutcome::NeedsUser { command: command.clone(), hint: hint.clone() })
            }
        }
    }
}

/// Write a file so a reader sees either the old bytes or the new ones.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), InstallError> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes).map_err(|err| io_error(&temporary, err))?;
    std::fs::rename(&temporary, path).map_err(|err| io_error(path, err))
}

/// Copy a file or a directory tree. Symlinks are followed: a bundle's internal
/// links point inside the bundle, and a rollback copy that keeps them dangling
/// is not a bundle that runs.
fn copy_tree(from: &Path, to: &Path) -> Result<(), InstallError> {
    std::fs::create_dir_all(to).map_err(|err| io_error(to, err))?;
    let entries = std::fs::read_dir(from).map_err(|err| io_error(from, err))?;
    for entry in entries {
        let entry = entry.map_err(|err| io_error(from, err))?;
        let source = entry.path();
        let destination = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &destination)?;
        } else {
            std::fs::copy(&source, &destination).map_err(|err| io_error(&source, err))?;
        }
    }
    Ok(())
}

/// Put `previous` back over `target`, leaving at least one working copy at every
/// step: the copy is made beside the target and renamed into place.
fn restore(previous: &Path, target: &Path) -> Result<(), InstallError> {
    let name = target
        .file_name()
        .ok_or_else(|| InstallError::NoBackup { path: target.to_path_buf() })?;
    let failed = target.with_file_name(format!("{}.failed", name.to_string_lossy()));

    if previous.is_dir() {
        let _ = std::fs::remove_dir_all(&failed);
        if target.exists() {
            std::fs::rename(target, &failed).map_err(|err| io_error(target, err))?;
        }
        copy_tree(previous, target)?;
        let _ = std::fs::remove_dir_all(&failed);
        return Ok(());
    }

    let mut staged = target.as_os_str().to_owned();
    staged.push(".rollback");
    let staged = PathBuf::from(staged);
    std::fs::copy(previous, &staged).map_err(|err| io_error(&staged, err))?;
    make_executable(&staged)?;
    std::fs::rename(&staged, target).map_err(|err| io_error(target, err))
}


/// Replace one file with another, across a device boundary if need be.
///
/// The replaced file is always left executable: an AppImage that loses its
/// executable bit is an AppImage the user cannot start again.
fn replace_file(staged: &Path, target: &Path) -> Result<(), InstallError> {
    if std::fs::rename(staged, target).is_ok() {
        return make_executable(target);
    }

    // A different filesystem: copy, then mark executable, then remove the
    // original. The copy lands next to the target so the final rename is cheap.
    let temporary = {
        let mut name = target.as_os_str().to_owned();
        name.push(".new");
        PathBuf::from(name)
    };
    std::fs::copy(staged, &temporary)
        .map_err(|err| io_error(&temporary, err))?;
    make_executable(&temporary)?;
    std::fs::rename(&temporary, target).map_err(|err| io_error(target, err))?;
    let _ = std::fs::remove_file(staged);
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), InstallError> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = std::fs::metadata(path).map_err(|err| io_error(path, err))?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).map_err(|err| io_error(path, err))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), InstallError> {
    Ok(())
}

fn io_error(path: &Path, err: std::io::Error) -> InstallError {
    InstallError::Io { path: path.to_path_buf(), reason: err.to_string() }
}

/// Start a detached shell that waits for this process to exit and then swaps
/// the bundle. Detached because the script has to outlive the process it is
/// replacing.
#[cfg(unix)]
fn spawn_script(script: &str) -> Result<(), InstallError> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| InstallError::Spawn { program: "/bin/sh".to_owned(), reason: err.to_string() })
}

#[cfg(not(unix))]
fn spawn_script(script: &str) -> Result<(), InstallError> {
    Err(InstallError::Spawn {
        program: "/bin/sh".to_owned(),
        reason: format!("no shell to run the swap script on this platform: {script}"),
    })
}

/// The macOS swap, as a script, because the process doing the swapping must
/// outlive the process being swapped.
///
/// `ditto` keeps the bundle's metadata and permissions, which a plain `cp -r`
/// does not; that matters for a signed bundle.
pub fn bundle_swap_script(dmg: &Path, target: &Path, pid: u32, team_id: Option<&str>) -> String {
    let log = target.with_file_name("goble-update.log");
    let mount = target.with_file_name("goble-update-mount");
    let backup = target.with_file_name("Goble.app.old");
    let bundle_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Goble.app".to_owned());

    let signature_check = match team_id {
        Some(team_id) => format!(
            r#"
  if ! /usr/bin/codesign -v -R='certificate leaf[subject.OU] = "{team_id}"' "$SOURCE"; then
    echo "the downloaded bundle is not signed by {team_id}" >> "{log}"
    exit 3
  fi"#,
            team_id = team_id,
            log = log.display()
        ),
        None => String::new(),
    };

    format!(
        r#"#!/bin/sh
# Written by goble at update time. Waits for the old process to exit, then puts
# the new bundle in place. Log: {log}
exec >> "{log}" 2>&1
echo "waiting for pid {pid} to exit"
while kill -0 {pid} 2>/dev/null; do sleep 0.2; done

MOUNT="{mount}"
DMG="{dmg}"
TARGET="{target}"
SOURCE="$MOUNT/{bundle_name}"
BACKUP="{backup}"

/usr/bin/hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT" "$DMG" || {{ echo "could not mount $DMG"; exit 1; }}
if [ ! -d "$SOURCE" ]; then echo "no $SOURCE in the image"; /usr/bin/hdiutil detach "$MOUNT"; exit 2; fi
{signature_check}
STAGING="$TARGET.new"
rm -rf "$STAGING" "$BACKUP"
/usr/bin/ditto "$SOURCE" "$STAGING" || {{ echo "could not stage the new bundle"; /usr/bin/hdiutil detach "$MOUNT"; exit 4; }}
/usr/bin/hdiutil detach "$MOUNT"

# Two renames rather than one atomic swap: see the module docs for why.
mv "$TARGET" "$BACKUP" || {{ echo "could not move the old bundle aside"; exit 5; }}
mv "$STAGING" "$TARGET" || {{ echo "could not install the new bundle"; mv "$BACKUP" "$TARGET"; exit 6; }}
rm -rf "$BACKUP"
sleep 0.5
/usr/bin/open -n "$TARGET"
echo "update complete"
"#,
        log = log.display(),
        pid = pid,
        mount = mount.display(),
        dmg = dmg.display(),
        target = target.display(),
        bundle_name = bundle_name,
        backup = backup.display(),
        signature_check = signature_check
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::StagedArtifact;
    use crate::manifest::Artifact;
    use std::path::PathBuf;

    fn staged(path: &str) -> StagedArtifact {
        StagedArtifact {
            path: PathBuf::from(path),
            artifact: Artifact {
                os: "macos".into(),
                arch: "aarch64".into(),
                url: format!("https://releases.goble.dev/stable/0.2.0/{path}"),
                sha256: "a".repeat(64),
                size: 10,
            },
            digest: "a".repeat(64),
        }
    }

    #[test]
    fn a_bundle_is_found_from_the_executable_inside_it() {
        let exe = Path::new("/Applications/Goble.app/Contents/MacOS/goble-app");
        assert_eq!(bundle_root(exe), Some(PathBuf::from("/Applications/Goble.app")));
        assert_eq!(bundle_root(Path::new("/usr/local/bin/goble-app")), None);
    }

    #[test]
    fn a_dmg_next_to_a_bundle_is_planned_as_a_swap() {
        let context = InstallContext::for_os(
            "macos",
            "/Applications/Goble.app/Contents/MacOS/goble-app",
        );
        let plan = plan_install(&context, &staged("Goble-0.2.0-arm64.dmg")).unwrap();
        assert_eq!(
            plan,
            InstallPlan::ReplaceAppBundle {
                dmg: PathBuf::from("Goble-0.2.0-arm64.dmg"),
                target: PathBuf::from("/Applications/Goble.app"),
                team_id: None,
            }
        );
        assert!(plan.needs_restart());
        assert!(plan.description().contains("/Applications/Goble.app"));
    }

    #[test]
    fn a_dmg_run_outside_a_bundle_asks_the_user() {
        let context = InstallContext::for_os("macos", "/Users/ana/bin/goble-app");
        let plan = plan_install(&context, &staged("Goble-0.2.0-arm64.dmg")).unwrap();
        assert!(matches!(plan, InstallPlan::Manual { .. }), "{plan:?}");
        assert!(!plan.needs_restart());
    }

    #[test]
    fn an_appimage_replaces_itself() {
        let mut context = InstallContext::for_os("linux", "/home/ana/Applications/Goble.AppImage");
        context.appimage = Some(PathBuf::from("/home/ana/Applications/Goble.AppImage"));
        let plan = plan_install(&context, &staged("/tmp/updates/Goble-0.2.0-x86_64.AppImage")).unwrap();
        assert_eq!(
            plan,
            InstallPlan::ReplaceAppImage {
                staged: PathBuf::from("/tmp/updates/Goble-0.2.0-x86_64.AppImage"),
                target: PathBuf::from("/home/ana/Applications/Goble.AppImage"),
            }
        );
    }

    #[test]
    fn a_package_managed_linux_install_is_handed_to_the_package_manager() {
        let context = InstallContext::for_os("linux", "/usr/bin/goble-app");

        let plan = plan_install(&context, &staged("/tmp/updates/goble_0.2.0_amd64.deb")).unwrap();
        assert!(matches!(plan, InstallPlan::Manual { .. }), "{plan:?}");
        assert!(plan.description().contains("apt install"), "{}", plan.description());

        let plan = plan_install(&context, &staged("/tmp/updates/goble-0.2.0.x86_64.rpm")).unwrap();
        assert!(plan.description().contains("dnf install"), "{}", plan.description());

        let plan = plan_install(&context, &staged("/tmp/updates/Goble-0.2.0-x86_64.AppImage")).unwrap();
        assert!(plan.description().contains("chmod +x"), "{}", plan.description());
    }

    #[test]
    fn a_windows_installer_is_run() {
        let context = InstallContext::for_os("windows", r"C:\Users\ana\AppData\Local\Goble\goble-app.exe");
        let plan = plan_install(&context, &staged(r"C:\Temp\GobleSetup.exe")).unwrap();
        assert_eq!(plan, InstallPlan::RunInstaller { installer: PathBuf::from(r"C:\Temp\GobleSetup.exe") });
        assert!(plan.needs_restart());
    }

    #[test]
    fn an_artifact_for_another_platform_is_never_installed() {
        let context = InstallContext::for_os("linux", "/usr/bin/goble-app");
        let plan = plan_install(&context, &staged("/tmp/updates/Goble-0.2.0-arm64.dmg")).unwrap();
        assert!(matches!(plan, InstallPlan::Manual { .. }), "{plan:?}");
    }

    fn request(paths: &UpdatePaths, version: &str, epoch: u64) -> InstallRequest {
        InstallRequest::new(paths.clone(), version, epoch)
    }

    #[test]
    fn a_manual_plan_hands_the_command_to_the_caller() {
        let dir = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(dir.path());
        let plan = InstallPlan::Manual {
            command: "sudo apt install '/tmp/x.deb'".into(),
            hint: "the package manager owns this".into(),
        };
        assert_eq!(
            plan.apply(&request(&paths, "0.2.0", 7)).unwrap(),
            InstallOutcome::NeedsUser {
                command: "sudo apt install '/tmp/x.deb'".into(),
                hint: "the package manager owns this".into(),
            }
        );
    }

    #[test]
    fn replacing_an_appimage_puts_the_new_bytes_in_place() {
        let staging = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());

        let staged_path = staging.path().join("Goble-0.2.0-x86_64.AppImage");
        std::fs::write(&staged_path, b"the new build").unwrap();
        let target = install.path().join("Goble.AppImage");
        std::fs::write(&target, b"the old build").unwrap();

        let plan = InstallPlan::ReplaceAppImage { staged: staged_path.clone(), target: target.clone() };
        assert_eq!(
            plan.apply(&request(&paths, "0.2.0", 7)).unwrap(),
            InstallOutcome::Applied { restart_required: true }
        );

        // The replaced bytes are kept, and the pending marker names what is in
        // place now.
        let previous = paths.previous_dir().join("Goble.AppImage");
        assert_eq!(std::fs::read(&previous).unwrap(), b"the old build");
        let pending = paths.pending().unwrap().expect("a pending marker");
        assert_eq!(pending.version, "0.2.0");
        assert_eq!(pending.epoch, 7);
        assert_eq!(pending.previous.as_deref(), Some(previous.as_path()));

        assert_eq!(std::fs::read(&target).unwrap(), b"the new build");
        assert!(!staged_path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn replacing_a_file_across_directories_still_works() {
        let staging = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();

        let staged_path = staging.path().join("staged");
        std::fs::write(&staged_path, b"new").unwrap();
        let target = install.path().join("target");

        replace_file(&staged_path, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!staged_path.exists());
    }

    #[test]
    fn the_swap_script_waits_for_this_process_and_cleans_up() {
        let script = bundle_swap_script(
            Path::new("/tmp/updates/Goble-0.2.0-arm64.dmg"),
            Path::new("/Applications/Goble.app"),
            4242,
            Some("ABCDE12345"),
        );

        assert!(script.contains("while kill -0 4242"), "{script}");
        assert!(script.contains("hdiutil attach"), "{script}");
        assert!(script.contains("/tmp/updates/Goble-0.2.0-arm64.dmg"), "{script}");
        assert!(script.contains("Goble.app.old"), "{script}");
        assert!(script.contains("leaf[subject.OU] = \"ABCDE12345\""), "{script}");
        assert!(script.contains("/usr/bin/open -n"), "{script}");
        assert!(script.trim_end().ends_with("echo \"update complete\""), "{script}");
    }

    #[test]
    fn the_swap_script_skips_the_signature_check_without_a_team_id() {
        let script = bundle_swap_script(
            Path::new("/tmp/x.dmg"),
            Path::new("/Applications/Goble.app"),
            1,
            None,
        );
        assert!(!script.contains("codesign"), "{script}");
    }

    /// A fake bundle layout: `Goble.app/Contents/MacOS/goble-app` plus a marker
    /// file whose contents say which build it is.
    fn fake_bundle(root: &Path, build: &str) -> PathBuf {
        let bundle = root.join("Goble.app");
        let macos = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(macos.join("goble-app"), build).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(
                macos.join("goble-app"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        bundle
    }

    fn build_of(bundle: &Path) -> String {
        std::fs::read_to_string(bundle.join("Contents/MacOS/goble-app")).unwrap()
    }

    #[test]
    fn a_backup_copies_a_bundle_and_leaves_the_original_alone() {
        let home = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());
        let bundle = fake_bundle(install.path(), "0.1.0");

        let backup = paths.backup(&bundle).unwrap();
        assert!(backup.is_dir());
        assert_eq!(build_of(&backup), "0.1.0");
        assert_eq!(build_of(&bundle), "0.1.0", "the running bundle is never moved");
    }

    #[test]
    fn a_missing_target_is_never_replaced() {
        let home = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());

        let staged = staging.path().join("Goble.AppImage");
        std::fs::write(&staged, b"new").unwrap();
        let target = staging.path().join("does-not-exist");

        let plan = InstallPlan::ReplaceAppImage { staged: staged.clone(), target };
        let error = plan.apply(&request(&paths, "0.2.0", 7)).unwrap_err();
        assert!(matches!(error, InstallError::NoBackup { .. }), "{error}");
        assert_eq!(std::fs::read(&staged).unwrap(), b"new", "the staged build is kept");
        assert!(paths.pending().unwrap().is_none());
    }

    #[test]
    fn a_healthy_install_clears_the_pending_marker() {
        let home = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());
        let pending = PendingInstall::for_appimage("0.2.0", 7, PathBuf::from("/tmp/Goble.AppImage"));
        paths.write_pending(&pending).unwrap();

        assert!(paths.pending().unwrap().is_some());
        assert!(!paths.is_healthy());

        let confirmed = paths.mark_healthy().unwrap().expect("the pending install");
        assert_eq!(confirmed.version, "0.2.0");
        assert!(paths.is_healthy());
        assert!(paths.pending().unwrap().is_none());
    }

    #[test]
    fn a_build_that_never_became_healthy_is_rolled_back_at_startup() {
        let home = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());

        // 0.1.0 was installed and accepted; 0.2.1 was swapped in and never
        // reached its healthy marker.
        let target = install.path().join("Goble.AppImage");
        std::fs::write(&target, b"the broken build").unwrap();
        let previous = paths.previous_dir().join("Goble.AppImage");
        std::fs::create_dir_all(paths.previous_dir()).unwrap();
        std::fs::write(&previous, b"the good build").unwrap();
        paths
            .write_pending(
                &PendingInstall::for_appimage("0.2.1", 8, target.clone())
                    .with_previous(Some(previous)),
            )
            .unwrap();

        let outcome = paths.recover().unwrap();
        assert_eq!(
            outcome,
            LocalRollback::Restored {
                failed_version: "0.2.1".into(),
                failed_epoch: 8,
                target: target.clone(),
            }
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"the good build");
        assert!(outcome.restart_required());
        assert!(paths.pending().unwrap().is_none());
        assert!(!paths.is_healthy());

        let state = UpdateState::load(&paths.state_path()).unwrap();
        assert_eq!(state.failed_version.as_deref(), Some("0.2.1"));
        assert_eq!(state.failed_epoch, 8);

        // Nothing is left to do on the next launch.
        assert_eq!(paths.recover().unwrap(), LocalRollback::None);
    }

    #[test]
    fn a_bundle_that_never_became_healthy_is_restored() {
        let home = tempfile::tempdir().unwrap();
        let install = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());
        let target = fake_bundle(install.path(), "broken");

        let previous = paths.previous_dir().join("Goble.app");
        std::fs::create_dir_all(paths.previous_dir()).unwrap();
        copy_tree(&fake_bundle(&home.path().join("good"), "good"), &previous).unwrap();
        paths
            .write_pending(
                &PendingInstall::for_bundle("0.2.1", 8, target.clone())
                    .with_previous(Some(previous)),
            )
            .unwrap();

        assert!(matches!(paths.recover().unwrap(), LocalRollback::Restored { .. }));
        assert_eq!(build_of(&target), "good");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(target.join("Contents/MacOS/goble-app"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "a restored app still has to run");
        }
    }

    #[test]
    fn a_recovered_install_with_nothing_to_restore_stays_visible() {
        let home = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());
        paths
            .write_pending(&PendingInstall::for_appimage(
                "0.2.1",
                8,
                home.path().join("Goble.AppImage"),
            ))
            .unwrap();

        let outcome = paths.recover().unwrap();
        assert_eq!(
            outcome,
            LocalRollback::NothingToRestore {
                failed_version: "0.2.1".into(),
                failed_epoch: 8,
            }
        );
        assert!(!outcome.restart_required());
        assert_eq!(
            UpdateState::load(&paths.state_path()).unwrap().failed_version.as_deref(),
            Some("0.2.1")
        );
    }

    #[test]
    fn a_healthy_marker_makes_the_pending_file_a_leftover() {
        let home = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());
        paths
            .write_pending(&PendingInstall::for_appimage("0.2.0", 7, PathBuf::from("/tmp/x")))
            .unwrap();
        std::fs::write(paths.healthy_path(), "2026-10-01T00:00:00Z\n").unwrap();

        assert_eq!(paths.recover().unwrap(), LocalRollback::None);
        assert!(paths.pending().unwrap().is_none());
    }
}
