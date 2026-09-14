use crate::harness::{execute_tool_call, MockCommandRunner, WebSearchConfig, SPAWN_SUBAGENT_TOOL};
use crate::mcp_manager::McpManager;
use crate::store::Store;
use crate::subagent::{SubAgentStatus, MAX_SUBAGENT_DEPTH};
use crate::subagent_run::run::child_tool_definitions;
use futures::StreamExt;

use super::{
    background_args, completion, harness_on, host_for, parent_chat, roles, spawn_args,
    spawn_through_tool, tool_call, turn_with, wait_until_terminal, GateProvider, ScriptedProvider,
};

#[tokio::test]
async fn a_spawn_at_the_depth_limit_is_refused_and_the_tool_is_withheld() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let llm = ScriptedProvider::scripted(Vec::new(), completion("", vec![]));
    let mcp_manager = McpManager::new();

    // The tool going away is the bound: at the limit the child's list has
    // no spawn_subagent; one level below it does.
    let at_limit = child_tool_definitions(MAX_SUBAGENT_DEPTH, &store, &mcp_manager);
    assert!(!at_limit.iter().any(|t| t.name == SPAWN_SUBAGENT_TOOL));
    let below = child_tool_definitions(MAX_SUBAGENT_DEPTH - 1, &store, &mcp_manager);
    assert!(below.iter().any(|t| t.name == SPAWN_SUBAGENT_TOOL));

    // And a call made anyway at the limit is refused before any child row,
    // directory or model turn exists.
    let host = host_for(&store, workspace.path(), llm);
    let call = tool_call("tc-deep", SPAWN_SUBAGENT_TOOL, spawn_args("deeper"));
    let result = execute_tool_call(
        &store,
        &MockCommandRunner,
        None,
        &mcp_manager,
        workspace.path(),
        workspace.path(),
        &call,
        &WebSearchConfig::default(),
        &turn_with(&host, &parent_chat, MAX_SUBAGENT_DEPTH),
    )
    .await;
    let error = result.expect_err("a spawn at the depth limit is refused");
    assert!(error.to_string().contains("refused"), "{error}");
    assert!(error.to_string().contains("depth"), "{error}");
    assert_eq!(store.list_chats().unwrap().len(), 1, "no child was opened");
    assert!(
        host.registry.records().is_empty(),
        "a refused spawn registers no child"
    );
}

#[tokio::test]
async fn a_background_spawn_returns_the_child_id_and_its_record_ends_completed() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let (llm, gates) = GateProvider::gated(&["review it later"]);
    let host = host_for(&store, workspace.path(), llm);

    let result = spawn_through_tool(
        &host,
        &store,
        workspace.path(),
        &parent_chat,
        background_args("review it later"),
    )
    .await
    .expect("a background spawn is accepted");

    // The call came back while the child was still parked on its first model
    // call: the id is in the result and the record is live.
    let children = host.registry.records();
    assert_eq!(children.len(), 1, "the spawn registered one child");
    let child = &children[0];
    let id = child.spec.id.clone();
    assert!(result.contains(&id.0), "{result}");
    assert!(result.contains("background"), "{result}");
    assert!(
        child.status.is_running() && !child.status.is_terminal(),
        "the record the caller sees is a live child: {child:?}"
    );
    assert!(child.spec.run_in_background);
    assert_eq!(child.spec.parent_chat_id, parent_chat);
    assert_eq!(child.spec.depth, 1, "the child runs one level deeper");
    assert_eq!(store.list_chats().unwrap().len(), 2, "the child owns a row");

    // Open the gate: the same record reaches Completed carrying the output.
    gates
        .into_iter()
        .next()
        .expect("one gate")
        .send("child final answer".to_string())
        .unwrap();
    let done = wait_until_terminal(&host, &id).await;
    match &done.status {
        SubAgentStatus::Completed {
            output,
            turns,
            tokens,
            duration_ms,
            ..
        } => {
            assert_eq!(output, "child final answer");
            assert_eq!(*turns, 1);
            assert!(*tokens > 0 && *duration_ms < 60_000, "{done:?}");
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    assert!(done.finished_at.is_some());
    // The background child wrote its own conversation, not the parent's.
    let child_rows = roles(&store, &id.0);
    assert_eq!(child_rows.len(), 2, "{child_rows:?}");
    assert_eq!(child_rows[0].0, "user");
    assert_eq!(
        child_rows[1],
        ("assistant".to_string(), "child final answer".to_string())
    );
    assert!(
        !roles(&store, &parent_chat)
            .iter()
            .any(|(_, content)| content == "review it later"),
        "the child's prompt never enters the parent's transcript"
    );
}

#[tokio::test]
async fn a_background_spawn_through_a_turn_ends_the_turn_while_the_child_is_parked() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let (llm, mut gates) = GateProvider::gated_with(
        &["parked routine"],
        vec![completion(
            "",
            vec![tool_call(
                "tc-bg",
                SPAWN_SUBAGENT_TOOL,
                background_args("parked routine"),
            )],
        )],
    );
    let harness = harness_on(store.clone(), workspace.path(), llm);
    let host = harness.subagent_host();

    let events: Vec<_> = harness
        .run_turn(&parent_chat, "spawn it and keep going", "mock", "model")
        .collect()
        .await;

    // The parent's turn ran to Done without waiting for the child: the tool
    // result it got is the child's id, and the child is still parked.
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::harness::HarnessEvent::Done)),
        "{events:?}"
    );
    let result = events
        .iter()
        .find_map(|event| match event {
            crate::harness::HarnessEvent::ToolCallFinished { result, .. } => {
                Some(result.clone())
            }
            _ => None,
        })
        .expect("the spawn call finished");
    let children = harness.subagent_records();
    assert_eq!(children.len(), 1, "{children:?}");
    let id = children[0].spec.id.clone();
    assert!(result.contains(&id.0), "{result}");
    assert!(children[0].status.is_running(), "{children:?}");
    assert_eq!(harness.subagent_record(&id).unwrap().spec.id, id);

    gates.remove(0).send("parked answer".to_string()).unwrap();
    let done = wait_until_terminal(&host, &id).await;
    assert!(
        matches!(&done.status, SubAgentStatus::Completed { output, .. }
            if output == "parked answer"),
        "{done:?}"
    );
}
