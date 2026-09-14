use std::time::Duration;

use crate::store::Store;
use crate::subagent::{SubAgentBudget, SubAgentStatus};
use crate::subagent_run::default_child_budget;

use super::{
    background_args, completion, credentials_call_forever, host_for, host_with_wait, parent_chat,
    roles, run_direct, spec_for, spawn_args, spawn_through_tool, tool_call, wait_until_terminal,
    GateProvider, ScriptedProvider,
};

#[tokio::test]
async fn a_turn_budget_exhausted_mid_run_ends_the_child_terminal_with_the_reason() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    // An always-tool-calling provider would loop forever; the budget stops
    // it. max_turns 3 charges twice, so the child gets exactly 2 turns.
    let llm = ScriptedProvider::loops_forever(credentials_call_forever());
    let record = run_direct(
        &store,
        workspace.path(),
        llm,
        spec_for(
            "child-turns",
            &workspace.path().join("child-turns"),
            SubAgentBudget::new(3, 100, 1_000_000),
        ),
    )
    .await;

    assert!(record.status.is_terminal());
    match &record.status {
        SubAgentStatus::Failed { error } => {
            assert!(
                error.contains("exhausted") && error.contains("turns"),
                "{error}"
            );
        }
        other => panic!("budget exhaustion must end the child Failed, got {other:?}"),
    }
    assert!(record.finished_at.is_some());
    let rows = roles(&store, "child-turns");
    let assistants = rows.iter().filter(|(role, _)| role == "assistant").count();
    let tools = rows.iter().filter(|(role, _)| role == "tool").count();
    assert_eq!(
        (assistants, tools),
        (2, 2),
        "the child stopped at its turn budget instead of looping: {rows:?}"
    );
}

#[tokio::test]
async fn token_and_tool_call_budgets_also_end_the_child_terminal() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let long = "x".repeat(400);
    // Tokens: the first reply alone passes the 40-token ceiling
    // (estimate_tokens: 400 chars / 4), so nothing of it persists.
    let llm = ScriptedProvider::loops_forever(completion(
        &long,
        vec![tool_call("tc1", "credentials", serde_json::json!({}))],
    ));
    let spec = spec_for(
        "child-tokens",
        &workspace.path().join("child-tokens"),
        SubAgentBudget::new(100, 100, 40),
    );
    let record = run_direct(&store, workspace.path(), llm, spec).await;
    assert!(matches!(&record.status,
        SubAgentStatus::Failed { error } if error.contains("tokens")
    ));
    assert_eq!(
        roles(&store, "child-tokens")
            .iter()
            .filter(|(role, _)| role == "assistant")
            .count(),
        0,
        "the turn that broke the ceiling is not persisted"
    );

    // Tool calls: max_tool_calls 1 allows zero calls — the first
    // reservation is already at the limit.
    let llm = ScriptedProvider::loops_forever(credentials_call_forever());
    let spec = spec_for(
        "child-calls",
        &workspace.path().join("child-calls"),
        SubAgentBudget::new(100, 1, 1_000_000),
    );
    let record = run_direct(&store, workspace.path(), llm, spec).await;
    assert!(matches!(&record.status,
        SubAgentStatus::Failed { error } if error.contains("tool_calls")
    ));
    let rows = roles(&store, "child-calls");
    assert_eq!(
        rows.iter().filter(|(role, _)| role == "tool").count(),
        0,
        "the reserved-but-refused call never ran: {rows:?}"
    );
}

#[tokio::test]
async fn a_foreground_child_past_its_wait_moves_to_the_background() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let (llm, gates) = GateProvider::gated(&["slow routine"]);
    // A child slower than the foreground budget is not killed: the call
    // returns its id and the run continues to its own end.
    let host = host_with_wait(&store, workspace.path(), llm, Duration::from_millis(20));
    let result = spawn_through_tool(
        &host,
        &store,
        workspace.path(),
        &parent_chat,
        spawn_args("slow routine"),
    )
    .await
    .expect("the foreground spawn returns either way");

    let children = host.registry.records();
    let id = children[0].spec.id.clone();
    assert!(result.contains(&id.0), "{result}");
    assert!(result.contains("background"), "{result}");
    assert!(
        host.registry.snapshot(&id).unwrap().status.is_running(),
        "the child outliving the wait is still live"
    );

    gates
        .into_iter()
        .next()
        .unwrap()
        .send("late answer".to_string())
        .unwrap();
    let done = wait_until_terminal(&host, &id).await;
    assert!(
        matches!(&done.status, SubAgentStatus::Completed { output, .. }
            if output == "late answer"),
        "{done:?}"
    );
}

#[tokio::test]
async fn a_budget_exhausted_background_child_is_terminal_in_the_registry() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    // A child that would loop forever on tool calls; only its budget stops it.
    let llm = ScriptedProvider::loops_forever(credentials_call_forever());
    let host = host_for(&store, workspace.path(), llm);
    spawn_through_tool(
        &host,
        &store,
        workspace.path(),
        &parent_chat,
        background_args("burn the budget"),
    )
    .await
    .expect("a background child");

    let id = host.registry.records()[0].spec.id.clone();
    let done = wait_until_terminal(&host, &id).await;
    assert!(
        matches!(&done.status, SubAgentStatus::Failed { error }
            if error.contains("exhausted") && error.contains("turns")),
        "budget exhaustion is terminal in the registry: {done:?}"
    );
    assert!(done.finished_at.is_some());
    // The turn ceiling is what stopped it: every turn the budget allowed ran,
    // and the one at the ceiling never reached the model.
    let rows = roles(&store, &id.0);
    assert_eq!(
        rows.iter().filter(|(role, _)| role == "assistant").count(),
        default_child_budget().max_turns as usize - 1,
        "the child stopped on its turn ceiling, not on the test: {rows:?}"
    );
}
