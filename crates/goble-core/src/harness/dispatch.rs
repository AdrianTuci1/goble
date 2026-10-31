use std::sync::Arc;

use crate::llm::LlmToolCall;
use crate::mcp_manager::McpManager;
use crate::protocol::DesktopMessage;
use crate::store::Store;
use crate::subagent_run::SubAgentHost;
use crate::worker::WorkerId;
use anyhow::Result;

use super::agents::{
    create_agent, delete_agent, memory_read, memory_write, run_agent, spawn_subagent, update_agent,
};
use super::commands::run_command;
use super::credentials::{list_credentials, list_principals};
use super::entities::{list_entities, search_store};
use super::files::{
    codebase_search, delete_file, edit_file, git_commit, git_diff, git_status, read_file,
    rename_file, write_file,
};
use super::guide::user_guide;
use super::mcp::{
    delete_mcp_server, discover_mcp_tools, install_mcp_server, list_mcp_servers, mcp_call,
    search_mcp_servers, update_mcp_server,
};
use super::web::{read_url, web_search};
use super::workflows::{
    create_team, create_workflow, delete_team, delete_workflow, deploy_agent, deploy_workflow,
    get_execution_status, schedule_workflow, update_team, update_workflow,
};
use super::CommandRunner;
use super::{ToolOutcome, WebSearchConfig, SPAWN_SUBAGENT_TOOL};

pub(crate) fn arc_to_sender_ref(
    arc: &Arc<dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync>,
) -> &(dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync) {
    &**arc
}

/// The conversation a tool call executes inside. `spawn_subagent` keys the
/// child's own chats row under `chat_id`, and `depth` is the sub-agent depth
/// of the caller (0 for the main agent): a spawn at the depth limit is
/// refused, and the child's tool set drops the tool entirely (`can_spawn_subagent`).
/// `host` is the harness side of a child run — its registry, its cancel bit and
/// the foreground budget — carried so a spawn registers where the parent's turn
/// can be read and a nested spawn joins the same registry.
#[derive(Clone)]
pub(crate) struct ToolTurnContext<'a> {
    pub chat_id: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub depth: u32,
    pub host: &'a SubAgentHost,
}

pub(crate) async fn execute_tool_call(
    store: &Store,
    runner: &dyn CommandRunner,
    deploy_sender: Option<&(dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync)>,
    mcp_manager: &McpManager,
    workspace_dir: &std::path::Path,
    docs_dir: &std::path::Path,
    call: &LlmToolCall,
    web_search_config: &WebSearchConfig,
    turn: &ToolTurnContext<'_>,
) -> Result<ToolOutcome> {
    match call.name.as_str() {
        "create_agent" => text(create_agent(store, &call.arguments)),
        "update_agent" => text(update_agent(store, &call.arguments)),
        "create_workflow" => text(create_workflow(store, &call.arguments)),
        "update_workflow" => text(update_workflow(store, &call.arguments)),
        "create_team" => text(create_team(store, &call.arguments)),
        "update_team" => text(update_team(store, &call.arguments)),
        "run_command" => text(run_command(store, runner, &call.arguments).await),
        "credentials" => text(list_credentials(store)),
        "principals" => text(list_principals(store)),
        "user_guide" => text(user_guide(&call.arguments, docs_dir)),
        "list_entities" => text(list_entities(store, &call.arguments)),
        "search_store" => text(search_store(store, &call.arguments)),
        "deploy_agent" => text(deploy_agent(store, deploy_sender, &call.arguments)),
        "deploy_workflow" => text(deploy_workflow(store, deploy_sender, &call.arguments)),
        "schedule_workflow" => text(schedule_workflow(store, &call.arguments)),
        "get_execution_status" => text(get_execution_status(store, &call.arguments)),
        "delete_agent" => text(delete_agent(store, &call.arguments)),
        "delete_workflow" => text(delete_workflow(store, &call.arguments)),
        "delete_team" => text(delete_team(store, &call.arguments)),
        "rename_file" => text(rename_file(&call.arguments, workspace_dir)),
        // The three file mutations record the change they made: a write shows
        // what it added, an edit its hunk, a deletion what it took away.
        "delete_file" => delete_file(&call.arguments, workspace_dir),
        "git_status" => text(git_status(runner, &call.arguments).await),
        "git_diff" => text(git_diff(runner, &call.arguments).await),
        "git_commit" => text(git_commit(runner, &call.arguments).await),
        "codebase_search" => text(codebase_search(&call.arguments, workspace_dir)),
        "install_mcp_server" => text(install_mcp_server(mcp_manager, store, &call.arguments).await),
        "list_mcp_servers" => text(list_mcp_servers(mcp_manager, store)),
        "discover_mcp_tools" => text(discover_mcp_tools(mcp_manager, &call.arguments)),
        "delete_mcp_server" => text(delete_mcp_server(mcp_manager, store, &call.arguments).await),
        "update_mcp_server" => text(update_mcp_server(mcp_manager, store, &call.arguments).await),
        "search_mcp_servers" => text(search_mcp_servers(mcp_manager, &call.arguments).await),
        "run_agent" => text(run_agent(store, &call.arguments)),
        SPAWN_SUBAGENT_TOOL => {
            text(spawn_subagent(workspace_dir, call, web_search_config, turn).await)
        }
        "read_file" => text(read_file(&call.arguments, workspace_dir)),
        "write_file" => write_file(&call.arguments, workspace_dir),
        "edit_file" => edit_file(&call.arguments, workspace_dir),
        "web_search" => text(web_search(&call.arguments, web_search_config).await),
        "read_url" => text(read_url(&call.arguments).await),
        "memory_write" => text(memory_write(store, &call.arguments)),
        "memory_read" => text(memory_read(store, &call.arguments)),
        "mcp_call" => text(mcp_call(&mcp_manager, &call.arguments)),
        _ => {
            if mcp_manager.is_mcp_tool(&call.name) {
                mcp_manager
                    .call_tool(&call.name, call.arguments.clone())
                    .map(|v: serde_json::Value| ToolOutcome::text(v.to_string()))
            } else {
                anyhow::bail!("unknown tool {}", call.name)
            }
        }
    }
}

/// A handler's text as an outcome: every tool but a file mutation has nothing
/// to show beyond what it returned.
fn text(output: Result<String>) -> Result<ToolOutcome> {
    output.map(ToolOutcome::text)
}
