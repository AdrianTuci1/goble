//! One-time import of the old store-backed app preferences into `[app]`.
//!
//! Before settings lived in `config.toml`, the desktop app kept a handful of
//! preferences in the SQLite settings table. This build reads them from the file
//! and writes them there; the import runs exactly once, following the shipped
//! `environment.toml` precedent: the store keys are **not** deleted (a rollback
//! to a build that reads them still finds them), and a store marker
//! ([`APP_IMPORT_MARKER`]) records that the import ran, so a second run — or a
//! run whose first attempt failed after a partial write — never repeats it.
//!
//! The import is a single path-op batch through the editor, so it gets the same
//! lock, revision fence, atomic write and comment preservation as any other
//! edit. Values the file already has are left alone: an `[app]` section at all
//! means the user has moved on, and the store copy is not authoritative.

use crate::store::Store;

use super::document::{ConfigError, ConfigFile, ConfigOp, ConfigValue};
use super::editor::ConfigEditor;
use super::schema;

/// The store key that records the one-time import ran.
pub const APP_IMPORT_MARKER: &str = "config.app_import_done";

/// The store keys the import reads, in the order it reports them. They are the
/// six keys the pre-file build wrote (`state/settings.rs` in the service).
const LEGACY_KEYS: [&str; 6] = [
    "friendly_name",
    "usage",
    "vim_mode",
    "auto_save_on_close",
    "auto_approve",
    "approval_mode",
];

/// What a run of the one-time import did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportOutcome {
    /// The values were written into `[app]` and the marker was set.
    Imported { keys: Vec<String> },
    /// A previous run imported already.
    AlreadyImported,
    /// `config.toml` already carries an `[app]` section; nothing was imported
    /// (and the marker was set so a later load does not try again).
    FileHasApp,
    /// The store holds none of the legacy keys; nothing was written and the
    /// marker was left unset.
    NothingToImport,
}

impl ImportOutcome {
    /// Whether this run wrote the file.
    pub fn imported(&self) -> bool {
        matches!(self, ImportOutcome::Imported { .. })
    }

    /// The settings keys imported, for the log line.
    pub fn keys(&self) -> &[String] {
        match self {
            ImportOutcome::Imported { keys } => keys,
            _ => &[],
        }
    }

    /// The one-line report for the log and the Advanced page.
    pub fn line(&self) -> String {
        match self {
            ImportOutcome::Imported { keys } => {
                format!("imported {} setting(s) into config.toml [app]: {}", keys.len(), keys.join(", "))
            }
            ImportOutcome::AlreadyImported => "app settings already imported".to_string(),
            ImportOutcome::FileHasApp => {
                "config.toml already has [app]; the store values were left alone".to_string()
            }
            ImportOutcome::NothingToImport => "no legacy app settings to import".to_string(),
        }
    }
}

/// Whether the one-time `[app]` import has run.
pub fn app_import_done(store: &Store) -> bool {
    store
        .get_setting(APP_IMPORT_MARKER)
        .ok()
        .flatten()
        .as_deref()
        == Some("1")
}

/// Import the store-backed app preferences into `[app]` once.
///
/// Returns [`ConfigError::RefusedUnparseable`] when `config.toml` is not TOML:
/// the caller keeps reading the store as a fallback and surfaces the failure; the
/// marker is not set, so a later run can retry.
pub fn import_store_prefs_once(
    editor: &ConfigEditor,
    store: &Store,
) -> Result<ImportOutcome, ConfigError> {
    if app_import_done(store) {
        return Ok(ImportOutcome::AlreadyImported);
    }

    let document = editor.load(ConfigFile::Config)?;
    if document.contains(&["app".to_string()]) {
        // The file owns the settings now. Mark the import done so a store value
        // written by an old build never resurrects over the file.
        mark_done(store)?;
        return Ok(ImportOutcome::FileHasApp);
    }

    let mut ops = Vec::new();
    let mut keys = Vec::new();
    for key in LEGACY_KEYS {
        let Some(value) = store.get_setting(key).map_err(store_error)?.filter(|value| !value.trim().is_empty()) else {
            continue;
        };
        let value = match key {
            "vim_mode" | "auto_save_on_close" | "auto_approve" => {
                ConfigValue::Bool(value == "1" || value.eq_ignore_ascii_case("true"))
            }
            "approval_mode" => {
                // A label this build does not know is not importable; leave the
                // store copy to a build that does and import the rest.
                if !schema::APPROVAL_MODES.contains(&value.as_str()) {
                    continue;
                }
                ConfigValue::Str(value)
            }
            _ => ConfigValue::Str(value),
        };
        keys.push(key.to_string());
        ops.push(ConfigOp::set(&["app", key], value));
    }

    if ops.is_empty() {
        return Ok(ImportOutcome::NothingToImport);
    }

    let expected = document.revision().clone();
    editor.edit(ConfigFile::Config, Some(&expected), |doc| {
        // A missing (or empty) config file is created with the documented
        // header, exactly as the app seeds it.
        super::editor::seed_default_document(doc)?;
        schema::apply_ops(doc, &ops)
    })?;

    mark_done(store)?;
    log::info!("{}", ImportOutcome::Imported { keys: keys.clone() }.line());
    Ok(ImportOutcome::Imported { keys })
}

fn mark_done(store: &Store) -> Result<(), ConfigError> {
    store
        .set_setting(APP_IMPORT_MARKER, "1")
        .map_err(store_error)
}

fn store_error(error: anyhow::Error) -> ConfigError {
    ConfigError::Io(ConfigFile::Config, format!("store: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "goble-config-import-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn store_with(values: &[(&str, &str)]) -> Store {
        let store = Store::open_in_memory().unwrap();
        for (key, value) in values {
            store.set_setting(key, value).unwrap();
        }
        store
    }

    const SEEDED: &str = "[app]\nfriendly_name = \"Ada\"\n";

    #[test]
    fn the_import_writes_the_legacy_prefs_once_and_keeps_the_store_copies() {
        let dir = scratch("once");
        let path = dir.join("config.toml");
        fs::write(&path, "[theme]\ndark = true\n").unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let store = store_with(&[
            ("friendly_name", "Ada"),
            ("usage", "coding"),
            ("vim_mode", "1"),
            ("auto_save_on_close", "0"),
            ("auto_approve", "1"),
            ("approval_mode", "plan"),
        ]);

        let outcome = import_store_prefs_once(&editor, &store).expect("import");
        match &outcome {
            ImportOutcome::Imported { keys } => {
                assert_eq!(keys.len(), 6, "{keys:?}");
            }
            other => panic!("expected an import, got {other:?}"),
        }
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[app]"), "{text}");
        assert!(text.contains("friendly_name = \"Ada\""), "{text}");
        assert!(text.contains("vim_mode = true"), "{text}");
        assert!(text.contains("auto_save_on_close = false"), "{text}");
        assert!(text.contains("approval_mode = \"plan\""), "{text}");
        // The old store copies are rollback backups: they stay.
        assert_eq!(store.get_setting("friendly_name").unwrap().as_deref(), Some("Ada"));
        assert!(app_import_done(&store));

        // A second run writes nothing.
        let before = fs::read_to_string(&path).unwrap();
        let second = import_store_prefs_once(&editor, &store).expect("second run");
        assert_eq!(second, ImportOutcome::AlreadyImported);
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn an_existing_app_section_is_left_alone() {
        let dir = scratch("existing");
        let path = dir.join("config.toml");
        fs::write(&path, SEEDED).unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let store = store_with(&[("friendly_name", "Grace")]);

        let outcome = import_store_prefs_once(&editor, &store).expect("no import");
        assert_eq!(outcome, ImportOutcome::FileHasApp);
        assert_eq!(fs::read_to_string(&path).unwrap(), SEEDED);
        assert!(
            app_import_done(&store),
            "the marker is set so a later load never imports over the file"
        );
    }

    #[test]
    fn an_empty_store_writes_nothing_and_leaves_the_marker_unset() {
        let dir = scratch("empty");
        let path = dir.join("config.toml");
        fs::write(&path, "[theme]\ndark = true\n").unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let store = store_with(&[]);

        let outcome = import_store_prefs_once(&editor, &store).expect("no import");
        assert_eq!(outcome, ImportOutcome::NothingToImport);
        assert!(!app_import_done(&store));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[theme]\ndark = true\n"
        );
    }

    #[test]
    fn an_unparseable_file_is_refused_and_the_marker_stays_unset() {
        let dir = scratch("broken");
        let path = dir.join("config.toml");
        let broken = "[app\n= oops\n";
        fs::write(&path, broken).unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let store = store_with(&[("friendly_name", "Ada")]);

        let error = import_store_prefs_once(&editor, &store).expect_err("refused");
        assert!(
            matches!(error, ConfigError::RefusedUnparseable(_, ConfigFile::Config)),
            "{error:?}"
        );
        assert!(!app_import_done(&store), "the import can retry later");
        assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    }

    #[test]
    fn a_missing_file_is_created_with_the_documented_header() {
        let dir = scratch("missing");
        let editor = ConfigEditor::new(dir.clone());
        let store = store_with(&[("friendly_name", "Ada")]);

        import_store_prefs_once(&editor, &store).expect("import");
        let text = fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(
            text.starts_with("# goble configuration"),
            "the created file carries the documented header: {text}"
        );
        assert!(text.contains("[app]"), "{text}");
        assert!(text.contains("friendly_name = \"Ada\""), "{text}");
    }

    #[test]
    fn an_unknown_approval_label_is_skipped_and_the_rest_imports() {
        let dir = scratch("unknown-mode");
        let path = dir.join("config.toml");
        fs::write(&path, "[theme]\ndark = true\n").unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let store = store_with(&[("friendly_name", "Ada"), ("approval_mode", "yolo")]);

        let outcome = import_store_prefs_once(&editor, &store).expect("import");
        assert_eq!(outcome.keys(), &["friendly_name".to_string()][..]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("yolo"), "{text}");
        assert!(text.contains("friendly_name = \"Ada\""), "{text}");
    }
}
