//! Workflow wiring: a harness-backed [`StepExecutor`] and a journaling host.
//!
//! `goble-workflow` owns the engine model; this module supplies the daemon-side
//! halves. [`HarnessStepExecutor`] is a [`StepExecutor`] that runs each step by
//! building a [`HarnessTurn`] from the step and invoking the daemon's run seam
//! (a synchronous "run this turn and tell me how it settled" entry point),
//! returning the resulting [`StepOutcome`]. [`WorkflowHost`] journals each
//! [`WorkflowHostRequest`], looks up the referenced [`Workflow`], runs it through
//! the engine, and persists the resulting [`WorkflowRun`].
//!
//! Like `goble-workflow`, this stays framework-agnostic: it depends only on the
//! harness-seam crates and the workflow engine model — no `goble-core`, no UI.

use std::sync::Mutex;

use goble_harness_types::{HarnessId, HarnessTurn, MediumId, ProjectId, SessionId};
use goble_workflow::{
    Step, StepExecutor, StepOutcome, Workflow, WorkflowEngine, WorkflowHostRequest, WorkflowId,
    WorkflowRun,
};

type RunSeam = dyn Fn(HarnessTurn) -> StepOutcome + Send + Sync;

/// A [`StepExecutor`] that drives each step through the daemon's run seam.
///
/// Each step is turned into a [`HarnessTurn`] on a fresh session (derived from
/// `session_prefix` plus a per-step counter), whose goal is the step's name. The
/// turn is handed to `run_seam`, which the daemon supplies as its synchronous
/// "run a turn to completion" entry point; whatever [`StepOutcome`] the seam
/// reports is the step's outcome.
pub struct HarnessStepExecutor {
    harness_id: HarnessId,
    project_id: ProjectId,
    medium_id: MediumId,
    session_prefix: String,
    run_seam: std::sync::Arc<RunSeam>,
    counter: Mutex<u64>,
}

impl HarnessStepExecutor {
    /// Build an executor that runs steps on `harness_id`, scoped to
    /// `project_id`/`medium_id`, using `run_seam` to actually run each turn.
    pub fn new(
        harness_id: HarnessId,
        project_id: ProjectId,
        medium_id: MediumId,
        session_prefix: impl Into<String>,
        run_seam: std::sync::Arc<RunSeam>,
    ) -> Self {
        Self {
            harness_id,
            project_id,
            medium_id,
            session_prefix: session_prefix.into(),
            run_seam,
            counter: Mutex::new(0),
        }
    }
}

impl StepExecutor for HarnessStepExecutor {
    fn run(&self, step: &Step) -> StepOutcome {
        let index = {
            let mut counter = self.counter.lock().unwrap();
            let i = *counter;
            *counter += 1;
            i
        };
        let session_id = SessionId::new(format!("{}-{}", self.session_prefix, index));
        let mut turn = HarnessTurn::new(self.harness_id.clone(), session_id, step.name.clone());
        turn.project_id = self.project_id.clone();
        turn.medium_id = self.medium_id.clone();
        (self.run_seam)(turn)
    }
}

/// The daemon-side workflow host: journal + workflow store + run persistence.
///
/// The host owns the [`WorkflowEngine`] (built once from a [`StepExecutor`]),
/// a registry of known [`Workflow`]s, an append-only journal of accepted
/// [`WorkflowHostRequest`]s, and a ledger of resulting [`WorkflowRun`]s. A
/// request is journaled, the referenced workflow is looked up and run through
/// the engine, and the run is persisted before being returned.
pub struct WorkflowHost {
    engine: WorkflowEngine,
    workflows: Mutex<Vec<Workflow>>,
    journal: Mutex<Vec<WorkflowHostRequest>>,
    runs: Mutex<Vec<WorkflowRun>>,
    next_seq: Mutex<u64>,
}

impl WorkflowHost {
    /// Build a host whose engine runs steps with `executor`.
    pub fn new(executor: impl StepExecutor + 'static) -> Self {
        Self {
            engine: WorkflowEngine::new(executor),
            workflows: Mutex::new(Vec::new()),
            journal: Mutex::new(Vec::new()),
            runs: Mutex::new(Vec::new()),
            next_seq: Mutex::new(0),
        }
    }

    /// Register a workflow so it can be looked up and run. The workflow must
    /// validate (non-empty, unique step ids).
    pub fn register_workflow(&self, workflow: Workflow) -> anyhow::Result<()> {
        workflow.validate()?;
        let mut workflows = self.workflows.lock().unwrap();
        if workflows.iter().any(|w| w.id == workflow.id) {
            anyhow::bail!("workflow {} is already registered", workflow.id);
        }
        workflows.push(workflow);
        Ok(())
    }

    /// The registered workflows, in registration order.
    pub fn workflows(&self) -> Vec<Workflow> {
        self.workflows.lock().unwrap().clone()
    }

    /// Look up a registered workflow by id.
    pub fn workflow(&self, id: &WorkflowId) -> Option<Workflow> {
        self.workflows
            .lock()
            .unwrap()
            .iter()
            .find(|w| &w.id == id)
            .cloned()
    }

    /// Journal a request: assign the next monotonic sequence and append it.
    pub fn journal(&self, request: WorkflowHostRequest) -> WorkflowHostRequest {
        let seq = {
            let mut next = self.next_seq.lock().unwrap();
            let s = *next;
            *next += 1;
            s
        };
        let mut entry = request;
        entry.seq = seq;
        self.journal.lock().unwrap().push(entry.clone());
        entry
    }

    /// The journal, in append order.
    pub fn journal_entries(&self) -> Vec<WorkflowHostRequest> {
        self.journal.lock().unwrap().clone()
    }

    /// Persisted workflow runs, in the order they were run.
    pub fn runs(&self) -> Vec<WorkflowRun> {
        self.runs.lock().unwrap().clone()
    }

    /// Run the workflow referenced by `request`: journal it, look the workflow
    /// up, run it through the engine, persist the run, and return it.
    ///
    /// The request's `trigger` records what caused the run; the host dispatches
    /// on the referenced workflow regardless of trigger kind (the fired trigger
    /// is carried in the journal for auditing and replay).
    pub fn run(&self, request: WorkflowHostRequest) -> anyhow::Result<WorkflowRun> {
        let entry = self.journal(request);
        let workflow = self
            .workflow(&entry.workflow_id)
            .ok_or_else(|| anyhow::anyhow!("no workflow registered for {}", entry.workflow_id))?;
        let run = self.engine.run(&workflow);
        self.runs.lock().unwrap().push(run.clone());
        Ok(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_workflow::{MockStep, Step, Trigger};

    fn build_workflow(id: &str) -> Workflow {
        Workflow::new(WorkflowId::new(id), "Build & release")
            .with_step(Step::new("checkout", "Checkout source"))
            .with_step(Step::new("build", "Compile artifacts"))
            .with_step(Step::new("release", "Publish release"))
            .with_trigger(Trigger::Manual)
    }

    fn request_for(id: &str, seq: u64) -> WorkflowHostRequest {
        WorkflowHostRequest::new(
            seq,
            format!("req-{id}-{seq}"),
            WorkflowId::new(id),
            Trigger::Manual,
            serde_json::json!({}),
        )
    }

    #[test]
    fn host_runs_steps_in_declared_order_and_persists_the_run() {
        let host = WorkflowHost::new(MockStep::new());
        host.register_workflow(build_workflow("build")).unwrap();

        let run = host.run(request_for("build", 0)).unwrap();

        assert_eq!(run.workflow_id, WorkflowId::new("build"));
        assert_eq!(run.status, goble_workflow::RunStatus::Succeeded);
        let order: Vec<_> = run.steps.iter().map(|s| s.step_id.clone()).collect();
        assert_eq!(
            order,
            vec![
                goble_workflow::StepId::new("checkout"),
                goble_workflow::StepId::new("build"),
                goble_workflow::StepId::new("release"),
            ]
        );
        assert!(run.steps.iter().all(|s| s.status == goble_workflow::StepStatus::Succeeded));

        // The run is persisted and the request journaled with a monotonic seq.
        assert_eq!(host.runs().len(), 1);
        let entry = &host.journal_entries()[0];
        assert_eq!(entry.seq, 0);
        assert_eq!(entry.request_id, "req-build-0");
        assert_eq!(host.workflow(&WorkflowId::new("build")).map(|w| w.steps.len()), Some(3));
    }

    #[test]
    fn a_failing_step_marks_the_run_failed() {
        let host = WorkflowHost::new(MockStep::new().fail_step("build"));
        host.register_workflow(build_workflow("build")).unwrap();

        let run = host.run(request_for("build", 0)).unwrap();

        assert_eq!(
            run.status,
            goble_workflow::RunStatus::Failed("forced failure for build".into())
        );
        assert_eq!(run.steps[0].status, goble_workflow::StepStatus::Succeeded);
        assert_eq!(
            run.steps[1].status,
            goble_workflow::StepStatus::Failed("forced failure for build".into())
        );
        // Steps after the failure are recorded as skipped.
        assert_eq!(run.steps[2].status, goble_workflow::StepStatus::Skipped);
        // The failed run is persisted.
        assert_eq!(host.runs().len(), 1);
        assert_eq!(
            host.runs()[0].status,
            goble_workflow::RunStatus::Failed("forced failure for build".into())
        );
    }

    #[test]
    fn host_rejects_unknown_workflow_and_invalid_workflow() {
        let host = WorkflowHost::new(MockStep::new());
        assert!(host.run(request_for("nope", 0)).is_err());

        let empty = Workflow::new(WorkflowId::new("empty"), "x");
        assert!(host.register_workflow(empty).is_err());
    }

    #[test]
    fn host_assigns_monotonic_sequence_across_runs() {
        let host = WorkflowHost::new(MockStep::new());
        host.register_workflow(build_workflow("build")).unwrap();

        host.run(request_for("build", 0)).unwrap();
        host.run(request_for("build", 0)).unwrap();

        let seqs: Vec<u64> = host.journal_entries().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![0, 1]);
    }

    #[test]
    fn harness_executor_builds_a_turn_and_reports_the_seam_outcome() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<HarnessTurn>::new()));
        let seen_for_seam = std::sync::Arc::clone(&seen);
        let run_seam: std::sync::Arc<RunSeam> = std::sync::Arc::new(move |turn| {
            seen_for_seam.lock().unwrap().push(turn);
            StepOutcome::Succeeded
        });
        let executor = HarnessStepExecutor::new(
            HarnessId::new("cli"),
            ProjectId::new("p1"),
            MediumId::new("remote"),
            "wf",
            run_seam,
        );

        let outcome = executor.run(&Step::new("checkout", "Checkout source"));

        assert_eq!(outcome, StepOutcome::Succeeded);
        let turns = seen.lock().unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].harness_id, HarnessId::new("cli"));
        assert_eq!(turns[0].project_id, ProjectId::new("p1"));
        assert_eq!(turns[0].medium_id, MediumId::new("remote"));
        assert_eq!(turns[0].goal, "Checkout source");
        assert_eq!(turns[0].session_id.0, "wf-0");
    }

    #[test]
    fn harness_executor_reports_a_failing_seam() {
        let run_seam: std::sync::Arc<RunSeam> = std::sync::Arc::new(|_| {
            StepOutcome::Failed("boom".to_string())
        });
        let executor = HarnessStepExecutor::new(
            HarnessId::new("cli"),
            ProjectId::new("p1"),
            MediumId::new("local"),
            "wf",
            run_seam,
        );
        assert_eq!(
            executor.run(&Step::new("build", "Build")),
            StepOutcome::Failed("boom".to_string())
        );
    }
}
