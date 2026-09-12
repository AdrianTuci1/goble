use super::*;

/// The string form of a workspace routing choice as persisted on a chat.
pub(crate) fn routing_to_str(routing: WorkspaceRouting) -> &'static str {
    match routing {
        WorkspaceRouting::Local => "local",
        WorkspaceRouting::Remote => "remote",
    }
}

/// Parse a persisted workspace-routing string back into a [`WorkspaceRouting`].
pub(crate) fn routing_from_str(value: &str) -> Option<WorkspaceRouting> {
    match value {
        "local" => Some(WorkspaceRouting::Local),
        "remote" => Some(WorkspaceRouting::Remote),
        _ => None,
    }
}
