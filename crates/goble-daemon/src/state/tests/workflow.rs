use super::*;

// --- workflow wiring: DaemonPort::run_workflow + the harness run seam ---

/// The daemon runs a workflow end-to-end: the host journals the request,
/// runs the referenced workflow through the engine, and persists the run.
#[test]
fn run_workflow_journals_and_runs_through_the_host() {
    use goble_workflow::{MockStep, Step, Trigger, Workflow, WorkflowHostRequest, WorkflowId};

    let workflow = Workflow::new(WorkflowId::new("build"), "Build & release")
        .with_step(Step::new("checkout", "Checkout source"))
        .with_step(Step::new("build", "Compile artifacts"))
        .with_trigger(Trigger::Manual);

    let host = WorkflowHost::new(MockStep::new().fail_step("build"));
    let daemon = DaemonState::new(HarnessRegistry::new(), Arc::new(crate::sink::NoopSink));
    daemon.install_workflow_host(Arc::new(host));
    daemon.register_workflow(workflow).unwrap();

    let request = WorkflowHostRequest::new(
        0,
        "req-build-0",
        WorkflowId::new("build"),
        Trigger::Manual,
        serde_json::json!({}),
    );
    let run = daemon.run_workflow(request).unwrap();

    assert_eq!(
        run.status,
        goble_workflow::RunStatus::Failed("forced failure for build".into())
    );
    assert_eq!(run.steps.len(), 2);
    assert_eq!(run.steps[0].status, goble_workflow::StepStatus::Succeeded);
    assert_eq!(
        run.steps[1].status,
        goble_workflow::StepStatus::Failed("forced failure for build".into())
    );
}

#[test]
fn run_workflow_errors_without_an_installed_host() {
    use goble_workflow::{Trigger, WorkflowHostRequest, WorkflowId};
    let daemon = DaemonState::new(HarnessRegistry::new(), Arc::new(crate::sink::NoopSink));
    let request = WorkflowHostRequest::new(
        0,
        "req-0",
        WorkflowId::new("build"),
        Trigger::Manual,
        serde_json::json!({}),
    );
    assert!(daemon.run_workflow(request).is_err());
}

/// The installed executor runs each step by driving a harness turn through
/// the daemon run seam, so a real harness is invoked per step.
#[test]
fn installed_executor_runs_steps_through_a_real_harness() {
    use goble_workflow::{Step, Trigger, Workflow, WorkflowHostRequest, WorkflowId};

    let registry = HarnessRegistry::new();
    registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
    let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));
    daemon.install_workflow_executor(
        HarnessId::new("mock"),
        ProjectId::new("p1"),
        MediumId::new("local"),
        "wf",
    );
    daemon
        .register_workflow(
            Workflow::new(WorkflowId::new("greet"), "Greet")
                .with_step(Step::new("say", "Say hi"))
                .with_trigger(Trigger::Manual),
        )
        .unwrap();

    let request = WorkflowHostRequest::new(
        0,
        "req-greet-0",
        WorkflowId::new("greet"),
        Trigger::Manual,
        serde_json::json!({}),
    );
    // MockHarness emits a delta then Done, so the step settles as succeeded.
    let run = daemon.run_workflow(request).unwrap();
    assert_eq!(run.status, goble_workflow::RunStatus::Succeeded);
    assert_eq!(run.steps.len(), 1);
    assert_eq!(run.steps[0].status, goble_workflow::StepStatus::Succeeded);
}

#[test]
fn run_turn_blocking_reports_success_and_failure() {
    use goble_workflow::StepOutcome;

    struct ErrorHarness {
        id: HarnessId,
    }
    impl ErrorHarness {
        fn new(id: HarnessId) -> Self {
            Self { id }
        }
    }
    impl HarnessRuntime for ErrorHarness {
        fn id(&self) -> HarnessId {
            self.id.clone()
        }
        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::internal()
        }
        fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
            let session_id = turn.session_id;
            HarnessRun {
                events: Box::pin(futures::stream::iter(vec![
                    HarnessServerEvent::AssistantDelta {
                        session_id: session_id.clone(),
                        delta: "oops".to_string(),
                    },
                    HarnessServerEvent::Error {
                        session_id,
                        message: "step boom".to_string(),
                    },
                ])),
            }
        }
    }

    let registry = HarnessRegistry::new();
    registry.register(Arc::new(MockHarness::new(HarnessId::new("ok"), "hi")));
    registry.register(Arc::new(ErrorHarness::new(HarnessId::new("err"))));
    let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

    let ok = daemon.run_turn_blocking(HarnessTurn::new(
        HarnessId::new("ok"),
        SessionId::new("s1"),
        "x",
    ));
    assert_eq!(ok, StepOutcome::Succeeded);

    let err = daemon.run_turn_blocking(HarnessTurn::new(
        HarnessId::new("err"),
        SessionId::new("s2"),
        "x",
    ));
    assert_eq!(err, StepOutcome::Failed("step boom".to_string()));

    let missing = daemon.run_turn_blocking(HarnessTurn::new(
        HarnessId::new("nope"),
        SessionId::new("s3"),
        "x",
    ));
    assert_eq!(missing, StepOutcome::Failed("no harness resolved for nope".to_string()));
}
