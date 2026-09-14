use std::sync::Arc;

use goble_desktop_service::{DesktopState, TeamInfo};

pub fn list_teams(state: &Arc<DesktopState>) -> Vec<TeamInfo> {
    state.list_teams()
}

pub struct CreateTeamRequest {
    pub id: String,
    pub name: String,
    pub metadata: String,
    pub agent_ids: Vec<String>,
}

pub fn create_team(state: &Arc<DesktopState>, req: CreateTeamRequest) -> anyhow::Result<TeamInfo> {
    state.create_team(&req.id, &req.name, &req.metadata, req.agent_ids)
}
