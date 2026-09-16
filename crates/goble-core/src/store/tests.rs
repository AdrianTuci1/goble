use super::*;

#[test]
fn test_in_memory_store() {
    let store = Store::open_in_memory().unwrap();
    store.set_setting("theme", "dark").unwrap();
    assert_eq!(
        store.get_setting("theme").unwrap(),
        Some("dark".to_string())
    );
    store.set_setting("theme", "light").unwrap();
    assert_eq!(
        store.get_setting("theme").unwrap(),
        Some("light".to_string())
    );
}

#[test]
fn test_chat_workspace_routing_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_chat("c1", "Demo", None, None, "2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z")
        .unwrap();
    assert_eq!(store.get_chat_workspace_routing("c1").unwrap(), None);

    store.set_chat_workspace_routing("c1", Some("local")).unwrap();
    assert_eq!(
        store.get_chat_workspace_routing("c1").unwrap(),
        Some("local".to_string())
    );

    store.set_chat_workspace_routing("c1", Some("remote")).unwrap();
    assert_eq!(
        store.get_chat_workspace_routing("c1").unwrap(),
        Some("remote".to_string())
    );

    // Clearing the choice is allowed (falls back to the default).
    store.set_chat_workspace_routing("c1", None).unwrap();
    assert_eq!(store.get_chat_workspace_routing("c1").unwrap(), None);

    // Unknown chats have no routing.
    assert_eq!(store.get_chat_workspace_routing("missing").unwrap(), None);
}

/// A conversation's title is written on its own and read back, so a subject
/// derived from the conversation survives a restart. Retitling leaves the
/// conversation's `updated_at` alone: the title says what the conversation is
/// about, not when it was last said in.
#[test]
fn test_chat_title_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_chat(
            "c1",
            "New conversation",
            None,
            None,
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
    assert_eq!(
        store.get_chat_title("c1").unwrap(),
        Some("New conversation".to_string())
    );

    store
        .set_chat_title("c1", "Initial user onboarding")
        .unwrap();
    assert_eq!(
        store.get_chat_title("c1").unwrap(),
        Some("Initial user onboarding".to_string())
    );
    let rows = store.list_chats().unwrap();
    assert_eq!(rows[0].1, "Initial user onboarding");
    assert_eq!(
        rows[0].5, "2024-01-01T00:00:00Z",
        "retitling is not activity: updated_at still says when it was last said in"
    );

    // The title is the conversation's own; a chat that does not exist has none.
    store.set_chat_title("missing", "Nowhere").unwrap();
    assert_eq!(store.get_chat_title("missing").unwrap(), None);
}

/// A conversation's reported token counts accumulate across its model calls and
/// are read back as one total. A provider that reports no cache accounting
/// leaves the cached figure absent rather than folding in a zero, and a chat
/// nothing has been reported for has no total at all (not a zero).
#[test]
fn test_chat_usage_accumulates_and_stays_absent_when_unreported() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_chat("c1", "Demo", None, None, "2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z")
        .unwrap();
    assert_eq!(
        store.chat_usage("c1").unwrap(),
        None,
        "a chat with no reported usage has no total"
    );
    assert_eq!(store.chat_usage("missing").unwrap(), None);

    store.add_chat_usage("c1", 1_200, Some(900), 80).unwrap();
    store.add_chat_usage("c1", 300, None, 20).unwrap();

    let usage = store.chat_usage("c1").unwrap().expect("the total");
    assert_eq!(usage.input, 1_500);
    assert_eq!(usage.cached, Some(900), "the cache-sharing call left it as it was");
    assert_eq!(usage.output, 100);
    assert_eq!(usage.total(), 1_600);
}

#[test]
fn test_credentials_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.get_credential("github_token").unwrap(), None);
    store.set_credential("github_token", "ghs_secret").unwrap();
    assert_eq!(
        store.get_credential("github_token").unwrap(),
        Some("ghs_secret".to_string())
    );
    assert_eq!(store.list_credential_names().unwrap(), vec!["github_token"]);
    // Upsert updates in place; the name set is unchanged.
    store.set_credential("github_token", "rotated").unwrap();
    assert_eq!(store.get_credential("github_token").unwrap(), Some("rotated".to_string()));
    assert_eq!(
        store.list_credential_names().unwrap(),
        vec!["github_token"]
    );
}

#[test]
fn test_access_grants_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_principal("p1", "user", "Ada", "2024-01-01T00:00:00Z")
        .unwrap();
    assert!(store.list_access("p1").unwrap().is_empty());

    store.grant_access("p1", "run", "workspace").unwrap();
    store.grant_access("p1", "read", "mcp:search").unwrap();
    let grants = store.list_access("p1").unwrap();
    assert_eq!(grants.len(), 2);
    assert!(grants.iter().any(|(g, s, _)| g == "run" && s == "workspace"));

    assert!(store.revoke_access("p1", "run", "workspace").unwrap());
    assert!(!store.revoke_access("p1", "run", "workspace").unwrap());
    let grants = store.list_access("p1").unwrap();
    assert_eq!(grants.len(), 1);
    assert!(grants.iter().all(|(g, _, _)| g != "run"));
}

#[test]
fn test_agent_crud() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_agent(
            "a1",
            "test-agent",
            "{}",
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
    let agents = store.list_agents().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].1, "test-agent");
    assert!(store.get_agent("a1").unwrap().is_some());
    store.delete_agent("a1").unwrap();
    assert!(store.get_agent("a1").unwrap().is_none());
}

#[test]
fn test_worker_crud() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_worker(
            "w1",
            "vps",
            Some("localhost:8787"),
            "unpaired",
            None,
            "{}",
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
    let workers = store.list_workers().unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].1, "vps");
    store.delete_worker("w1").unwrap();
    assert!(store.list_workers().unwrap().is_empty());
}

#[test]
fn test_chat_messages() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_chat(
            "c1",
            "Test",
            None,
            None,
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
    store
        .insert_chat_message("m1", "c1", "user", "hello", None, "2024-01-01T00:00:01Z")
        .unwrap();
    let msgs = store.list_chat_messages("c1").unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].2, "hello");
}

#[test]
fn test_mcp_server_crud() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_mcp_server(
            "mcp1",
            "files",
            "npm",
            Some("@modelcontextprotocol/server-files"),
            "{}",
            None,
            "[]",
            "[]",
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
    let servers = store.list_mcp_servers().unwrap();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].1, "files");
}

#[test]
fn test_agent_memory_crud_roundtrip() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.get_agent_memory("a1").unwrap().is_none());

    let mut memory = crate::agent_memory::AgentMemory::new("a1", "ship v1");
    memory.add_goal("implement memory");
    memory.record_decision("use sqlite", "simple");
    store.put_agent_memory(&memory).unwrap();

    let loaded = store.get_agent_memory("a1").unwrap().unwrap();
    assert_eq!(loaded.brief, "ship v1");
    assert_eq!(loaded.goals.len(), 1);
    assert_eq!(loaded.decisions.len(), 1);
    assert_eq!(loaded.version, memory.version);

    // Upsert updates in place.
    let mut updated = loaded.clone();
    updated.add_fact("new fact");
    store.put_agent_memory(&updated).unwrap();
    let reloaded = store.get_agent_memory("a1").unwrap().unwrap();
    assert_eq!(reloaded.facts.len(), 1);
    assert_eq!(store.list_agent_memories().unwrap().len(), 1);
}

#[test]
fn test_agent_memory_in_snapshot() {
    let store = Store::open_in_memory().unwrap();
    let mut memory = crate::agent_memory::AgentMemory::new("a1", "brief");
    memory.add_goal("goal");
    store.put_agent_memory(&memory).unwrap();

    let payload = store.export_snapshot_payload().unwrap();
    assert!(payload.tables.contains_key("agent_memory"));
    assert_eq!(payload.tables["agent_memory"].len(), 1);

    let store2 = Store::open_in_memory().unwrap();
    store2.import_snapshot_payload(payload).unwrap();
    let loaded = store2.get_agent_memory("a1").unwrap().unwrap();
    assert_eq!(loaded.brief, "brief");
    assert_eq!(loaded.goals.len(), 1);
}

#[test]
fn test_team_crud() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_team("t1", "Platform", "{}", "2024-01-01T00:00:00Z")
        .unwrap();
    let teams = store.list_teams().unwrap();
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0].1, "Platform");
    store.insert_team_member("t1", "a1").unwrap();
    let members = store.list_team_members("t1").unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].1, "a1");
}

#[test]
fn test_vault_secret_crud() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_vault_secret("api_key", b"secret", "{}", "2024-01-01T00:00:00Z")
        .unwrap();
    let secrets = store.list_vault_secrets().unwrap();
    assert_eq!(secrets.len(), 1);
    assert_eq!(secrets[0].0, "api_key");
    assert_eq!(secrets[0].1, b"secret");
}

#[test]
fn test_workflow_crud() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_workflow(
            "wf1",
            "Deploy",
            "Deploy app",
            "{}",
            "manual",
            true,
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
    let workflows = store.list_workflows().unwrap();
    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].1, "Deploy");
    assert!(workflows[0].5);
    store.delete_workflow("wf1").unwrap();
    assert!(store.list_workflows().unwrap().is_empty());
}

#[test]
fn test_execution_crud() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_execution(
            "e1",
            Some("a1"),
            Some("w1"),
            "running",
            "{}",
            "2024-01-01T00:00:00Z",
            None,
        )
        .unwrap();
    let execs = store.list_executions().unwrap();
    assert_eq!(execs.len(), 1);
    assert_eq!(execs[0].3, "running");
}

#[test]
fn test_audit_log_roundtrip() {
    use crate::audit::{AuditCategory, AuditEntry};
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("store.db")).unwrap();
    let entry = AuditEntry::new(
        "audit-1",
        "2026-08-14T00:00:00Z",
        AuditCategory::Identity,
        "device-1",
        "cluster_created",
    )
    .with_detail("cluster_name", "prod");
    store.append_audit_log(&entry).unwrap();
    let loaded = store.list_audit_logs(Some(10)).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].action, "cluster_created");
    assert_eq!(loaded[0].details.get("cluster_name").unwrap(), "prod");
}

#[test]
fn test_snapshot_export_import_roundtrip() {
    use crate::snapshot::Snapshot;
    use crate::worker::WorkerId;

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("store.db");
    let store1 = Store::open(&path).unwrap();
    store1.set_setting("hello", "world").unwrap();
    store1
        .insert_agent(
            "a1",
            "agent",
            "{}",
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();

    let key = crate::cluster_key::ClusterKey::generate();
    let worker_id = WorkerId::generate();
    let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();

    let path2 = tmp.path().join("store2.db");
    let store2 = Store::open(&path2).unwrap();
    snapshot.restore_into_store(&store2, &key).unwrap();

    assert_eq!(
        store2.get_setting("hello").unwrap(),
        Some("world".to_string())
    );
    assert_eq!(store2.list_agents().unwrap().len(), 1);
}

#[test]
fn test_identity_wallet_roundtrip_in_snapshot() {
    use crate::cluster_key::ClusterKey;
    use crate::encrypted_wallet::IdentityWallet;
    use crate::snapshot::Snapshot;
    use crate::worker::WorkerId;

    let tmp = tempfile::tempdir().unwrap();
    let store1 = Store::open(tmp.path().join("store1.db")).unwrap();
    let identity = IdentityWallet::new(
        ClusterKey::generate().to_base64(),
        "test-cluster",
        "ca-cert-pem",
        "ca-key-pem",
    );
    let sealed = identity.seal(b"passphrase").unwrap();
    store1.set_cluster_wallet(&sealed).unwrap();

    let key = ClusterKey::generate();
    let worker_id = WorkerId::generate();
    let snapshot = Snapshot::from_store(&store1, &worker_id, &key).unwrap();

    let store2 = Store::open(tmp.path().join("store2.db")).unwrap();
    snapshot.restore_into_store(&store2, &key).unwrap();

    let loaded = store2
        .get_cluster_wallet()
        .unwrap()
        .expect("wallet missing");
    let opened = IdentityWallet::open(&loaded, b"passphrase").unwrap();
    assert_eq!(opened, identity);
}

#[test]
fn update_agent_changes_name_and_spec() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("store.db")).unwrap();
    let id = "agent-1";
    store
        .insert_agent(
            id,
            "Old",
            "{}",
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
    let spec = r#"{"prompt":"hello"}"#;
    store
        .update_agent(id, "New", spec, "2024-02-01T00:00:00Z")
        .unwrap();
    let agent = store.get_agent(id).unwrap().unwrap();
    assert_eq!(agent.1, "New");
    assert_eq!(agent.2, spec);
}

/// An environment group is a thing of its own: it is created, listed with its
/// entries, edited in place by (group, name), and deleted with every entry it
/// holds. A group that is deleted takes its entries with it, and an entry that
/// is renamed does not leave the old name behind.
#[test]
fn test_secret_groups_roundtrip_and_lifecycle() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.list_secret_groups().unwrap().is_empty());

    store
        .create_secret_group("g1", "production", "2024-01-01T00:00:00Z")
        .unwrap();
    store
        .create_secret_group("g2", "staging", "2024-01-01T00:00:00Z")
        .unwrap();

    // A fresh group has no entries, and comes back with an empty list rather
    // than being absent.
    let groups = store.list_secret_groups().unwrap();
    assert_eq!(groups.len(), 2, "both groups are listed, ordered by name");
    assert_eq!(groups[0].name, "production");
    assert_eq!(groups[1].name, "staging");
    assert!(groups[0].entries.is_empty());

    store
        .upsert_secret_entry("e1", "g1", "API_KEY", "sk-live", "2024-01-01T00:00:01Z")
        .unwrap();
    store
        .upsert_secret_entry("e2", "g1", "DB_URL", "postgres://x", "2024-01-01T00:00:02Z")
        .unwrap();
    store
        .upsert_secret_entry("e3", "g2", "API_KEY", "sk-stage", "2024-01-01T00:00:03Z")
        .unwrap();

    let production = store.get_secret_group("g1").unwrap().expect("production");
    assert_eq!(production.entries.len(), 2);
    // Entries are ordered by name, and each one belongs to its own group.
    assert_eq!(production.entries[0].name, "API_KEY");
    assert_eq!(production.entries[0].value, "sk-live");
    assert_eq!(production.entries[1].name, "DB_URL");
    assert!(production.entries.iter().all(|e| e.group_id == "g1"));
    let staging = store.get_secret_group("g2").unwrap().expect("staging");
    assert_eq!(staging.entries.len(), 1);
    assert_eq!(staging.entries[0].value, "sk-stage");

    // The same name in the same group edits the entry rather than duplicating
    // it; the same name in another group is a different entry.
    store
        .upsert_secret_entry("e9", "g1", "API_KEY", "sk-rotated", "2024-01-02T00:00:00Z")
        .unwrap();
    let production = store.get_secret_group("g1").unwrap().unwrap();
    assert_eq!(production.entries.len(), 2, "the name did not duplicate");
    assert_eq!(production.entries[0].value, "sk-rotated");

    // Renaming an entry removes the name it used to carry.
    assert!(store.delete_secret_entry_named("g1", "DB_URL").unwrap());
    assert!(!store.delete_secret_entry_named("g1", "DB_URL").unwrap());
    store
        .upsert_secret_entry("e4", "g1", "DATABASE_URL", "postgres://y", "2024-01-02T00:00:01Z")
        .unwrap();
    let names: Vec<String> = store
        .get_secret_group("g1")
        .unwrap()
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, vec!["API_KEY".to_string(), "DATABASE_URL".to_string()]);

    // Removing one entry leaves the others. (`e2` is already gone: the rename
    // above removed the entry that carried `DB_URL`.)
    assert!(!store.delete_secret_entry("e2").unwrap());
    assert!(store.delete_secret_entry("e4").unwrap());
    assert!(!store.delete_secret_entry("e4").unwrap());
    assert_eq!(store.get_secret_group("g1").unwrap().unwrap().entries.len(), 1);

    // Deleting a group removes its entries and leaves the other groups alone.
    assert!(store.delete_secret_group("g1").unwrap());
    assert!(!store.delete_secret_group("g1").unwrap());
    assert!(store.get_secret_group("g1").unwrap().is_none());
    let remaining = store.list_secret_groups().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].name, "staging");
    assert_eq!(remaining[0].entries.len(), 1);
    assert!(
        store
            .list_secret_groups()
            .unwrap()
            .iter()
            .all(|g| g.entries.iter().all(|e| e.group_id != "g1")),
        "no entry outlives the group it belonged to"
    );
}
