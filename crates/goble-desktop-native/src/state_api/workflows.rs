use std::sync::Arc;

use goble_core::agent::Trigger;
use goble_core::workflow::{WorkflowId, WorkflowStep};
use goble_desktop_service::{DesktopState, WorkflowInfo};

pub fn list_workflows(state: &Arc<DesktopState>) -> Vec<WorkflowInfo> {
    state.list_workflows()
}

pub struct CreateWorkflowRequest {
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub trigger: String,
}

pub fn create_workflow(
    state: &Arc<DesktopState>,
    req: CreateWorkflowRequest,
) -> anyhow::Result<WorkflowInfo> {
    let trigger = Trigger::Cron {
        expression: req.trigger,
    };
    state.create_workflow(&req.name, &req.description, req.steps, trigger)
}

pub fn delete_workflow(state: &Arc<DesktopState>, workflow_id: &str) -> anyhow::Result<()> {
    state.delete_workflow(&WorkflowId(workflow_id.to_string()))
}
