use std::sync::Arc;

use goble_core::execution::ExecutionTrace;
use goble_desktop_service::{DesktopState, ExecutionInfo};

pub fn list_executions(state: &Arc<DesktopState>) -> Vec<ExecutionInfo> {
    state.list_executions()
}

pub fn get_execution_trace(
    state: &Arc<DesktopState>,
    trace_id: &str,
) -> anyhow::Result<ExecutionTrace> {
    state
        .get_execution_trace(trace_id)
        .ok_or_else(|| anyhow::anyhow!("trace not found"))
}
