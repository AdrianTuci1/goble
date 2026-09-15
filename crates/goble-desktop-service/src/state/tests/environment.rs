//! Settings → Environment: the groups live in `~/.goble/environment.toml`, so
//! what these tests read back is the file, not the store.

use std::fs;

use goble_core::environment::ENVIRONMENT_HEADER;
use goble_core::store::SecretGroup;

use super::tmp_state_with_environment_file;

/// The variable names a group holds, in the order the page sees them.
fn variables(group: &SecretGroup) -> Vec<String> {
    group.entries.iter().map(|e| e.name.clone()).collect()
}

/// A group is created by name, the file holds its table, and its variables are
/// written into that table. The store gains nothing: the file is the identity.
#[test]
fn a_created_group_is_a_table_in_the_file() {
    let (_dir, state, store, file) = tmp_state_with_environment_file();
    assert!(
        state.environment_groups().unwrap().is_empty(),
        "no file yet: there are no groups"
    );

    let group = state.create_environment_group("production").unwrap();
    assert_eq!(group.id, "production", "a group's id is its name");
    assert_eq!(group.name, "production");
    assert!(group.entries.is_empty());
    assert!(
        state.create_environment_group("  ").is_err(),
        "a group needs a name"
    );

    let text = fs::read_to_string(&file).unwrap();
    assert!(text.starts_with(ENVIRONMENT_HEADER), "the header is written");
    assert!(text.contains("[groups.production]"), "{text}");

    state
        .save_environment_secret("production", None, "API_KEY", "sk-live")
        .unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("API_KEY = \"sk-live\""), "{text}");

    assert!(
        store.list_secret_groups().unwrap().is_empty(),
        "nothing writes the store's tables any more"
    );

    let groups = state.environment_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, "production");
    assert_eq!(variables(&groups[0]), ["API_KEY"]);
    assert_eq!(groups[0].entries[0].group_id, "production");
    assert_eq!(groups[0].entries[0].value, "sk-live");
}

/// A save writes the file through its one writer, and saving the same value
/// again leaves the bytes as they were: a hand edit sees no churn.
#[test]
fn a_save_writes_the_file_in_a_stable_order() {
    let (_dir, state, _store, file) = tmp_state_with_environment_file();
    state.create_environment_group("production").unwrap();
    state
        .save_environment_secret("production", None, "B", "2")
        .unwrap();
    state
        .save_environment_secret("production", None, "A", "1")
        .unwrap();

    let expected = format!("{ENVIRONMENT_HEADER}[groups.production]\nA = \"1\"\nB = \"2\"\n");
    assert_eq!(fs::read_to_string(&file).unwrap(), expected);

    state
        .save_environment_secret("production", None, "B", "2")
        .unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), expected);
}

/// An edit that renames a variable replaces its key in the table.
#[test]
fn a_rename_replaces_one_key() {
    let (_dir, state, _store, file) = tmp_state_with_environment_file();
    state.create_environment_group("production").unwrap();
    state
        .save_environment_secret("production", None, "API_KEY", "sk-live")
        .unwrap();

    let entry = state
        .save_environment_secret("production", Some("API_KEY"), "API_TOKEN", "sk-live")
        .unwrap();
    assert_eq!(entry.name, "API_TOKEN");

    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("API_KEY"), "the old key is gone: {text}");
    assert!(text.contains("API_TOKEN = \"sk-live\""), "{text}");
    assert_eq!(
        variables(&state.environment_groups().unwrap()[0]),
        ["API_TOKEN"]
    );
}

/// Deleting a variable takes its key out of the group it belongs to and leaves
/// a variable of the same name in another group alone; deleting a group takes
/// its table.
#[test]
fn a_delete_takes_its_own_row_out_of_the_file() {
    let (_dir, state, _store, file) = tmp_state_with_environment_file();
    state.create_environment_group("production").unwrap();
    state.create_environment_group("staging").unwrap();
    state
        .save_environment_secret("production", None, "API_KEY", "prod")
        .unwrap();
    state
        .save_environment_secret("staging", None, "API_KEY", "stage")
        .unwrap();

    // The id the page's delete control carries is the entry's own.
    let entry = state
        .environment_group("production")
        .unwrap()
        .unwrap()
        .entries[0]
        .id
        .clone();
    assert!(state.delete_environment_secret(&entry).unwrap());
    assert!(
        !state.delete_environment_secret(&entry).unwrap(),
        "the key is already gone"
    );

    let groups = state.environment_groups().unwrap();
    assert!(groups[0].entries.is_empty(), "production lost its variable");
    assert_eq!(groups[1].entries[0].value, "stage", "staging keeps its own");
    assert!(fs::read_to_string(&file).unwrap().contains("API_KEY = \"stage\""));

    assert!(state.delete_environment_group("staging").unwrap());
    assert!(!state.delete_environment_group("staging").unwrap());
    let text = fs::read_to_string(&file).unwrap();
    assert!(!text.contains("[groups.staging]"), "{text}");
    assert!(state.environment_group("staging").unwrap().is_none());

    // A secret cannot be added to a group that is not there.
    assert!(state.save_environment_secret("missing", None, "K", "V").is_err());
}

/// A `{ vault = .. }` value a hand edit wrote stays a reference: the pane never
/// resolves it, and a save of another variable writes it back as it was read.
#[test]
fn a_vault_reference_survives_a_write() {
    let (_dir, state, _store, file) = tmp_state_with_environment_file();
    fs::write(
        &file,
        "[groups.production]\nDEPLOY_TOKEN = { vault = \"deploy-token\" }\n",
    )
    .unwrap();

    let groups = state.environment_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].entries[0].name, "DEPLOY_TOKEN");
    assert_eq!(
        groups[0].entries[0].value, "{ vault = \"deploy-token\" }",
        "the pane shows the reference the file holds"
    );

    state
        .save_environment_secret("production", None, "LOG_LEVEL", "info")
        .unwrap();
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("DEPLOY_TOKEN = { vault = \"deploy-token\" }"),
        "{text}"
    );
    assert!(text.contains("LOG_LEVEL = \"info\""), "{text}");
    assert_eq!(state.environment_groups().unwrap()[0].entries.len(), 2);
}

/// An install that predates the file has its groups in the store: the first read
/// writes them into the file with the header, and no later read does it again.
#[test]
fn the_store_s_groups_are_migrated_into_the_file_once() {
    let (_dir, state, store, file) = tmp_state_with_environment_file();
    let now = "2026-01-01T00:00:00Z";
    store.create_secret_group("g1", "production", now).unwrap();
    store
        .upsert_secret_entry("e1", "g1", "API_KEY", "sk-live", now)
        .unwrap();

    let groups = state.environment_groups().unwrap();
    assert_eq!(groups.len(), 1, "the store's group is in the file now");
    assert_eq!(groups[0].id, "production", "named by its table");
    assert_eq!(groups[0].entries[0].value, "sk-live");

    let text = fs::read_to_string(&file).unwrap();
    assert!(text.starts_with(ENVIRONMENT_HEADER), "{text}");
    assert!(text.contains("[groups.production]"), "{text}");
    assert!(text.contains("API_KEY = \"sk-live\""), "{text}");

    // A second read finds groups in the file, so the store is not read again.
    assert_eq!(state.environment_groups().unwrap().len(), 1);
    assert_eq!(fs::read_to_string(&file).unwrap(), text);

    // The rows stay for a build that reads them again.
    assert_eq!(store.list_secret_groups().unwrap().len(), 1);
}

/// A file that is not TOML at all is never written over: the groups last read
/// are kept and every write is refused until the file parses.
#[test]
fn a_malformed_file_keeps_the_last_read_and_refuses_writes() {
    let (_dir, state, _store, file) = tmp_state_with_environment_file();
    state.create_environment_group("production").unwrap();
    state
        .save_environment_secret("production", None, "API_KEY", "sk-live")
        .unwrap();
    let good = fs::read_to_string(&file).unwrap();

    fs::write(&file, "this is not toml\n").unwrap();
    let malformed = fs::read_to_string(&file).unwrap();

    let groups = state.environment_groups().unwrap();
    assert_eq!(groups.len(), 1, "the groups last read are kept");
    assert_eq!(groups[0].entries[0].value, "sk-live");
    let problem = state.environment_problem().expect("the parse failure");
    assert!(problem.contains("not valid TOML"), "{problem}");
    assert!(problem.contains("environment.toml"), "{problem}");

    assert!(state.create_environment_group("staging").is_err());
    assert!(state
        .save_environment_secret("production", None, "B", "2")
        .is_err());
    assert!(state.delete_environment_group("production").is_err());
    let entry = groups[0].entries[0].id.clone();
    assert!(state.delete_environment_secret(&entry).is_err());
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        malformed,
        "the bytes of a file that cannot be parsed are left alone"
    );

    // A file that parses again is written again.
    fs::write(&file, good).unwrap();
    state.create_environment_group("staging").unwrap();
    assert!(
        fs::read_to_string(&file).unwrap().contains("[groups.staging]"),
        "the write lands once the file parses"
    );
}
