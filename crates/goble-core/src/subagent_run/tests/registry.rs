use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::harness::HarnessEvent;
use crate::llm::TokenUsage;
use crate::subagent::{SubAgentBudget, SubAgentId, SubAgentRecord, SubAgentStatus};
use crate::subagent_run::cancel::ChildCancel;
use crate::subagent_run::SubAgentRegistry;

use super::spec_for;

#[tokio::test]
async fn killing_a_child_also_stops_the_children_it_spawned() {
    let workspace = tempfile::TempDir::new().unwrap();
    let registry = SubAgentRegistry::default();
    let parent_bit = Arc::new(AtomicBool::new(false));
    let mut ids = Vec::new();
    for (id, parent) in [
        ("child-a", "parent-chat"),
        ("child-b", "parent-chat"),
        ("grandchild", "child-a"),
    ] {
        let mut spec = spec_for(
            id,
            &workspace.path().join(id),
            SubAgentBudget::new(10, 10, 1_000),
        );
        spec.parent_chat_id = parent.to_string();
        let mut record = SubAgentRecord::new(spec);
        record.begin_running();
        registry.register(record.clone(), ChildCancel::new(parent_bit.clone()));
        ids.push(SubAgentId(id.to_string()));
    }

    assert!(registry.cancel(&ids[0], "killed from the overlay"));
    for killed in [&ids[0], &ids[2]] {
        assert!(
            matches!(&registry.snapshot(killed).unwrap().status,
                SubAgentStatus::Cancelled { reason } if reason == "killed from the overlay"),
            "{killed} is a descendant of the killed child"
        );
    }
    assert!(
        registry.snapshot(&ids[1]).unwrap().status.is_running(),
        "the sibling keeps running"
    );
    assert!(
        !registry.cancel(&SubAgentId("never-spawned".to_string()), "stranger"),
        "an unknown id cancels nothing"
    );
}

/// A background child spends with no turn listening: its calls wait on the
/// registry and are handed to the next turn that attaches, once each, under the
/// parent conversation's id.
#[tokio::test]
async fn spend_with_no_turn_attached_waits_for_the_next_one() {
    let workspace = tempfile::TempDir::new().unwrap();
    let registry = SubAgentRegistry::default();
    let spec = spec_for(
        "child-bg",
        workspace.path(),
        SubAgentBudget::new(10, 10, 1_000),
    );
    let parent_bit = Arc::new(AtomicBool::new(false));
    registry.register(
        SubAgentRecord::new(spec),
        ChildCancel::new(parent_bit.clone()),
    );

    // No sink yet: the child's one call is recorded but goes nowhere.
    registry.note_child_usage(
        "child-bg",
        "parent-chat",
        &TokenUsage {
            input: 900,
            cached: Some(700),
            output: 40,
        },
    );

    let mut sink = registry.attach_event_sink();
    match sink.try_recv().expect("the unreported call is handed over") {
        HarnessEvent::TokenUsage {
            chat_id,
            input,
            cached,
            output,
        } => {
            assert_eq!(chat_id, "parent-chat");
            assert_eq!(input, 900);
            assert_eq!(cached, Some(700));
            assert_eq!(output, 40);
        }
        other => panic!("expected the child's spend, got {other:?}"),
    }
    assert!(
        sink.try_recv().is_err(),
        "the same spend is never reported twice"
    );

    // With a turn listening the next call crosses immediately, and the turn
    // after it has nothing left to hand over.
    registry.note_child_usage(
        "child-bg",
        "parent-chat",
        &TokenUsage {
            input: 100,
            cached: None,
            output: 5,
        },
    );
    match sink.try_recv().expect("a live call crosses at once") {
        HarnessEvent::TokenUsage {
            input,
            cached,
            output,
            ..
        } => {
            assert_eq!((input, cached, output), (100, None, 5));
        }
        other => panic!("expected the child's live call, got {other:?}"),
    }
    let mut next = registry.attach_event_sink();
    assert!(
        next.try_recv().is_err(),
        "everything the child spent has already been reported"
    );
}
