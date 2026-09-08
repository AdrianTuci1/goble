//! Scripted workflow engine model for the daemon.
//!
//! This is the `types`-layer crate for Goble's workflow subsystem. A workflow
//! is a named, ordered list of steps that fires on a [`Trigger`]; the daemon's
//! host accepts a [`WorkflowHostRequest`] to run one. The host drives execution
//! through a [`WorkflowEngine`], which runs each step in order against a
//! [`StepExecutor`], settles the run, and returns a [`WorkflowRun`] whose status
//! log records how every step resolved.
//!
//! The crate is deliberately framework-agnostic: it owns only model-shaped
//! data plus a narrow execution seam (`StepExecutor`), with no dependence on
//! `app/`, `goble-ui`, `goble-core`, `wgpu`, or `winit`. Concrete behavior is
//! injected by the caller; [`MockStep`] is provided as a scripted test double
//! that decides outcomes by step id/name, so the engine can be exercised
//! deterministically without a real payload. It sits in the `types` layer of
//! the `types <- protocol <- runtime` split; runtime concerns (transport,
//! scheduling, persistence) belong to a higher crate.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Identifies a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(pub String);

impl WorkflowId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for WorkflowId {
    fn default() -> Self {
        Self::new("default")
    }
}

/// Identifies a single step within a workflow (unique inside a [`Workflow`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StepId(pub String);

impl StepId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for StepId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What causes a workflow to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    /// Fired by a person (the default).
    Manual,
    /// Fired on a cron schedule.
    Schedule {
        cron: String,
    },
    /// Fired when a named event is observed.
    Event {
        event: String,
    },
}

impl Default for Trigger {
    fn default() -> Self {
        Trigger::Manual
    }
}

/// One step in a workflow: a named, addressable unit the engine performs.
///
/// The step is declarative metadata only. How it executes is determined by the
/// [`StepExecutor`] handed to the engine, so a step can be read, serialized,
/// and scheduled without binding this crate to any concrete action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub id: StepId,
    pub name: String,
}

impl Step {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: StepId::new(id),
            name: name.into(),
        }
    }
}

/// A scripted workflow: an ordered list of [`Step`]s that fires on a [`Trigger`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub steps: Vec<Step>,
    pub trigger: Trigger,
}

impl Workflow {
    pub fn new(id: WorkflowId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            steps: Vec::new(),
            trigger: Trigger::Manual,
        }
    }

    pub fn with_steps(mut self, steps: Vec<Step>) -> Self {
        self.steps = steps;
        self
    }

    pub fn with_step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }

    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// Validate the workflow: it must be non-empty and have unique step ids.
    pub fn validate(&self) -> Result<(), WorkflowError> {
        if self.steps.is_empty() {
            return Err(WorkflowError::NoSteps(self.id.clone()));
        }
        let mut seen = HashMap::new();
        for step in &self.steps {
            if seen.insert(step.id.clone(), ()).is_some() {
                return Err(WorkflowError::DuplicateStepId(step.id.clone()));
            }
        }
        Ok(())
    }

    /// Deserialize a workflow from a JSON string.
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Serialize a workflow to a JSON string.
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

/// Validation errors for a [`Workflow`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowError {
    #[error("workflow {0:?} has no steps")]
    NoSteps(WorkflowId),
    #[error("step id {0} is duplicated")]
    DuplicateStepId(StepId),
}

/// The outcome a [`StepExecutor`] reports for a single step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Succeeded,
    Failed(String),
}

/// The status of a single step within a [`WorkflowRun`] status log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Not yet run (a later step when an earlier one failed).
    Skipped,
    Succeeded,
    Failed(String),
}

/// The overall status of a [`WorkflowRun`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed(String),
}

/// One entry in a run's status log: how a single step settled, in run order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepExecution {
    /// 0-based position of the step in the workflow.
    pub index: usize,
    pub step_id: StepId,
    pub name: String,
    pub status: StepStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// The result of running a workflow: a status plus an ordered status log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub workflow_id: WorkflowId,
    pub status: RunStatus,
    pub steps: Vec<StepExecution>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A request to run a workflow, as journaled by the workflow host.
///
/// The host appends these opaquely so they survive a restart and can be
/// replayed; the [`WorkflowHostRequest::seq`] is the monotonic journal position
/// and the `request_id` is a stable per-entry identity. The `payload` is an
/// arbitrary JSON blob the host interprets (e.g. event arguments), kept as
/// `serde_json::Value` so this crate never needs to know the shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowHostRequest {
    pub seq: u64,
    pub request_id: String,
    pub workflow_id: WorkflowId,
    pub trigger: Trigger,
    pub payload: serde_json::Value,
    pub submitted_at: DateTime<Utc>,
}

impl WorkflowHostRequest {
    /// A new journal entry. Callers own `seq` (tracked by the host's journal);
    /// `request_id` is generated by the caller as a stable per-entry identity.
    pub fn new(
        seq: u64,
        request_id: impl Into<String>,
        workflow_id: WorkflowId,
        trigger: Trigger,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            seq,
            request_id: request_id.into(),
            workflow_id,
            trigger,
            payload,
            submitted_at: Utc::now(),
        }
    }

    /// Encode one journal entry as a single newline-terminated JSON line.
    pub fn to_line(&self) -> anyhow::Result<String> {
        let mut line = serde_json::to_string(self)?;
        line.push('\n');
        Ok(line)
    }

    /// Decode a single line of JSON into a journal entry.
    pub fn from_line(line: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str::<Self>(line.trim())?)
    }
}

/// Runs a single workflow step and reports how it settled.
///
/// This is the execution seam the [`WorkflowEngine`] drives. It is kept small so
/// a host can inject any behavior (a real action, a subprocess, a script) while
/// the engine remains deterministic about *which* steps run and in *what order*.
pub trait StepExecutor: Send + Sync {
    fn run(&self, step: &Step) -> StepOutcome;
}

/// A scripted test double for [`StepExecutor`].
///
/// Outcomes are chosen by step id (falling back to step name), so a test can
/// say "step `b` fails" without a real action. Any step without a mapped
/// outcome succeeds by default. Because the engine owns the run's status log,
/// `MockStep` is immutable per run: it decides outcomes but records nothing.
#[derive(Debug, Clone, Default)]
pub struct MockStep {
    outcomes: HashMap<String, StepOutcome>,
}

impl MockStep {
    pub fn new() -> Self {
        Self::default()
    }

    /// Map a step (by id or name) to a fixed outcome.
    pub fn with_outcome(mut self, step: impl Into<String>, outcome: StepOutcome) -> Self {
        self.outcomes.insert(step.into(), outcome);
        self
    }

    /// Make the step identified by `step` fail with a descriptive message.
    pub fn fail_step(mut self, step: impl Into<String>) -> Self {
        let key = step.into();
        self.outcomes
            .insert(key.clone(), StepOutcome::Failed(format!("forced failure for {key}")));
        self
    }

    fn outcome_for(&self, step: &Step) -> StepOutcome {
        self.outcomes
            .get(&step.id.0)
            .or_else(|| self.outcomes.get(&step.name))
            .cloned()
            .unwrap_or(StepOutcome::Succeeded)
    }
}

impl StepExecutor for MockStep {
    fn run(&self, step: &Step) -> StepOutcome {
        self.outcome_for(step)
    }
}

/// A deterministic workflow runner.
///
/// Runs the workflow's steps in declaration order, stopping at the first
/// failure. A failing step marks the run [`RunStatus::Failed`] and leaves any
/// not-yet-reached steps marked [`StepStatus::Skipped`]; if every step
/// succeeds the run settles as [`RunStatus::Succeeded`]. The produced
/// [`WorkflowRun`] is a full status log keyed to the workflow's step order.
pub struct WorkflowEngine {
    executor: Box<dyn StepExecutor>,
}

impl WorkflowEngine {
    /// Build an engine that uses `executor` to run each step.
    pub fn new(executor: impl StepExecutor + 'static) -> Self {
        Self {
            executor: Box::new(executor),
        }
    }

    /// Build an engine with a default [`MockStep`] (every step succeeds).
    pub fn with_default_executor() -> Self {
        Self::new(MockStep::new())
    }

    /// Run `workflow` against the configured executor.
    pub fn run(&self, workflow: &Workflow) -> WorkflowRun {
        let started_at = Utc::now();
        let mut steps = Vec::with_capacity(workflow.steps.len());
        let mut status = RunStatus::Running;
        let mut finished_at = None;

        for (index, step) in workflow.steps.iter().enumerate() {
            let step_started = Utc::now();
            let (taken, failed) = match self.executor.run(step) {
                StepOutcome::Succeeded => (StepStatus::Succeeded, None),
                StepOutcome::Failed(msg) => (
                    StepStatus::Failed(msg.clone()),
                    Some(msg),
                ),
            };
            let finished = Utc::now();
            steps.push(StepExecution {
                index,
                step_id: step.id.clone(),
                name: step.name.clone(),
                status: taken,
                started_at: step_started,
                finished_at: Some(finished),
            });

            if let Some(msg) = failed {
                status = RunStatus::Failed(msg);
                finished_at = Some(finished);
                for (i, remaining) in workflow.steps.iter().enumerate().skip(index + 1) {
                    steps.push(StepExecution {
                        index: i,
                        step_id: remaining.id.clone(),
                        name: remaining.name.clone(),
                        status: StepStatus::Skipped,
                        started_at: finished,
                        finished_at: Some(finished),
                    });
                }
                break;
            }
        }

        if status == RunStatus::Running {
            status = RunStatus::Succeeded;
            finished_at = Some(Utc::now());
        }

        WorkflowRun {
            workflow_id: workflow.id.clone(),
            status,
            steps,
            started_at,
            finished_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow() -> Workflow {
        Workflow::new(WorkflowId::new("build"), "Build & release")
            .with_step(Step::new("checkout", "Checkout source"))
            .with_step(Step::new("build", "Compile artifacts"))
            .with_step(Step::new("release", "Publish release"))
            .with_trigger(Trigger::Schedule { cron: "0 8 * * *".into() })
    }

    #[test]
    fn steps_run_in_declared_order_and_succeed() {
        let wf = workflow();
        let engine = WorkflowEngine::with_default_executor();
        let run = engine.run(&wf);

        assert_eq!(run.status, RunStatus::Succeeded);
        assert_eq!(run.workflow_id, WorkflowId::new("build"));
        assert_eq!(run.steps.len(), 3);
        assert_eq!(run.steps[0].index, 0);
        assert_eq!(run.steps[0].name, "Checkout source");
        // The engine runs steps in the order they were declared.
        let order: Vec<_> = run.steps.iter().map(|s| s.step_id.clone()).collect();
        assert_eq!(
            order,
            vec![StepId::new("checkout"), StepId::new("build"), StepId::new("release")]
        );
        assert!(run.steps.iter().all(|s| s.status == StepStatus::Succeeded));
        assert!(run.finished_at.is_some());
    }

    #[test]
    fn spec_also_matches_the_success_status_log() {
        let wf = workflow();
        let engine = WorkflowEngine::new(MockStep::new().with_outcome("build", StepOutcome::Succeeded));
        let run = engine.run(&wf);
        assert_eq!(run.status, RunStatus::Succeeded);
        assert_eq!(run.steps.len(), 3);
        for s in &run.steps {
            assert_eq!(s.status, StepStatus::Succeeded);
        }
    }

    #[test]
    fn a_failing_step_marks_the_run_failed_and_skips_the_rest() {
        let wf = workflow();
        let engine = WorkflowEngine::new(MockStep::new().fail_step("build"));
        let run = engine.run(&wf);

        assert_eq!(run.status, RunStatus::Failed("forced failure for build".into()));
        assert_eq!(run.steps.len(), 3);
        assert_eq!(run.steps[0].status, StepStatus::Succeeded);
        assert_eq!(run.steps[1].status, StepStatus::Failed("forced failure for build".into()));
        // Steps after the failure are recorded as skipped, already settled.
        assert_eq!(run.steps[2].status, StepStatus::Skipped);
        assert!(run.finished_at.is_some());
    }

    #[test]
    fn mock_step_outcome_requires_no_real_action() {
        let wf = workflow();
        let engine = WorkflowEngine::new(MockStep::new().with_outcome("release", StepOutcome::Failed("no creds".into())));
        let run = engine.run(&wf);
        assert_eq!(run.status, RunStatus::Failed("no creds".into()));
        assert_eq!(run.steps.last().unwrap().status, StepStatus::Failed("no creds".into()));
    }

    #[test]
    fn workflow_serde_roundtrip() {
        let wf = workflow();
        let json = serde_json::to_string(&wf).unwrap();
        let decoded: Workflow = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, wf);
        assert_eq!(decoded.trigger, Trigger::Schedule { cron: "0 8 * * *".into() });
        assert_eq!(decoded.steps.len(), 3);
    }

    #[test]
    fn workflow_json_helpers_roundtrip() {
        let wf = workflow();
        let json = wf.to_json().unwrap();
        let decoded = Workflow::from_json(&json).unwrap();
        assert_eq!(decoded, wf);
    }

    #[test]
    fn workflow_run_serde_roundtrip() {
        let engine = WorkflowEngine::with_default_executor();
        let run = engine.run(&workflow());
        let json = serde_json::to_string(&run).unwrap();
        let decoded: WorkflowRun = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, run);
    }

    #[test]
    fn host_request_serde_roundtrip() {
        let request = WorkflowHostRequest::new(
            3,
            "req-1",
            WorkflowId::new("build"),
            Trigger::Event { event: "push".into() },
            serde_json::json!({"ref": "refs/heads/main"}),
        );
        let json = serde_json::to_string(&request).unwrap();
        let decoded: WorkflowHostRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, request);
        assert_eq!(decoded.seq, 3);
        assert_eq!(decoded.request_id, "req-1");
    }

    #[test]
    fn host_request_line_framing_roundtrip() {
        let request = WorkflowHostRequest::new(
            0,
            "req-0",
            WorkflowId::new("build"),
            Trigger::Manual,
            serde_json::json!({}),
        );
        let line = request.to_line().unwrap();
        assert!(line.ends_with('\n'));
        let decoded = WorkflowHostRequest::from_line(&line).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn validate_rejects_empty_and_duplicate_steps() {
        assert!(matches!(
            Workflow::new(WorkflowId::new("empty"), "x").validate(),
            Err(WorkflowError::NoSteps(_))
        ));

        let dup = Workflow::new(WorkflowId::new("dup"), "x")
            .with_step(Step::new("a", "A"))
            .with_step(Step::new("a", "A again"));
        assert!(matches!(
            dup.validate(),
            Err(WorkflowError::DuplicateStepId(id)) if id == StepId::new("a")
        ));

        assert!(workflow().validate().is_ok());
    }
}
