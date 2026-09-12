use crate::agent::McpManifest;
use crate::mcp_manager::McpManager;
use crate::secret::Secret;
use crate::store::Store;
use anyhow::{Context as _, Result};

pub(super) async fn install_mcp_server(
    mcp_manager: &McpManager,
    store: &Store,
    args: &serde_json::Value,
) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    let name = args["name"].as_str().context("name is required")?;
    let source = args["source"].as_str().context("source is required")?;
    let source_value = args["source_value"].as_str();
    let manifest = args["manifest"]
        .as_str()
        .and_then(|m| serde_json::from_str::<McpManifest>(m).ok());
    let secret_ids: Vec<Secret> =
        serde_json::from_str(&args["secret_ids"].as_str().unwrap_or("[]")).unwrap_or_default();
    mcp_manager
        .install_mcp_server(store, id, name, source, source_value, &secret_ids, manifest)
        .await
}

pub(super) fn list_mcp_servers(mcp_manager: &McpManager, store: &Store) -> Result<String> {
    let rows = mcp_manager.list_mcp_servers(store)?;
    Ok(serde_json::to_string(&rows)?)
}

pub(super) async fn delete_mcp_server(
    mcp_manager: &McpManager,
    store: &Store,
    args: &serde_json::Value,
) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    mcp_manager.delete_mcp_server(store, id)
}

pub(super) async fn update_mcp_server(
    mcp_manager: &McpManager,
    store: &Store,
    args: &serde_json::Value,
) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    let name = args["name"].as_str();
    let source_value = args["source_value"].as_str();
    let manifest = args["manifest"]
        .as_str()
        .and_then(|m| serde_json::from_str::<McpManifest>(m).ok());
    let secret_ids: Vec<Secret> =
        serde_json::from_str(args["secret_ids"].as_str().unwrap_or("[]")).unwrap_or_default();
    mcp_manager
        .update_mcp_server(store, id, name, source_value, Some(&secret_ids), manifest)
        .await
}

pub(super) async fn search_mcp_servers(
    mcp_manager: &McpManager,
    args: &serde_json::Value,
) -> Result<String> {
    let query = args["query"].as_str().context("query is required")?;
    let results = mcp_manager.search_mcp_servers(query).await;
    Ok(serde_json::to_string(&results)?)
}

pub(super) fn discover_mcp_tools(
    mcp_manager: &McpManager,
    args: &serde_json::Value,
) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    let tools = mcp_manager.discover_and_register(id)?;
    Ok(serde_json::to_string(&tools)?)
}
pub(super) fn mcp_call(manager: &McpManager, args: &serde_json::Value) -> Result<String> {
    let server_id = args["server_id"]
        .as_str()
        .context("mcp_call requires server_id")?;
    let tool = args["tool"].as_str().context("mcp_call requires tool")?;
    let arguments = args
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let result = manager.call_tool_on_server(server_id, tool, arguments)?;
    Ok(result.to_string())
}
