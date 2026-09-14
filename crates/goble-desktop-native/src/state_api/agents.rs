use std::sync::Arc;

use goble_core::agent::{AgentId, Trigger};
use goble_core::thread::ThreadId;
use goble_core::worker::WorkerId;
use goble_desktop_service::{AgentInfo, DesktopState, Intent};

pub fn list_agents(state: &Arc<DesktopState>) -> Vec<AgentInfo> {
    state.list_agents()
}

pub struct CreateAgentRequest {
    pub name: String,
    pub prompt: String,
    pub description: Option<String>,
    pub tools: Vec<String>,
}

pub fn create_agent(
    state: &Arc<DesktopState>,
    req: CreateAgentRequest,
) -> anyhow::Result<AgentInfo> {
    state.create_agent(
        &req.name,
        &req.prompt,
        req.description.as_deref(),
        req.tools,
    )
}

pub struct UpdateAgentRequest {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub description: Option<String>,
    pub tools: Vec<String>,
}

pub fn update_agent(
    state: &Arc<DesktopState>,
    req: UpdateAgentRequest,
) -> anyhow::Result<AgentInfo> {
    state.update_agent(
        &AgentId(req.id),
        &req.name,
        &req.prompt,
        req.description.as_deref(),
        req.tools,
    )
}

pub fn delete_agent(state: &Arc<DesktopState>, agent_id: &str) -> anyhow::Result<()> {
    state.delete_agent(&AgentId(agent_id.to_string()))
}

pub struct RuntimeTarget {
    pub kind: String,
    pub tag: Option<String>,
    pub worker_id: Option<String>,
}

pub struct RunAgentRequest {
    pub target: RuntimeTarget,
    pub agent_id: String,
    pub prompt: String,
}

pub fn run_agent(state: &Arc<DesktopState>, req: RunAgentRequest) -> anyhow::Result<()> {
    let worker_id = state.resolve_worker_for_target(
        &req.target.kind,
        req.target.tag.as_deref(),
        req.target.worker_id.as_deref(),
    )?;
    state.run_agent(&worker_id, &AgentId(req.agent_id), &req.prompt)
}

pub struct ScheduleAgentRequest {
    pub worker_id: String,
    pub agent_id: String,
    pub trigger: String,
}

pub fn schedule_agent(state: &Arc<DesktopState>, req: ScheduleAgentRequest) -> anyhow::Result<()> {
    let trigger = Trigger::Cron {
        expression: req.trigger,
    };
    state.schedule_agent(&WorkerId(req.worker_id), &AgentId(req.agent_id), trigger)
}

pub struct RunAgentForThreadReplyRequest {
    pub target: RuntimeTarget,
    pub thread_id: String,
    pub agent_id: String,
    pub prompt: String,
}

pub fn run_agent_for_thread_reply(
    state: &Arc<DesktopState>,
    req: RunAgentForThreadReplyRequest,
) -> anyhow::Result<()> {
    let worker_id = state.resolve_worker_for_target(
        &req.target.kind,
        req.target.tag.as_deref(),
        req.target.worker_id.as_deref(),
    )?;
    state.run_agent_for_thread_reply(
        &worker_id,
        &ThreadId(req.thread_id),
        &AgentId(req.agent_id),
        &req.prompt,
    )
}

pub fn classify_intent(
    state: &Arc<DesktopState>,
    provider: &str,
    model: &str,
    text: &str,
) -> anyhow::Result<Intent> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|e| anyhow::anyhow!("no tokio runtime: {e}"))?;
    handle.block_on(state.classify_intent(provider, model, text))
}
