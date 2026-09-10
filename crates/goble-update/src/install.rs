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

use crate::client::StagedArtifact;

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
    /// Do it.
    pub fn apply(&self) -> Result<InstallOutcome, InstallError> {
        match self {
            Self::ReplaceAppImage { staged, target } => {
                replace_file(staged, target)?;
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

    #[test]
    fn a_manual_plan_hands_the_command_to_the_caller() {
        let plan = InstallPlan::Manual {
            command: "sudo apt install '/tmp/x.deb'".into(),
            hint: "the package manager owns this".into(),
        };
        assert_eq!(
            plan.apply().unwrap(),
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

        let staged_path = staging.path().join("Goble-0.2.0-x86_64.AppImage");
        std::fs::write(&staged_path, b"the new build").unwrap();
        let target = install.path().join("Goble.AppImage");
        std::fs::write(&target, b"the old build").unwrap();

        let plan = InstallPlan::ReplaceAppImage { staged: staged_path.clone(), target: target.clone() };
        assert_eq!(plan.apply().unwrap(), InstallOutcome::Applied { restart_required: true });

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
}
