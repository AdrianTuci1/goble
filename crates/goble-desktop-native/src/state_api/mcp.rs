use std::sync::Arc;

use goble_core::mcp_client::McpTool;
use goble_core::mcp_manager::McpServerSummary;
use goble_core::mcp_registry::McpSearchResult;
use goble_desktop_service::DesktopState;

pub fn search_mcp_servers(state: &Arc<DesktopState>, query: &str) -> Vec<McpSearchResult> {
    state.search_mcp_servers(query)
}

pub fn list_mcp_servers(state: &Arc<DesktopState>) -> anyhow::Result<Vec<McpServerSummary>> {
    state.list_mcp_servers()
}

pub struct InstallMcpRequest {
    pub id: String,
    pub name: String,
    pub source: String,
    pub source_value: Option<String>,
    pub secret_ids: Vec<String>,
}

pub fn install_mcp_server(
    state: &Arc<DesktopState>,
    req: InstallMcpRequest,
) -> anyhow::Result<String> {
    state.install_mcp_server(
        &req.id,
        &req.name,
        &req.source,
        req.source_value.as_deref(),
        req.secret_ids,
        None,
    )
}

pub struct UpdateMcpRequest {
    pub id: String,
    pub name: Option<String>,
    pub source_value: Option<String>,
    pub secret_ids: Vec<String>,
}

pub fn update_mcp_server(
    state: &Arc<DesktopState>,
    req: UpdateMcpRequest,
) -> anyhow::Result<String> {
    state.update_mcp_server(
        &req.id,
        req.name.as_deref(),
        req.source_value.as_deref(),
        Some(req.secret_ids),
        None,
    )
}

pub struct UpdateMcpMetaRequest {
    pub id: String,
    pub secret_ids: Vec<String>,
    pub enabled_tools: Vec<String>,
}

pub fn update_mcp_server_meta(
    state: &Arc<DesktopState>,
    req: UpdateMcpMetaRequest,
) -> anyhow::Result<String> {
    state.update_mcp_server_meta(&req.id, req.secret_ids, req.enabled_tools)
}

pub fn delete_mcp_server(state: &Arc<DesktopState>, id: &str) -> anyhow::Result<String> {
    state.delete_mcp_server(id)
}

pub fn discover_mcp_tools(state: &Arc<DesktopState>, id: &str) -> anyhow::Result<Vec<McpTool>> {
    state.discover_mcp_tools(id)
}

pub struct TestCallMcpRequest {
    pub id: String,
    pub tool_name: String,
    pub arguments: Option<serde_json::Value>,
}

pub fn test_call_mcp_tool(
    state: &Arc<DesktopState>,
    req: TestCallMcpRequest,
) -> anyhow::Result<serde_json::Value> {
    state.test_call_mcp_tool(
        &req.id,
        &req.tool_name,
        req.arguments.unwrap_or(serde_json::json!({})),
    )
}
