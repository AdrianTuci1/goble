//! `~/.goble/environment.toml`: the named sets of variables a session starts
//! with, written by Settings -> Environment and editable by hand.
//!
//! Two levels only — group name -> variable name -> value — and the value is a
//! string or an inline `{ vault = "<key>" }` table naming a key an encrypted
//! vault holds. The vault form is written back exactly as it was read; the UI
//! never resolves it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One variable of a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnvValue {
    /// The value itself.
    Literal(String),
    /// The name of the vault key whose value it is.
    Vault { vault: String },
}

/// Group name -> variable name -> value. `BTreeMap` at both levels so a save
/// writes groups and variables in a stable alphabetical order and a hand edit
/// produces no churn.
pub type EnvironmentFile = BTreeMap<String, BTreeMap<String, EnvValue>>;

/// The header a seeded `environment.toml` carries. The comments are the whole
/// format: there is nothing to configure, only groups to add.
pub const ENVIRONMENT_HEADER: &str = "\
# goble environment groups: the named sets of variables a session starts with.
# Settings -> Environment shows this file, and its rows write it back, so the
# table below and the page are the same data. Edit it by hand if you like; press
# \"Reload from file\" on that page afterwards.
#
# One [groups.<name>] table per group. Every key in the table is one variable of
# that group, and its value is a string:
#
#   [groups.production]
#   LOG_LEVEL = \"info\"
#   API_BASE  = \"https://api.example.com\"
#
# A value that must not sit in this file is an inline table naming the key an
# encrypted vault holds instead; the value itself never appears here:
#
#   DEPLOY_TOKEN = { vault = \"deploy-token\" }
#
# A group's name is its table key, so `[groups.staging-eu]` needs no quoting and
# `[groups.\"Staging EU\"]` does. Deleting the table deletes the group.

";

/// Why a load refused the file. A single group that does not parse is not one of
/// these: it is dropped and reported, and the rest of the file still loads.
#[derive(Debug, thiserror::Error)]
pub enum EnvironmentError {
    /// The text is not TOML at all — a duplicate key lands here too, because TOML
    /// forbids it. Nothing in the file can be trusted, so the caller refuses to
    /// write over it rather than merging.
    #[error("not valid TOML: {0}")]
    NotToml(#[from] toml::de::Error),
}

/// The document layout, so the groups come out as `[groups.<name>]` tables.
#[derive(Serialize)]
struct Document<'a> {
    groups: &'a EnvironmentFile,
}

/// Parse a file body: keep every group that parses, drop the ones that do not.
///
/// One message per dropped group goes to the log; [`load_toml_with_problems`]
/// hands the same messages to a caller that shows them. `Err` only when the text
/// is not TOML at all, which is the caller's cue to keep the file it has.
pub fn load_toml(text: &str) -> Result<EnvironmentFile, EnvironmentError> {
    let (groups, problems) = load_toml_with_problems(text)?;
    for problem in &problems {
        tracing::warn!("environment.toml: {problem}");
    }
    Ok(groups)
}

/// [`load_toml`] plus one message per group it dropped.
pub fn load_toml_with_problems(
    text: &str,
) -> Result<(EnvironmentFile, Vec<String>), EnvironmentError> {
    let table: toml::Table = toml::from_str(text)?;
    let mut groups = EnvironmentFile::new();
    let mut problems = Vec::new();

    let Some(value) = table.get("groups") else {
        return Ok((groups, problems));
    };
    let toml::Value::Table(table) = value else {
        problems.push("[groups] ignored: expected a table of groups".to_string());
        return Ok((groups, problems));
    };

    for (name, value) in table {
        match parse_group(value) {
            Ok(variables) => {
                groups.insert(name.clone(), variables);
            }
            Err(reason) => problems.push(format!("[groups.{name}] ignored: {reason}")),
        }
    }
    Ok((groups, problems))
}

/// A group is a table whose every entry is a string or a vault reference. One bad
/// entry drops the whole group: the UI writes a group back as a unit, so a
/// half-read group would lose the entry it could not read.
fn parse_group(value: &toml::Value) -> std::result::Result<BTreeMap<String, EnvValue>, String> {
    let toml::Value::Table(table) = value else {
        return Err("expected a table of variables".to_string());
    };
    let mut variables = BTreeMap::new();
    for (name, value) in table {
        let parsed: EnvValue = value.clone().try_into().map_err(|_| {
            format!("{name} is not a string and not a {{ vault = \"key\" }} table")
        })?;
        variables.insert(name.clone(), parsed);
    }
    Ok(variables)
}

/// The file text: the header, then the groups. The same map always writes the
/// same bytes, so a save after a hand edit touches only what changed.
pub fn to_toml_with_header(groups: &EnvironmentFile) -> String {
    if groups.is_empty() {
        return ENVIRONMENT_HEADER.to_string();
    }
    let body = toml::to_string(&Document { groups })
        .expect("groups hold strings and inline tables, which TOML always represents");
    format!("{ENVIRONMENT_HEADER}{body}")
}

/// The one writer for the file, so its mode is set in one place: on Unix it is
/// `0600`, because a group's values are a session's secrets.
pub fn save(path: &Path, groups: &EnvironmentFile) -> Result<()> {
    write_private(path, to_toml_with_header(groups).as_bytes())
}

#[cfg(unix)]
fn write_private(path: &Path, text: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("write {}", path.display()))?;
    file.write_all(text)
        .with_context(|| format!("write {}", path.display()))?;

    // `mode` applies only to a file this call created; an existing file keeps the
    // mode it had, so it is set here too.
    let mut permissions = file
        .metadata()
        .with_context(|| format!("stat {}", path.display()))?
        .permissions();
    permissions.set_mode(0o600);
    file.set_permissions(permissions)
        .with_context(|| format!("chmod 0600 {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &[u8]) -> Result<()> {
    fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A private scratch directory. Tests run in parallel, so the name carries
    /// both the process id and the test's own name.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("goble-env-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    fn group(entries: &[(&str, EnvValue)]) -> BTreeMap<String, EnvValue> {
        entries
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect()
    }

    fn literal(text: &str) -> EnvValue {
        EnvValue::Literal(text.to_string())
    }

    #[test]
    fn literal_and_vault_round_trip_through_save_and_load() {
        let dir = temp_dir("round-trip");
        let path = dir.join("environment.toml");
        let mut file = EnvironmentFile::new();
        file.insert(
            "production".to_string(),
            group(&[
                ("API_BASE", literal("https://api.example.com")),
                (
                    "DEPLOY_TOKEN",
                    EnvValue::Vault {
                        vault: "deploy-token".to_string(),
                    },
                ),
                ("LOG_LEVEL", literal("info")),
            ]),
        );
        file.insert("staging-eu".to_string(), group(&[("LOG_LEVEL", literal("debug"))]));

        save(&path, &file).unwrap();
        let text = fs::read_to_string(&path).unwrap();

        // The vault form stays an inline table: the UI never resolves it, and a
        // save of another variable writes it back as it was.
        assert!(
            text.contains("DEPLOY_TOKEN = { vault = \"deploy-token\" }"),
            "{text}"
        );
        assert_eq!(load_toml(&text).unwrap(), file);
        assert!(load_toml_with_problems(&text).unwrap().1.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn empty_document_is_the_header_alone() {
        let text = to_toml_with_header(&EnvironmentFile::new());
        assert_eq!(text, ENVIRONMENT_HEADER);
        assert!(load_toml(&text).unwrap().is_empty());
    }

    #[test]
    fn header_documents_the_file() {
        assert!(ENVIRONMENT_HEADER.starts_with("# goble environment groups"));
        assert_eq!(ENVIRONMENT_HEADER.matches("[groups.production]").count(), 1);
        assert!(ENVIRONMENT_HEADER.contains("LOG_LEVEL = \"info\""));
        assert!(ENVIRONMENT_HEADER.contains("API_BASE  = \"https://api.example.com\""));
        assert!(ENVIRONMENT_HEADER.contains("DEPLOY_TOKEN = { vault = \"deploy-token\" }"));
        assert!(ENVIRONMENT_HEADER.contains("[groups.staging-eu]"));
        assert!(ENVIRONMENT_HEADER.contains("[groups.\"Staging EU\"]"));
        assert!(ENVIRONMENT_HEADER.contains("Deleting the table deletes the group."));
    }

    #[test]
    fn a_bad_group_is_dropped_with_one_message_and_the_others_load() {
        let text = format!(
            "{ENVIRONMENT_HEADER}\
[groups.alpha]
A = \"1\"

[groups.broken]
LIST = [\"a\", \"b\"]
MISSING = 3

[groups.zeta]
Z = \"9\"
"
        );

        let (loaded, problems) = load_toml_with_problems(&text).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("[groups.broken]"), "{problems:?}");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["alpha"]["A"], literal("1"));
        assert_eq!(loaded["zeta"]["Z"], literal("9"));
        assert_eq!(load_toml(&text).unwrap(), loaded);
    }

    #[test]
    fn a_group_that_is_not_a_table_is_dropped_too() {
        let text = "[groups.alpha]\nA = \"1\"\n\n[groups]\n";
        // `[groups.alpha]` already made `groups` a table, so use a scalar to make
        // the shape wrong on purpose.
        let text = format!("{text}broken = \"scalar\"\n");
        let (loaded, problems) = load_toml_with_problems(&text).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(loaded.len(), 1);

        let (loaded, problems) = load_toml_with_problems("groups = \"not a table\"\n").unwrap();
        assert!(loaded.is_empty());
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn a_vault_table_with_a_wrong_type_is_dropped() {
        let text = "[groups.alpha]\nTOKEN = { vault = 7 }\n";
        let (loaded, problems) = load_toml_with_problems(text).unwrap();
        assert!(loaded.is_empty());
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn a_duplicate_key_is_an_error_not_a_merge() {
        let text = "[groups.alpha]\nA = \"1\"\nA = \"2\"\n";
        let err = load_toml(text).unwrap_err();
        assert!(matches!(err, EnvironmentError::NotToml(_)), "{err}");
    }

    #[test]
    fn text_that_is_not_toml_is_an_error() {
        let err = load_toml("this is not toml\n").unwrap_err();
        assert!(matches!(err, EnvironmentError::NotToml(_)), "{err}");
    }

    #[test]
    fn save_writes_groups_in_a_stable_order() {
        let dir = temp_dir("order");
        let path = dir.join("environment.toml");
        let mut file = EnvironmentFile::new();
        file.insert("zeta".to_string(), group(&[("B", literal("2")), ("A", literal("1"))]));
        file.insert("alpha".to_string(), group(&[("C", literal("3"))]));

        let text = to_toml_with_header(&file);
        let expected = format!(
            "{ENVIRONMENT_HEADER}\
[groups.alpha]
C = \"3\"

[groups.zeta]
A = \"1\"
B = \"2\"
"
        );
        assert_eq!(text, expected);

        save(&path, &file).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        save(&path, &file).unwrap();
        let second = fs::read_to_string(&path).unwrap();
        assert_eq!(first, second);
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_saved_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("mode");
        let path = dir.join("environment.toml");

        // A file that already existed with a looser mode is tightened too.
        fs::write(&path, "").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        save(&path, &EnvironmentFile::new()).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        cleanup(&dir);
    }
}
