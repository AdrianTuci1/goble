use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::store::Store;
use crate::subagent::{SubAgentId, SubAgentStatus};
use crate::subagent_run::PARENT_CANCEL_REASON;

use super::{
    background_args, harness_on, host_for, parent_chat, roles, settle, spawn_through_tool,
    wait_until_terminal, GateProvider,
};

#[tokio::test]
async fn cancelling_one_child_leaves_another_running() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let (llm, mut gates) = GateProvider::gated(&["first routine", "second routine"]);
    let host = host_for(&store, workspace.path(), llm);

    for prompt in ["first routine", "second routine"] {
        spawn_through_tool(
            &host,
            &store,
            workspace.path(),
            &parent_chat,
            background_args(prompt),
        )
        .await
        .expect("two background children");
    }
    let ids: Vec<SubAgentId> = host
        .registry
        .records()
        .into_iter()
        .map(|child| child.spec.id)
        .collect();
    assert_eq!(ids.len(), 2, "the registry lists both children");

    // Kill one of them, by id, with a reason the record carries.
    assert!(
        host.registry.cancel(&ids[0], "killed from the overlay"),
        "the child is live, so the kill lands"
    );
    let killed = host.registry.snapshot(&ids[0]).unwrap();
    match &killed.status {
        SubAgentStatus::Cancelled { reason } => assert_eq!(reason, "killed from the overlay"),
        other => panic!("expected Cancelled, got {other:?}"),
    }
    assert!(killed.status.is_terminal() && killed.finished_at.is_some());
    let survivor = host.registry.snapshot(&ids[1]).unwrap();
    assert!(
        survivor.status.is_running(),
        "the other child is untouched: {survivor:?}"
    );
    // A second kill of a terminal child reports itself as nothing done.
    assert!(
        !host.registry.cancel(&ids[0], "again"),
        "an already terminal child is not cancelled twice"
    );

    // The killed child's task stops at its next check instead of finishing:
    // it neither writes a reply nor turns Completed.
    gates.remove(0).send("too late".to_string()).unwrap();
    settle().await;
    assert!(
        matches!(
            host.registry.snapshot(&ids[0]).unwrap().status,
            SubAgentStatus::Cancelled { .. }
        ),
        "the cancellation is what the record says"
    );
    let rows = roles(&store, &ids[0].0);
    assert_eq!(
        rows.iter().filter(|(role, _)| role == "assistant").count(),
        0,
        "a cancelled child stops instead of finishing the turn: {rows:?}"
    );

    // The survivor runs to its own end.
    gates.remove(0).send("second done".to_string()).unwrap();
    let done = wait_until_terminal(&host, &ids[1]).await;
    assert!(
        matches!(&done.status, SubAgentStatus::Completed { output, .. }
            if output == "second done"),
        "{done:?}"
    );
}

#[tokio::test]
async fn the_harness_cancel_bit_reaches_every_child_and_its_accessors_see_them() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let (llm, gates) = GateProvider::gated(&["alpha", "beta"]);
    let harness = harness_on(store.clone(), workspace.path(), llm);
    let host = harness.subagent_host();

    for prompt in ["alpha", "beta"] {
        spawn_through_tool(
            &host,
            &store,
            workspace.path(),
            &parent_chat,
            background_args(prompt),
        )
        .await
        .expect("a background child");
    }
    assert_eq!(harness.subagent_records().len(), 2);
    let ids: Vec<SubAgentId> = harness
        .subagent_records()
        .into_iter()
        .map(|child| child.spec.id)
        .collect();
    for id in &ids {
        assert!(harness.subagent_record(id).unwrap().status.is_running());
    }

    // The parent's stop reaches both children through the shared bit.
    harness.cancel();
    for id in &ids {
        let record = harness.subagent_record(id).unwrap();
        assert!(
            matches!(&record.status, SubAgentStatus::Cancelled { reason }
                if reason == PARENT_CANCEL_REASON),
            "{id}: {record:?}"
        );
    }
    // ...and their loops stop: neither writes a reply after the gates open.
    for gate in gates {
        gate.send("never read".to_string()).unwrap();
    }
    settle().await;
    for id in &ids {
        let rows = roles(&store, &id.0);
        assert!(
            !rows.iter().any(|(role, _)| role == "assistant"),
            "a stopped child writes no turn: {id} {rows:?}"
        );
    }
}

#[tokio::test]
async fn the_parent_bit_set_by_hand_ends_a_live_child_without_a_registry_call() {
    // `Harness::with_cancel` hands the bit to the host, so a host can also be
    // stopped by someone holding only the Arc.
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let (llm, mut gates) = GateProvider::gated(&["stopped by the bit"]);
    let parent_bit = Arc::new(AtomicBool::new(false));
    let mut host = host_for(&store, workspace.path(), llm);
    host.parent_cancel = parent_bit.clone();
    spawn_through_tool(
        &host,
        &store,
        workspace.path(),
        &parent_chat,
        background_args("stopped by the bit"),
    )
    .await
    .expect("a background child");
    let id = host.registry.records()[0].spec.id.clone();

    parent_bit.store(true, Ordering::Relaxed);
    gates.remove(0).send("never read".to_string()).unwrap();
    let done = wait_until_terminal(&host, &id).await;
    assert!(
        matches!(&done.status, SubAgentStatus::Cancelled { reason }
            if reason == PARENT_CANCEL_REASON),
        "{done:?}"
    );
}
