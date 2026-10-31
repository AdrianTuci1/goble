//! Running a stored workflow from the desktop service.
//!
//! The store keeps the workflow definition (`spec` JSON). A run converts it to
//! the engine model, drives it through a [`WorkflowHost`] whose agent seam is
//! the embedded daemon, and persists the resulting status log in
//! `workflow_runs` so the UI can show a real run instead of standing the
//! definition in for one.

use std::sync::Arc;

use anyhow::Context as _;
use chrono::Utc;
use goble_core::agent::Trigger as CoreTrigger;
use goble_core::workflow::Workflow as CoreWorkflow;
use goble_daemon::WorkflowHost;
use goble_harness_types::{HarnessId, MediumId, ProjectId};
use goble_workflow::{
    RunStatus, StepOutcome, StepStatus, Workflow, WorkflowHostRequest, WorkflowId, WorkflowRun,
};

use super::{DesktopState, WorkflowRunInfo, WorkflowRunStep};

impl DesktopState {
    /// Read the persisted definition of `workflow_id`.
    fn stored_workflow(&self, workflow_id: &str) -> anyhow::Result<CoreWorkflow> {
        let rows = self.store.lock().list_workflows()?;
        let row = rows
            .into_iter()
            .find(|(id, ..)| id == workflow_id)
            .with_context(|| format!("workflow {workflow_id} not found"))?;
        Ok(serde_json::from_str(&row.3)?)
    }

    /// Start one run of `workflow_id` on a background thread. The run is
    /// persisted and `workflows:updated` is emitted when it settles.
    ///
    /// A workflow with a Rhai body runs that body; a step-list workflow runs
    /// its steps through the same daemon seam.
    pub fn run_workflow(self: &Arc<Self>, workflow_id: &str) -> anyhow::Result<()> {
        let spec = self.stored_workflow(workflow_id)?;
        let workflow = engine_workflow(&spec)?;
        workflow.validate().map_err(|e| anyhow::anyhow!("{e}"))?;

        let run_id = uuid::Uuid::new_v4().to_string();
        let harness_id = HarnessId::new(format!("workflow-{run_id}"));
        let project = ProjectId::new("local");
        let medium = MediumId::new("local");

        // A workflow turn uses the configured default model, falling back to
        // the same provider resolution a chat turn uses.
        let model = self.config_default_model().unwrap_or_default();
        let (llm, model_name) = self.resolve_llm_provider("openai", &model);
        let runner = Arc::new(
            goble_core::harness::SandboxedCommandRunner::default_tools()
                .with_sandbox(goble_core::harness::harness_sandbox()),
        );
        // Clone the store into its own binding: the guard held by
        // `self.store.lock().clone()` would outlive the `let` and deadlock the
        // settings reads below, which take the same mutex.
        let store = self.store.lock().clone();
        let internal = goble_harness_internal::InternalHarness::new(store)
            .with_llm(llm)
            .with_provider("internal")
            .with_model(&model_name)
            .with_auto_approve(self.get_auto_approve())
            .with_web_search(self.get_web_search_setting())
            .with_runner(runner)
            .with_id(harness_id.clone());
        self.daemon_state.register(Arc::new(internal));

        let text_weak = Arc::downgrade(self);
        let text_seam: Arc<goble_daemon::TextRunSeam> =
            Arc::new(move |turn| match text_weak.upgrade() {
                Some(state) => state.daemon_state.run_turn_blocking_with_output(turn),
                None => (
                    StepOutcome::Failed("the desktop went away mid-run".to_string()),
                    String::new(),
                ),
            });
        let step_weak = Arc::downgrade(self);
        let step_seam: Arc<dyn Fn(goble_harness_types::HarnessTurn) -> StepOutcome + Send + Sync> =
            Arc::new(move |turn| match step_weak.upgrade() {
                Some(state) => state.daemon_state.run_turn_blocking(turn),
                None => StepOutcome::Failed("the desktop went away mid-run".to_string()),
            });

        let script_host = Arc::new(goble_daemon::ScriptTurnHost::new(
            harness_id.clone(),
            project.clone(),
            medium.clone(),
            format!("workflow-{run_id}"),
            text_seam,
        ));
        let executor = goble_daemon::HarnessStepExecutor::new(
            harness_id,
            project,
            medium,
            format!("workflow-{run_id}"),
            step_seam,
        );
        let host = WorkflowHost::new(executor).with_script_host(script_host);

        let request = WorkflowHostRequest::new(
            0,
            run_id.clone(),
            workflow.id.clone(),
            workflow.trigger.clone(),
            serde_json::json!({}),
        );
        let state = Arc::clone(self);
        let event_id = workflow_id.to_string();
        let execute = move || {
            let result = host.run_workflow(&workflow, request);
            state.persist_run(&run_id, &event_id, result);
            state.emit(
                "workflows:updated",
                serde_json::json!({ "workflow_id": event_id }),
            );
        };
        // A run drives harness turns with a blocking executor, so it belongs on
        // a blocking thread. A caller with no runtime (a test, or a tool that
        // wants the settled run) gets it inline.
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn_blocking(execute);
            }
            Err(_) => execute(),
        }
        Ok(())
    }

    fn persist_run(&self, run_id: &str, workflow_id: &str, result: anyhow::Result<WorkflowRun>) {
        let run = result.unwrap_or_else(|error| WorkflowRun {
            workflow_id: WorkflowId::new(workflow_id),
            status: RunStatus::Failed(format!("{error:#}")),
            steps: Vec::new(),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            output: None,
            log: Vec::new(),
        });
        let status = status_label(&run.status);
        let json = match serde_json::to_string(&run) {
            Ok(json) => json,
            Err(error) => {
                log::error!("workflow run {run_id} could not be serialized: {error}");
                return;
            }
        };
        let finished = run.finished_at.map(|at| at.to_rfc3339());
        if let Err(error) = self.store.lock().insert_workflow_run(
            run_id,
            &run.workflow_id.to_string(),
            status,
            &json,
            &run.started_at.to_rfc3339(),
            finished.as_deref(),
        ) {
            log::error!("workflow run {run_id} could not be persisted: {error}");
        }
    }

    /// Settled runs, newest first, as the UI reads them.
    pub fn list_workflow_runs(&self) -> Vec<WorkflowRunInfo> {
        let rows = match self.store.lock().list_workflow_runs() {
            Ok(rows) => rows,
            Err(error) => {
                log::warn!("workflow runs unavailable: {error}");
                return Vec::new();
            }
        };
        rows.into_iter()
            .filter_map(|(id, workflow_id, status, json, started_at, finished_at)| {
                let run: WorkflowRun = serde_json::from_str(&json).ok()?;
                Some(WorkflowRunInfo {
                    id,
                    workflow_id,
                    status,
                    error: match &run.status {
                        RunStatus::Failed(message) => Some(message.clone()),
                        _ => None,
                    },
                    steps: run
                        .steps
                        .into_iter()
                        .map(|step| WorkflowRunStep {
                            index: step.index,
                            name: step.name,
                            status: step_status_label(&step.status),
                            detail: match step.status {
                                StepStatus::Failed(message) => Some(message),
                                _ => None,
                            },
                            started_at: step.started_at.to_rfc3339(),
                            finished_at: step.finished_at.map(|at| at.to_rfc3339()),
                        })
                        .collect(),
                    started_at,
                    finished_at,
                    output: run.output,
                    log: run.log,
                })
            })
            .collect()
    }
}

fn engine_workflow(spec: &CoreWorkflow) -> anyhow::Result<Workflow> {
    let mut workflow = Workflow::new(WorkflowId::new(spec.id.to_string()), spec.name.clone());
    if let Some(script) = &spec.script {
        workflow = workflow
            .with_script(script.clone())
            .map_err(|error| anyhow::anyhow!("{error}"))?;
    } else {
        workflow = workflow.with_steps(
            spec.steps
                .iter()
                .map(|step| goble_workflow::Step::new(step.id.clone(), step.name.clone()))
                .collect(),
        );
    }
    Ok(workflow.with_trigger(match &spec.trigger {
        CoreTrigger::Manual => goble_workflow::Trigger::Manual,
        CoreTrigger::Cron { expression } => goble_workflow::Trigger::Schedule {
            cron: expression.clone(),
        },
        CoreTrigger::Http { path } => goble_workflow::Trigger::Event {
            event: format!("http:{path}"),
        },
        CoreTrigger::Heartbeat { interval_seconds } => goble_workflow::Trigger::Event {
            event: format!("heartbeat:{interval_seconds}"),
        },
    }))
}

pub(super) fn status_label(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed(_) => "failed",
    }
}

fn step_status_label(status: &StepStatus) -> String {
    match status {
        StepStatus::Skipped => "skipped".to_string(),
        StepStatus::Succeeded => "succeeded".to_string(),
        StepStatus::Failed(_) => "failed".to_string(),
    }
}
