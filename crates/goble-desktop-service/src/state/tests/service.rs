use goble_core::agent::Trigger;
use goble_core::protocol::WorkerMessage;
use goble_core::store::Store;
use goble_core::workflow::WorkflowStep;

use crate::state::*;

use super::tmp_state;

#[test]
fn test_state_add_worker() {
    let (_dir, state) = tmp_state();
    let wid = WorkerId::generate();
    state
        .add_worker(
            wid.clone(),
            "vps".to_string(),
            "wss://localhost:8787/ws".to_string(),
        )
        .unwrap();
    let workers = state.list_workers();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].name, "vps");
    assert!(!workers[0].paired);
    state.remove_worker(&wid);
    assert!(state.list_workers().is_empty());
}

#[test]
fn screen_registry_has_local_capturer_and_controller() {
    let (_dir, state) = tmp_state();
    let reg = state.screen_registry();
    assert!(
        reg.capturer(goble_screen_adapter::LOCAL_SOURCE).is_some(),
        "a local capturer must be registered on DesktopState"
    );
    assert!(
        reg.controller(goble_screen_adapter::LOCAL_SOURCE).is_some(),
        "a local controller must be registered on DesktopState"
    );
    assert_eq!(
        reg.capturer_sources(),
        vec![goble_screen_adapter::LOCAL_SOURCE.to_string()]
    );
}

#[test]
#[cfg(not(feature = "remote-screen"))]
fn open_remote_screen_without_feature_errors() {
    let (_dir, state) = tmp_state();
    let err = state
        .open_remote_screen(goble_harness_types::RemoteScreenConfig::new("vm", "u", "p"))
        .unwrap_err();
    assert!(
        err.to_string().contains("remote-screen"),
        "the error should name the missing feature, got: {err}"
    );
}

#[test]
#[cfg(feature = "remote-screen")]
fn open_remote_screen_registers_a_remote_pair() {
    // The client connects lazily on its own thread; registering the
    // capturer+controller is synchronous, so a dead endpoint still yields a
    // registered (blank) source. A connection-refused endpoint fails fast.
    let (_dir, state) = tmp_state();
    let source = state
        .open_remote_screen(goble_harness_types::RemoteScreenConfig::new(
            "127.0.0.1", "u", "p",
        ))
        .unwrap();
    let reg = state.screen_registry();
    assert!(reg.capturer(&source).is_some(), "remote capturer registered");
    assert!(reg.controller(&source).is_some(), "remote controller registered");
    assert!(reg.capturer_sources().contains(&source));
}

#[test]
fn test_state_logs_and_chat() {
    let (_dir, state) = tmp_state();
    state.add_log("hello");
    assert_eq!(state.get_logs().len(), 1);
    let chat_id = state.create_chat("Test chat", None, None).unwrap();
    state
        .add_chat_message(&chat_id, "user", "hello agent")
        .unwrap();
    let msgs = state.list_chat_messages(&chat_id).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].content, "hello agent");
}

#[test]
fn test_state_agent_and_workflow() {
    let (_dir, state) = tmp_state();
    let agent = state
        .create_agent("greeter", "say hello", Some("test agent"), vec![])
        .unwrap();
    assert_eq!(state.list_agents().len(), 1);
    let step = WorkflowStep {
        id: uuid::Uuid::new_v4().to_string(),
        name: "greet".to_string(),
        agent_id: AgentId(agent.id.clone()),
        input_template: "Greet the user".to_string(),
        depends_on: vec![],
    };
    let wf = state
        .create_workflow("hello", "Hello workflow", vec![step], Trigger::Manual)
        .unwrap();
    assert_eq!(state.list_workflows().len(), 1);
    assert_eq!(wf.steps.len(), 1);
    state.delete_workflow(&WorkflowId(wf.id)).unwrap();
    assert!(state.list_workflows().is_empty());
    state.delete_agent(&AgentId(agent.id)).unwrap();
    assert!(state.list_agents().is_empty());
}

#[test]
fn test_state_team_and_vault() {
    let (_dir, state) = tmp_state();
    state.set_vault_passphrase("passphrase".to_string());
    state.set_vault_secret("api_key", "sk-123").unwrap();
    assert_eq!(state.list_vault_secrets().len(), 1);
    state
        .create_team("team1", "Platform", "{}", vec!["a1".to_string()])
        .unwrap();
    assert_eq!(state.list_teams().len(), 1);
}

#[test]
fn test_worker_message_handling() {
    let (_dir, state) = tmp_state();
    let wid = WorkerId::generate();
    state
        .add_worker(
            wid.clone(),
            "vps".to_string(),
            "ws://localhost:8787/ws".to_string(),
        )
        .unwrap();
    state.handle_worker_message(&wid, WorkerMessage::Paired);
    assert!(state.list_workers()[0].paired);
    state.handle_worker_message(
        &wid,
        WorkerMessage::AgentLog {
            trace_id: "t1".to_string(),
            step_id: "s1".to_string(),
            level: goble_core::execution::LogLevel::Info,
            message: "hello".to_string(),
        },
    );
    assert_eq!(state.get_logs().len(), 2);
}

#[test]
fn test_agent_workflow_team_vault_roundtrip() {
    let (_dir, state) = tmp_state();
    state.set_vault_passphrase("secret".to_string());
    let agent = state
        .create_agent("greeter", "say hello", Some("test agent"), vec![])
        .unwrap();
    let step = WorkflowStep {
        id: uuid::Uuid::new_v4().to_string(),
        name: "greet".to_string(),
        agent_id: AgentId(agent.id.clone()),
        input_template: "Greet the user".to_string(),
        depends_on: vec![],
    };
    let wf = state
        .create_workflow(
            "hello",
            "Hello workflow",
            vec![step],
            goble_core::agent::Trigger::Manual,
        )
        .unwrap();
    assert_eq!(state.list_agents().len(), 1);
    assert_eq!(state.list_workflows().len(), 1);

    state
        .create_team("team1", "Platform", "{}", vec![agent.id.clone()])
        .unwrap();
    assert_eq!(state.list_teams().len(), 1);
    assert_eq!(state.list_teams()[0].members.len(), 1);

    state.set_vault_secret("api_key", "sk-123").unwrap();
    assert_eq!(state.list_vault_secrets().len(), 1);

    state.delete_workflow(&WorkflowId(wf.id)).unwrap();
    state.delete_agent(&AgentId(agent.id)).unwrap();
    assert!(state.list_workflows().is_empty());
    assert!(state.list_agents().is_empty());
}

#[test]
fn test_persistence_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("store.db")).unwrap();
    let state = DesktopState::new(
        store,
        crate::thread_store::ThreadStore::new(tmp.path().join("threads")).unwrap(),
    );
    state.set_vault_passphrase("p".to_string());
    let agent = state.create_agent("a", "prompt", None, vec![]).unwrap();
    let step = WorkflowStep {
        id: uuid::Uuid::new_v4().to_string(),
        name: "s".to_string(),
        agent_id: AgentId(agent.id.clone()),
        input_template: "in".to_string(),
        depends_on: vec![],
    };
    state
        .create_workflow("wf", "desc", vec![step], goble_core::agent::Trigger::Manual)
        .unwrap();
    state
        .create_team("t", "Team", "{}", vec![agent.id])
        .unwrap();
    state.set_vault_secret("k", "v").unwrap();

    let state2 = DesktopState::new(
        Store::open(tmp.path().join("store.db")).unwrap(),
        crate::thread_store::ThreadStore::new(tmp.path().join("threads")).unwrap(),
    );
    state2.load_from_store().unwrap();
    assert_eq!(state2.list_agents().len(), 1);
    assert_eq!(state2.list_workflows().len(), 1);
    assert_eq!(state2.list_teams().len(), 1);
    assert_eq!(state2.list_vault_secrets().len(), 1);
}

#[test]
fn test_cluster_identity_encrypted_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("store.db")).unwrap();
    let state = DesktopState::new(
        store,
        crate::thread_store::ThreadStore::new(tmp.path().join("threads")).unwrap(),
    );
    let identity = state.create_cluster("test-cluster", "secret-pass").unwrap();
    assert!(!identity.cluster_name.is_empty());

    let state2 = DesktopState::new(
        Store::open(tmp.path().join("store.db")).unwrap(),
        crate::thread_store::ThreadStore::new(tmp.path().join("threads")).unwrap(),
    );
    assert!(state2.has_stored_cluster_identity());
    assert!(state2.unlock_cluster_identity("wrong").is_err());
    assert!(state2.unlock_cluster_identity("secret-pass").unwrap());
    let loaded = state2.get_cluster_identity().unwrap();
    assert_eq!(loaded.cluster_name, identity.cluster_name);
}

#[test]
fn migrate_legacy_chats_creates_threads() {
    let (_dir, state) = tmp_state();
    // Create a legacy chat with two messages
    state.create_chat("legacy chat", None, None).unwrap();
    let chats = state.list_chats();
    let chat = &chats[0];
    state.add_chat_message(&chat.id, "user", "hello").unwrap();
    state
        .add_chat_message(&chat.id, "user", "hi there")
        .unwrap();

    let threads = state.migrate_legacy_chats_to_threads().unwrap();
    assert_eq!(threads.len(), 1);
    let summary = &threads[0];
    assert_eq!(summary.title, "legacy chat");

    let messages = state
        .thread_store()
        .list_messages(&goble_core::thread::ThreadId(summary.id.clone()))
        .unwrap();
    assert_eq!(messages.len(), 2);
}
