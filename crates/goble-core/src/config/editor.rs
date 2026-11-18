//! The one writer for the settings files.
//!
//! Every settings edit — the Settings tab, the CLI and the one-time imports —
//! goes through [`ConfigEditor::edit`], which follows DeepSeek Harness's
//! `ConfigEditor::edit` protocol:
//!
//! 1. take the per-file in-process mutex **and** an advisory file lock, so two
//!    processes (the app and `goble-cli setup`) cannot interleave;
//! 2. re-read the file from disk and compare its content hash with the revision
//!    the caller rendered from — a mismatch is a [`ConfigError::Conflict`] and
//!    nothing is written;
//! 3. parse the current bytes into a [`toml_edit::DocumentMut`]; a file that is
//!    not TOML is [`ConfigError::RefusedUnparseable`] and is never overwritten;
//! 4. apply the caller's document edit, validate the result and refuse the write
//!    if validation fails or the edit would add a new section problem;
//! 5. serialize, write a temp file in the same directory, `fsync` it, rename it
//!    over the target and `fsync` the directory. The target is mode `0600`.
//!
//! The document's revision is only remembered after the rename, and the written
//! hash is recorded so the file watcher can ignore our own writes.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs2::FileExt;
use parking_lot::Mutex;
use toml_edit::DocumentMut;

use crate::environment::{self, EnvironmentFile};
use crate::hosts::{self, HostsFile};

use super::document::{
    problems_for, ConfigDocument, ConfigError, ConfigFile, ConfigProblem, ConfigRevision,
    FieldError,
};
use super::schema;

/// What an edit produced: the caller's value, the document after the write (or
/// after the no-op, when nothing changed) and whether the file was rewritten.
#[derive(Debug)]
pub struct EditOutcome<T> {
    /// The closure's return value.
    pub value: T,
    /// The document as it now stands on disk (or would, when `written` is false).
    pub document: ConfigDocument,
    /// `false` when the edit produced the same bytes the file already had.
    pub written: bool,
}

impl<T> EditOutcome<T> {
    /// The revision the caller should hold from now on.
    pub fn revision(&self) -> &ConfigRevision {
        self.document.revision()
    }
}

/// The settings-file writer. One editor owns the per-file locks, the revision
/// cache and the last-written hashes; the app, the CLI and the imports share it.
pub struct ConfigEditor {
    root: PathBuf,
    /// Overrides for a caller that addresses a file by an explicit path (the
    /// CLI's `--config`, tests).
    paths: Mutex<HashMap<ConfigFile, PathBuf>>,
    /// Per-file in-process mutex, held across the whole edit protocol.
    locks: Mutex<HashMap<ConfigFile, Arc<Mutex<()>>>>,
    generations: Mutex<HashMap<ConfigFile, u64>>,
    revisions: Mutex<HashMap<ConfigFile, ConfigRevision>>,
    written: Mutex<HashMap<ConfigFile, ConfigRevision>>,
}

impl ConfigEditor {
    /// An editor for the three settings files under `root` (`~/.goble` in the
    /// running app).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            paths: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            generations: Mutex::new(HashMap::new()),
            revisions: Mutex::new(HashMap::new()),
            written: Mutex::new(HashMap::new()),
        }
    }

    /// An editor for a Goble home.
    pub fn for_home(home: &crate::app_home::GobleHome) -> Self {
        Self::new(home.root())
    }

    /// An editor whose `Config` file is at an explicit path (a CLI `--config`
    /// that is not `--home/config.toml`).
    pub fn with_config_path(root: impl Into<PathBuf>, path: impl Into<PathBuf>) -> Self {
        let editor = Self::new(root);
        editor.set_path(ConfigFile::Config, path);
        editor
    }

    /// Where the editor writes `file`.
    pub fn path(&self, file: ConfigFile) -> PathBuf {
        if let Some(path) = self.paths.lock().get(&file) {
            return path.clone();
        }
        file.path_in(&self.root)
    }

    /// Point one file at an explicit path.
    pub fn set_path(&self, file: ConfigFile, path: impl Into<PathBuf>) {
        self.paths.lock().insert(file, path.into());
    }

    /// The directory the three files live in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The revision this process last read or wrote for `file`.
    pub fn revision(&self, file: ConfigFile) -> Option<ConfigRevision> {
        self.revisions.lock().get(&file).cloned()
    }

    /// The revision of the bytes this process last *wrote* for `file`. The
    /// watcher uses it to ignore our own writes.
    pub fn last_written(&self, file: ConfigFile) -> Option<ConfigRevision> {
        self.written.lock().get(&file).cloned()
    }

    /// Whether `revision` is the file's content as this editor last wrote it
    /// (so a watcher event for it is our own write, not an external edit).
    pub fn is_own_revision(&self, file: ConfigFile, revision: &ConfigRevision) -> bool {
        self.last_written(file)
            .is_some_and(|written| written.hash == revision.hash)
    }

    /// The per-process generation counter for `file`.
    pub fn generation(&self, file: ConfigFile) -> u64 {
        *self.generations.lock().get(&file).unwrap_or(&0)
    }

    /// Read and parse one file. Missing is an empty document; not TOML is a
    /// refusal. The revision cache is updated to what was read.
    pub fn load(&self, file: ConfigFile) -> Result<ConfigDocument, ConfigError> {
        let path = self.path(file);
        let in_process = self.lock_handle(file);
        let _guard = in_process.lock();
        let raw = read_text(&path, file)?;
        let revision = self.remember(file, &raw);
        ConfigDocument::from_text(file, &path, raw, revision.generation)
    }

    /// Tighten an existing settings file to mode `0600` without reading or
    /// rewriting it. The first save does this anyway (`config.toml` shipped
    /// world-readable with an inline key); this is the startup migration the
    /// app calls once, and it reports whether the mode changed. A missing file
    /// is not created.
    #[cfg(unix)]
    pub fn tighten_permissions(&self, file: ConfigFile) -> std::io::Result<bool> {
        use std::os::unix::fs::PermissionsExt;
        let path = self.path(file);
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if metadata.permissions().mode() & 0o777 == 0o600 {
            return Ok(false);
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        log::info!(
            "{}: tightened permissions to 0600 (it holds credentials inline)",
            path.display()
        );
        Ok(true)
    }

    /// Read one file without disturbing the editor's caches (a read-only
    /// diagnostics path for the Advanced page).
    pub fn peek(&self, file: ConfigFile) -> Result<ConfigDocument, ConfigError> {
        let path = self.path(file);
        let raw = read_text(&path, file)?;
        ConfigDocument::from_text(file, &path, raw, self.generation(file))
    }

    /// Apply one edit to `file` and write it back atomically.
    ///
    /// `expected` is the revision the caller rendered from; `None` means "apply
    /// to whatever is on disk now" and is used by the imports and by a caller
    /// that just read the file. The closure receives the mutable document and
    /// returns whatever the caller wants back; returning an error aborts the
    /// edit and writes nothing.
    pub fn edit<T>(
        &self,
        file: ConfigFile,
        expected: Option<&ConfigRevision>,
        change: impl FnOnce(&mut DocumentMut) -> Result<T, ConfigError>,
    ) -> Result<EditOutcome<T>, ConfigError> {
        let path = self.path(file);
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .map_err(|error| ConfigError::Io(file, format!("create {}: {error}", parent.display())))?;
        }

        let in_process = self.lock_handle(file);
        let _guard = in_process.lock();
        let _file_lock = AdvisoryLock::acquire(&path, file)?;

        // 1. Re-read under the lock. The bytes on disk are the only base an edit
        //    may be applied to; a caller's stale document is never merged.
        let raw = read_text(&path, file)?;
        let actual = ConfigRevision::of(raw.as_bytes(), self.generation(file));
        if let Some(expected) = expected {
            if expected.hash != actual.hash {
                return Err(ConfigError::Conflict {
                    file,
                    expected: expected.clone(),
                    actual,
                });
            }
        }

        // 2. Parse. A file we cannot parse is never overwritten.
        let mut doc: DocumentMut = raw
            .parse::<DocumentMut>()
            .map_err(|error: toml_edit::TomlError| {
                ConfigError::RefusedUnparseable(error.to_string(), file)
            })?;
        let before_problems = problems_for(file, &raw);

        // 3. The caller's change.
        let value = change(&mut doc)?;
        let text = doc.to_string();

        // 4. A no-op edit is not a write: no mtime change, no watcher event.
        if text == raw {
            let revision = self.remember(file, &raw);
            let document = ConfigDocument::from_text(file, &path, raw, revision.generation)?;
            return Ok(EditOutcome {
                value,
                document,
                written: false,
            });
        }

        // 5. Validate before persisting.
        let errors = schema::validate(file, &doc);
        if !errors.is_empty() {
            return Err(ConfigError::Invalid(errors));
        }
        let after_problems = problems_for(file, &text);
        let new_problems: Vec<&ConfigProblem> = after_problems
            .iter()
            .filter(|problem| !before_problems.contains(*problem))
            .collect();
        if !new_problems.is_empty() {
            return Err(ConfigError::Invalid(
                new_problems
                    .into_iter()
                    .map(|problem| FieldError {
                        path: problem
                            .section
                            .split('.')
                            .filter(|part| !part.is_empty())
                            .map(str::to_string)
                            .collect(),
                        message: problem.line(),
                    })
                    .collect(),
            ));
        }

        // 6. Atomic private write, then remember the new revision.
        write_atomic_private(&path, text.as_bytes())
            .map_err(|error| ConfigError::Io(file, format!("write {}: {error}", path.display())))?;
        let revision = self.remember_written(file, &text);
        let document = ConfigDocument::from_text(file, &path, text, revision.generation)?;
        Ok(EditOutcome {
            value,
            document,
            written: true,
        })
    }

    /// Apply a batch of [`ConfigOp`](super::ConfigOp)s in one atomic write, with
    /// the same conflict/refusal/validation semantics as [`Self::edit`].
    pub fn apply_ops(
        &self,
        file: ConfigFile,
        ops: &[super::ConfigOp],
        expected: Option<&ConfigRevision>,
    ) -> Result<EditOutcome<()>, ConfigError> {
        self.edit(file, expected, |doc| schema::apply_ops(doc, ops))
    }

    /// Reserialize a typed [`GobleConfig`](super::GobleConfig) into
    /// `config.toml` through the full edit protocol (lock, revision fence,
    /// refusal of an unparseable file, validation, atomic `0600` write).
    ///
    /// This is the compatibility shim for the legacy whole-config writers
    /// (the service's old `save_config`); new settings writes use path ops so
    /// comments and unknown keys survive. A legacy `[llm]` section is folded
    /// before the write, so the shim writes the adopted schema.
    pub fn save_typed_config(
        &self,
        config: &super::GobleConfig,
        expected: Option<&ConfigRevision>,
    ) -> Result<EditOutcome<()>, ConfigError> {
        let mut config = config.clone();
        config.absorb_legacy();
        let text = config
            .to_toml()
            .map_err(|error| ConfigError::Io(ConfigFile::Config, error.to_string()))?;
        let replacement: DocumentMut = text
            .parse::<DocumentMut>()
            .map_err(|error| ConfigError::RefusedUnparseable(error.to_string(), ConfigFile::Config))?;
        self.edit(ConfigFile::Config, expected, |doc| {
            *doc = replacement;
            Ok(())
        })
    }

    /// Replace `environment.toml` with `groups` through the same protocol. The
    /// file's writer is typed (a group is written as a unit), but the lock,
    /// revision fence, refusal and atomic private write are shared.
    pub fn save_environment(
        &self,
        groups: &EnvironmentFile,
        expected: Option<&ConfigRevision>,
    ) -> Result<EditOutcome<()>, ConfigError> {
        self.write_typed(
            ConfigFile::Environment,
            expected,
            environment::to_toml_with_header(groups),
        )
    }

    /// Replace `hosts.toml` with `hosts` through the same protocol. The file's
    /// header is preserved.
    pub fn save_hosts(
        &self,
        hosts: &HostsFile,
        expected: Option<&ConfigRevision>,
    ) -> Result<EditOutcome<()>, ConfigError> {
        self.write_typed(ConfigFile::Hosts, expected, hosts::to_toml_with_header(hosts))
    }

    /// The shared tail of a typed whole-file save: lock, re-read, conflict, parse
    /// the file we are about to replace, validate the replacement, write.
    fn write_typed(
        &self,
        file: ConfigFile,
        expected: Option<&ConfigRevision>,
        text: String,
    ) -> Result<EditOutcome<()>, ConfigError> {
        let path = self.path(file);
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .map_err(|error| ConfigError::Io(file, format!("create {}: {error}", parent.display())))?;
        }
        let in_process = self.lock_handle(file);
        let _guard = in_process.lock();
        let _file_lock = AdvisoryLock::acquire(&path, file)?;

        let raw = read_text(&path, file)?;
        let actual = ConfigRevision::of(raw.as_bytes(), self.generation(file));
        if let Some(expected) = expected {
            if expected.hash != actual.hash {
                return Err(ConfigError::Conflict {
                    file,
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        // A file we cannot parse is a file we do not understand: refuse rather
        // than replace the user's edits with our typed copy.
        raw.parse::<DocumentMut>()
            .map_err(|error: toml_edit::TomlError| {
                ConfigError::RefusedUnparseable(error.to_string(), file)
            })?;

        let replacement: DocumentMut = text
            .parse::<DocumentMut>()
            .map_err(|error: toml_edit::TomlError| {
                ConfigError::RefusedUnparseable(error.to_string(), file)
            })?;
        let errors = schema::validate(file, &replacement);
        if !errors.is_empty() {
            return Err(ConfigError::Invalid(errors));
        }

        write_atomic_private(&path, text.as_bytes())
            .map_err(|error| ConfigError::Io(file, format!("write {}: {error}", path.display())))?;
        let revision = self.remember_written(file, &text);
        let document = ConfigDocument::from_text(file, &path, text, revision.generation)?;
        Ok(EditOutcome {
            value: (),
            document,
            written: true,
        })
    }

    fn lock_handle(&self, file: ConfigFile) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock();
        Arc::clone(locks.entry(file).or_default())
    }

    fn remember(&self, file: ConfigFile, raw: &str) -> ConfigRevision {
        let generation = {
            let mut generations = self.generations.lock();
            let generation = generations.entry(file).or_insert(0);
            *generation += 1;
            *generation
        };
        let revision = ConfigRevision::of(raw.as_bytes(), generation);
        self.revisions.lock().insert(file, revision.clone());
        revision
    }

    fn remember_written(&self, file: ConfigFile, raw: &str) -> ConfigRevision {
        let revision = self.remember(file, raw);
        self.written.lock().insert(file, revision.clone());
        revision
    }
}

impl std::fmt::Debug for ConfigEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigEditor")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

/// Read a settings file as text. A missing file is the empty document, so a
/// first write creates it.
fn read_text(path: &Path, file: ConfigFile) -> Result<String, ConfigError> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|error| ConfigError::RefusedUnparseable(error.to_string(), file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(ConfigError::Io(
            file,
            format!("read {}: {error}", path.display()),
        )),
    }
}

/// The advisory lock beside a settings file (`.lock` suffix, mode 0600). Held
/// for the whole edit protocol, so the app and `goble-cli setup` serialize even
/// though they are separate processes.
struct AdvisoryLock {
    file: File,
}

impl AdvisoryLock {
    fn acquire(target: &Path, file: ConfigFile) -> Result<Self, ConfigError> {
        let lock_path = target.with_extension(format!("{}.lock", extension_of(target)));
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                ConfigError::Io(
                    file,
                    format!("open lock {}: {error}", lock_path.display()),
                )
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = lock.set_permissions(fs::Permissions::from_mode(0o600));
        }
        FileExt::lock_exclusive(&lock).map_err(|error| {
            ConfigError::Io(
                file,
                format!("lock {}: {error}", lock_path.display()),
            )
        })?;
        Ok(Self { file: lock })
    }
}

impl Drop for AdvisoryLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn extension_of(path: &Path) -> &str {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("lock")
}

/// Fill an empty `config.toml` document with the documented seed: the
/// [`CONFIG_HEADER`](super::CONFIG_HEADER) block and the default tables, exactly
/// the bytes [`GobleConfig::to_toml`](super::GobleConfig::to_toml) writes. A
/// document that already holds a key is left alone, so a first write on a fresh
/// home creates the same file the app seeds on first launch.
pub fn seed_default_document(doc: &mut DocumentMut) -> Result<(), ConfigError> {
    if !doc.as_table().is_empty() {
        return Ok(());
    }
    let seeded = super::GobleConfig::default()
        .to_toml()
        .map_err(|error| ConfigError::Io(ConfigFile::Config, error.to_string()))?;
    if let Ok(parsed) = seeded.parse::<DocumentMut>() {
        *doc = parsed;
    }
    Ok(())
}

/// Write `bytes` to `path` atomically and privately.
///
/// The temp file is created in the document's own directory (so the rename is
/// same-filesystem and atomic), written, flushed and `fsync`ed, then renamed
/// over the target. On Unix the temp file is mode 0600 before the rename, so the
/// target is 0600 as well — including a target that existed as 0644, which is
/// how the inline-key `config.toml` gets tightened. The directory is `fsync`ed
/// after the rename so the rename itself survives a crash.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    temp.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    {
        // Best-effort: a directory that refuses an fsync still took the rename.
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigOp;
    use crate::config::ConfigValue;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "goble-config-editor-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const FIXTURE: &str = "\
# goble configuration, hand written
# a comment that must survive every edit

[app]
friendly_name = \"Ada\"

[model.deepseek]
model = \"deepseek-flash\"
# the key below is mine
api_key = \"sk-live\"
unknown_key = \"kept\"

[cli]
installer = \"internal\"
";

    #[test]
    fn editing_a_value_keeps_comments_and_unknown_sections() {
        let dir = scratch("preserve");
        let path = dir.join("config.toml");
        fs::write(&path, FIXTURE).unwrap();
        let editor = ConfigEditor::new(dir.clone());

        editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(&["app", "vim_mode"], ConfigValue::Bool(true))],
                None,
            )
            .expect("edit");

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.starts_with("# goble configuration, hand written\n"), "{after}");
        assert!(
            after.contains("# a comment that must survive every edit"),
            "{after}"
        );
        assert!(after.contains("# the key below is mine"), "{after}");
        assert!(after.contains("unknown_key = \"kept\""), "{after}");
        assert!(after.contains("[cli]\ninstaller = \"internal\""), "{after}");
        assert!(after.contains("vim_mode = true"), "{after}");
        assert!(after.contains("friendly_name = \"Ada\""), "{after}");
        // The targeted edit changed exactly the targeted lines.
        let before = FIXTURE
            .lines()
            .filter(|line| !line.starts_with("vim_mode"))
            .collect::<Vec<_>>();
        let edited = after
            .lines()
            .filter(|line| !line.starts_with("vim_mode"))
            .collect::<Vec<_>>();
        assert_eq!(before, edited);
    }

    #[test]
    fn unsetting_returns_to_the_default_and_removes_the_key() {
        let dir = scratch("unset");
        let path = dir.join("config.toml");
        fs::write(&path, "[app]\nvim_mode = true\nscroll_speed = 80\n").unwrap();
        let editor = ConfigEditor::new(dir.clone());

        editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::unset(&["app", "vim_mode"])],
                None,
            )
            .expect("unset");

        let after = fs::read_to_string(&path).unwrap();
        assert!(!after.contains("vim_mode"), "{after}");
        assert!(after.contains("scroll_speed = 80"), "{after}");
        // Unset is idempotent.
        editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::unset(&["app", "vim_mode"])],
                None,
            )
            .expect("second unset");
        assert_eq!(fs::read_to_string(&path).unwrap(), after);
    }

    #[test]
    fn a_conflict_is_refused_when_the_file_changed() {
        let dir = scratch("conflict");
        let path = dir.join("config.toml");
        fs::write(&path, FIXTURE).unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let loaded = editor.load(ConfigFile::Config).unwrap();
        let revision = loaded.revision().clone();

        // Someone else appends a comment after the UI read the file.
        let mut external = fs::read_to_string(&path).unwrap();
        external.push_str("\n# edited while the UI was open\n");
        fs::write(&path, &external).unwrap();

        let error = editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(&["app", "vim_mode"], ConfigValue::Bool(true))],
                Some(&revision),
            )
            .expect_err("a stale revision must conflict");
        assert!(error.is_conflict(), "{error:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), external);

        // The fresh revision writes.
        let fresh = editor.load(ConfigFile::Config).unwrap();
        editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(&["app", "vim_mode"], ConfigValue::Bool(true))],
                Some(fresh.revision()),
            )
            .expect("fresh revision writes");
    }

    #[test]
    fn an_unparseable_file_is_never_overwritten() {
        let dir = scratch("unparseable");
        let path = dir.join("config.toml");
        let broken = "[app\nvim_mode = true\n";
        fs::write(&path, broken).unwrap();
        let editor = ConfigEditor::new(dir.clone());

        let error = editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(&["app", "vim_mode"], ConfigValue::Bool(false))],
                None,
            )
            .expect_err("refused");
        assert!(
            matches!(error, ConfigError::RefusedUnparseable(_, ConfigFile::Config)),
            "{error:?}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    }

    #[test]
    fn a_failed_validation_writes_nothing() {
        let dir = scratch("invalid");
        let path = dir.join("config.toml");
        fs::write(&path, FIXTURE).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        let editor = ConfigEditor::new(dir.clone());

        let error = editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(&["app", "scroll_speed"], ConfigValue::Int(500))],
                None,
            )
            .expect_err("out of range");
        match &error {
            ConfigError::Invalid(errors) => {
                assert_eq!(errors.len(), 1, "{errors:?}");
                assert_eq!(errors[0].dotted(), "app.scroll_speed", "{errors:?}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        // An enum outside the known set is refused too.
        let error = editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(
                    &["app", "approval_mode"],
                    ConfigValue::Str("yolo".to_string()),
                )],
                None,
            )
            .expect_err("unknown mode");
        assert!(matches!(error, ConfigError::Invalid(_)), "{error:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn the_write_is_atomic_and_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch("atomic");
        let path = dir.join("config.toml");
        // A pre-existing 0644 file (today's config.toml with an inline key).
        fs::write(&path, FIXTURE).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let editor = ConfigEditor::new(dir.clone());

        // A reader thread must never observe a partial file.
        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            while !reader_stop.load(Ordering::SeqCst) {
                if let Ok(bytes) = fs::read(&reader_path) {
                    let text = String::from_utf8(bytes).expect("always utf-8");
                    if !text.is_empty() {
                        toml::from_str::<toml::Table>(&text)
                            .expect("a reader never sees a partial document");
                    }
                }
            }
        });

        for index in 0..25 {
            editor
                .apply_ops(
                    ConfigFile::Config,
                    &[ConfigOp::set(
                        &["app", "friendly_name"],
                        ConfigValue::Str(format!("name-{index}")),
                    )],
                    None,
                )
                .expect("write");
        }
        stop.store(true, Ordering::SeqCst);
        reader.join().unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the file is private after a write");
        // No temp files survive.
        let leftovers: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .filter(|name| name != "config.toml" && name != "config.toml.lock")
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
    }

    #[test]
    fn concurrent_editors_serialize_and_both_writes_land() {
        let dir = scratch("concurrent");
        let path = dir.join("config.toml");
        fs::write(&path, "[app]\n").unwrap();

        let inside = Arc::new(AtomicBool::new(false));
        let overlapped = Arc::new(AtomicBool::new(false));

        let first_dir = dir.clone();
        let first_inside = Arc::clone(&inside);
        let first_overlapped = Arc::clone(&overlapped);
        let first = std::thread::spawn(move || {
            let editor = ConfigEditor::new(first_dir);
            editor
                .edit(ConfigFile::Config, None, |doc| {
                    first_inside.store(true, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    schema::apply_ops(
                        doc,
                        &[ConfigOp::set(
                            &["app", "friendly_name"],
                            ConfigValue::Str("first".to_string()),
                        )],
                    )?;
                    first_inside.store(false, Ordering::SeqCst);
                    Ok(())
                })
                .expect("first write");
        });

        let second_dir = dir.clone();
        let second_inside = Arc::clone(&inside);
        let second_overlapped = Arc::clone(&overlapped);
        let second = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let editor = ConfigEditor::new(second_dir);
            editor
                .edit(ConfigFile::Config, None, |doc| {
                    if second_inside.load(Ordering::SeqCst) {
                        second_overlapped.store(true, Ordering::SeqCst);
                    }
                    schema::apply_ops(
                        doc,
                        &[ConfigOp::set(
                            &["app", "usage"],
                            ConfigValue::Str("second".to_string()),
                        )],
                    )
                })
                .expect("second write");
        });

        first.join().unwrap();
        second.join().unwrap();
        assert!(
            !overlapped.load(Ordering::SeqCst),
            "the second edit started while the first held the lock"
        );

        // Both edits were applied to the latest document (no lost update).
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("friendly_name = \"first\""), "{text}");
        assert!(text.contains("usage = \"second\""), "{text}");
    }

    #[cfg(unix)]
    #[test]
    fn hosts_round_trip_through_the_editor() {
        use crate::hosts::HostEntry;
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch("hosts");
        let editor = ConfigEditor::new(dir.clone());
        let mut hosts = HostsFile::default();
        hosts.hosts.push(HostEntry {
            name: "prod".to_string(),
            host: "prod.example.com".to_string(),
            user: Some("deploy".to_string()),
            port: Some(2222),
            identity: Some("~/.ssh/id_prod".to_string()),
        });
        editor.save_hosts(&hosts, None).expect("write hosts");

        let text = fs::read_to_string(dir.join("hosts.toml")).unwrap();
        assert!(text.starts_with("# Saved SSH hosts"), "{text}");
        let mode = fs::metadata(dir.join("hosts.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        let loaded = editor.load(ConfigFile::Hosts).unwrap();
        let typed = loaded.typed_hosts().unwrap();
        assert_eq!(typed.hosts.len(), 1);
        assert_eq!(typed.hosts[0].name, "prod");
        assert_eq!(typed.hosts[0].port, Some(2222));
    }

    #[test]
    fn a_noop_edit_does_not_rewrite_the_file() {
        let dir = scratch("noop");
        let path = dir.join("config.toml");
        fs::write(&path, FIXTURE).unwrap();
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let outcome = editor
            .edit(ConfigFile::Config, None, |_doc| Ok(()))
            .expect("no-op");
        assert!(!outcome.written);
        assert_eq!(fs::read_to_string(&path).unwrap(), FIXTURE);
        let after = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(before, after, "a no-op edit leaves the mtime alone");
    }

    #[test]
    fn a_model_table_is_written_and_deleted_as_one_path() {
        let dir = scratch("models");
        let editor = ConfigEditor::new(dir.clone());
        editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(
                    &["model", "kimi"],
                    ConfigValue::Table(vec![
                        (
                            "model".to_string(),
                            ConfigValue::Str("kimi-k2".to_string()),
                        ),
                        (
                            "base_url".to_string(),
                            ConfigValue::Str("https://api.kimi.com/v1".to_string()),
                        ),
                        (
                            "max_completion_tokens".to_string(),
                            ConfigValue::Int(8192),
                        ),
                    ]),
                )],
                None,
            )
            .expect("write model");
        let text = fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(text.contains("[model.kimi]"), "{text}");
        assert!(text.contains("max_completion_tokens = 8192"), "{text}");

        editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::unset(&["model", "kimi"])],
                None,
            )
            .expect("delete model");
        let text = fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(!text.contains("[model.kimi]"), "{text}");
    }
}
