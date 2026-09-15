//! Environment secret groups: the named groups of secrets the Settings →
//! Environment pane builds, for a remote session to start with.
//!
//! The groups live in `~/.goble/environment.toml`, a file meant to be
//! hand-edited, so the file is the identity: a group *is* its `[groups.<name>]`
//! table and its name is its id. The store's `secret_groups` tables are read
//! once, to migrate an install that predates the file, and never again.
//!
//! **No server side exists yet:** nothing in this tree runs a remote session —
//! [`DesktopState::run_chat_turn`] runs the embedded daemon, and the app's
//! remote route (`DaemonModel::run_turn` with `WorkspaceRouting::Remote`) needs
//! a `DaemonClient` that nothing ever attaches — so these groups are read by
//! the settings pane and stop there. The accessor below is where a remote
//! session would read them.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use goble_core::environment::{self, EnvValue, EnvironmentFile};
use goble_core::store::{SecretEntry, SecretGroup};

use super::DesktopState;

/// What the last read of the file saw. The file is the identity of a group;
/// this is only the page's last view of it, kept so an unparsable edit does not
/// blank the page.
#[derive(Default)]
pub(crate) struct EnvironmentCache {
    /// The file, once resolved. `None` means `~/.goble/environment.toml`.
    path: Option<PathBuf>,
    /// The groups the file held the last time it parsed.
    groups: EnvironmentFile,
    /// Why the last read failed; `None` when the file parsed.
    problem: Option<String>,
    /// Whether the store has had its one chance to seed the file.
    migrated: bool,
}

/// A group as the page holds it: the file's table key is the id, and the file
/// records no timestamps.
fn group_of(name: &str, variables: &BTreeMap<String, EnvValue>) -> SecretGroup {
    SecretGroup {
        id: name.to_string(),
        name: name.to_string(),
        created_at: String::new(),
        updated_at: String::new(),
        entries: variables
            .iter()
            .map(|(variable, value)| SecretEntry {
                id: entry_id(name, variable),
                group_id: name.to_string(),
                name: variable.to_string(),
                value: value_text(value),
                updated_at: String::new(),
            })
            .collect(),
    }
}

/// The groups as the page holds them, ordered by group name like the file is.
fn groups_of(file: &EnvironmentFile) -> Vec<SecretGroup> {
    file.iter()
        .map(|(name, variables)| group_of(name, variables))
        .collect()
}

/// What the page shows for a value. A vault reference has no value to show, so
/// the reference itself comes back: the pane is a window onto the file, and an
/// edit that writes a literal replaces the reference visibly.
fn value_text(value: &EnvValue) -> String {
    match value {
        EnvValue::Literal(text) => text.clone(),
        EnvValue::Vault { vault } => format!("{{ vault = \"{vault}\" }}"),
    }
}

/// An entry's id, the group it is in and the variable's name joined by a unit
/// separator. The file has no ids of its own, and the page's delete control
/// carries only this string, so the group has to travel inside it.
fn entry_id(group: &str, name: &str) -> String {
    format!("{group}\u{1f}{name}")
}

fn split_entry_id(id: &str) -> Option<(&str, &str)> {
    id.split_once('\u{1f}')
}

impl DesktopState {
    /// Point the environment accessors at `path` instead of
    /// `~/.goble/environment.toml`. `open_default` pins the located home; a test
    /// pins a temp file, so the real home is never read or written.
    pub fn set_environment_path(&self, path: impl Into<PathBuf>) {
        *self.environment.lock() = EnvironmentCache {
            path: Some(path.into()),
            ..EnvironmentCache::default()
        };
    }

    /// The file Settings → Environment shows and writes.
    ///
    /// A state built with `DesktopState::new` has no home, so this refuses
    /// rather than guessing one: the page then draws its error line, and no test
    /// can write into the user's real `~/.goble` by accident.
    pub fn environment_path(&self) -> anyhow::Result<PathBuf> {
        if let Some(path) = self.environment.lock().path.clone() {
            return Ok(path);
        }
        anyhow::bail!(
            "this state has no environment path: pin one with `set_environment_path` (the running app gets it from `open_default`)"
        )
    }

    /// Why the last read failed, or `None` when the file parsed. A file that is
    /// not TOML at all is reported here, and every write is refused until it
    /// parses: what cannot be parsed is never written over.
    pub fn environment_problem(&self) -> Option<String> {
        self.environment.lock().problem.clone()
    }

    /// Read the file and parse it. `Err` carries the reason the file could not
    /// be read as a whole — it is not TOML at all, or it cannot be read. A file
    /// that is not there is `Ok` and empty: there are no groups yet. One group
    /// that does not parse only costs itself.
    fn load_environment_file(&self, path: &Path) -> std::result::Result<EnvironmentFile, String> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        match environment::load_toml_with_problems(&text) {
            Ok((groups, problems)) => {
                for problem in &problems {
                    log::error!("{}: {problem}", path.display());
                }
                Ok(groups)
            }
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The groups the file holds, as the cache last saw them.
    fn remember(&self, path: &Path, groups: EnvironmentFile) {
        let mut cache = self.environment.lock();
        cache.path.get_or_insert_with(|| path.to_path_buf());
        cache.groups = groups;
        cache.problem = None;
    }

    /// Keep the groups last read and record why this read failed.
    fn remember_failure(&self, path: &Path, problem: String) {
        log::error!("{problem}");
        let mut cache = self.environment.lock();
        cache.path.get_or_insert_with(|| path.to_path_buf());
        cache.problem = Some(problem);
    }

    /// Whether this read is the one-shot migration's: the first read of a file
    /// that holds no groups.
    fn take_migration_slot(&self, file_is_empty: bool) -> bool {
        let mut cache = self.environment.lock();
        let migrate = !cache.migrated && file_is_empty;
        cache.migrated = true;
        migrate
    }

    /// Read the file, apply `edit` to it, and write it back through
    /// [`environment::save`] — the one writer, so the mode and the header come
    /// from one place. A file that does not parse is refused, never overwritten.
    fn edit_environment<T>(
        &self,
        edit: impl FnOnce(&mut EnvironmentFile) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let path = self.environment_path()?;
        let mut groups = match self.load_environment_file(&path) {
            Ok(groups) => groups,
            Err(problem) => {
                self.remember_failure(&path, problem.clone());
                anyhow::bail!("refusing to write {}: {problem}", path.display());
            }
        };
        let outcome = edit(&mut groups)?;
        environment::save(&path, &groups)?;
        self.remember(&path, groups);
        Ok(outcome)
    }

    /// The one-shot migration: a file with no groups beside a store that has
    /// some is an install that predates the file. The store's groups are written
    /// into the file and it is never read for this purpose again — its tables
    /// stay, so a rollback to a build that reads them finds them.
    fn migrate_from_store(&self, path: &Path) -> anyhow::Result<()> {
        let groups = {
            let store = self.store.lock();
            let mut groups = EnvironmentFile::new();
            for group in store.list_secret_groups()? {
                let variables = group
                    .entries
                    .into_iter()
                    .map(|entry| (entry.name, EnvValue::Literal(entry.value)))
                    .collect();
                groups.insert(group.name, variables);
            }
            groups
        };
        if groups.is_empty() {
            self.remember(path, groups);
            return Ok(());
        }
        environment::save(path, &groups)?;
        log::info!(
            "{}: migrated {} group(s) out of the store",
            path.display(),
            groups.len()
        );
        self.remember(path, groups);
        Ok(())
    }

    /// Every environment group with its secrets, ordered by group name, read
    /// from the file. A file that cannot be parsed keeps the groups last read
    /// and reports itself through [`DesktopState::environment_problem`].
    ///
    /// This is the read path a remote session would use. Nothing transmits the
    /// groups today: there is no remote transport in the tree, so a value read
    /// here has not left the machine.
    pub fn environment_groups(&self) -> anyhow::Result<Vec<SecretGroup>> {
        let path = self.environment_path()?;
        match self.load_environment_file(&path) {
            Ok(groups) => match self.take_migration_slot(groups.is_empty()) {
                true => self.migrate_from_store(&path)?,
                false => self.remember(&path, groups),
            },
            Err(problem) => self.remember_failure(&path, problem),
        }
        Ok(groups_of(&self.environment.lock().groups))
    }

    /// One group with its secrets, or `None` when no such group exists. A group
    /// is its name, so `id` is the name the page holds.
    pub fn environment_group(&self, id: &str) -> anyhow::Result<Option<SecretGroup>> {
        Ok(self.environment_groups()?.into_iter().find(|g| g.id == id))
    }

    /// Create an empty group under `name` and return it, named by the table it
    /// makes. The file records no timestamps and the store is not written.
    pub fn create_environment_group(&self, name: &str) -> anyhow::Result<SecretGroup> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "a group needs a name");
        let now = Utc::now().to_rfc3339();
        self.edit_environment(|groups| {
            anyhow::ensure!(!groups.contains_key(name), "group {name} is already there");
            groups.insert(name.to_string(), BTreeMap::new());
            Ok(SecretGroup {
                id: name.to_string(),
                name: name.to_string(),
                created_at: now.clone(),
                updated_at: now,
                entries: Vec::new(),
            })
        })
    }

    /// Delete a group and every secret in it; returns whether a group was there.
    /// The name is the id, so this drops that one table from the file.
    pub fn delete_environment_group(&self, id: &str) -> anyhow::Result<bool> {
        self.edit_environment(|groups| Ok(groups.remove(id).is_some()))
    }

    /// Add a secret to a group, or edit the one already carrying that name.
    /// When `renamed_from` names the entry's previous name it is removed in the
    /// same write, so a rename leaves one key behind and not two.
    pub fn save_environment_secret(
        &self,
        group_id: &str,
        renamed_from: Option<&str>,
        name: &str,
        value: &str,
    ) -> anyhow::Result<SecretEntry> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "a secret needs a name");
        let now = Utc::now().to_rfc3339();
        self.edit_environment(|groups| {
            let variables = groups
                .get_mut(group_id)
                .ok_or_else(|| anyhow::anyhow!("group {group_id} does not exist"))?;
            if let Some(previous) = renamed_from {
                if previous != name {
                    variables.remove(previous);
                }
            }
            variables.insert(name.to_string(), EnvValue::Literal(value.to_string()));
            Ok(SecretEntry {
                id: entry_id(group_id, name),
                group_id: group_id.to_string(),
                name: name.to_string(),
                value: value.to_string(),
                updated_at: now.clone(),
            })
        })
    }

    /// Remove one secret by the id the page holds for it; returns whether a key
    /// was removed.
    pub fn delete_environment_secret(&self, id: &str) -> anyhow::Result<bool> {
        let Some((group, name)) = split_entry_id(id) else {
            return Ok(false);
        };
        self.edit_environment(|groups| {
            let Some(variables) = groups.get_mut(group) else {
                return Ok(false);
            };
            Ok(variables.remove(name).is_some())
        })
    }
}
