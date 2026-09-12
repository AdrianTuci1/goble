use crate::agent::{AgentId, AgentSpec, Trigger};
use crate::llm::LlmToolCall;
use crate::store::Store;
use crate::subagent::{
    can_spawn_subagent, SubAgentId, SubAgentRecord, SubAgentSpec, SubAgentStatus,
    MAX_SUBAGENT_DEPTH,
};
use anyhow::{Context as _, Result};
use chrono::Utc;

use super::dispatch::ToolTurnContext;
use super::files::resolve_path;
use super::WebSearchConfig;

pub(super) fn create_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = args["name"].as_str().unwrap_or(&id).to_string();
    let description = args["description"].as_str().unwrap_or_default().to_string();
    let prompt = args["prompt"]
        .as_str()
        .context("prompt is required")?
        .to_string();
    let tools = args["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mcp_ids = args["mcp_ids"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let now = Utc::now().to_rfc3339();
    let spec = AgentSpec {
        id: AgentId(id.clone()),
        name,
        description,
        prompt,
        tools,
        triggers: vec![Trigger::Manual],
        mcp_ids,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let spec_json = serde_json::to_string(&spec)?;
    store.insert_agent(&id, &spec.name, &spec_json, &now, &now)?;
    Ok(format!("agent {id} created"))
}

pub(super) fn update_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    let existing = store.get_agent(id)?.context("agent not found")?;
    let mut spec: AgentSpec = serde_json::from_str(&existing.2)?;
    if let Some(name) = args["name"].as_str() {
        spec.name = name.to_string();
    }
    if let Some(description) = args["description"].as_str() {
        spec.description = description.to_string();
    }
    if let Some(prompt) = args["prompt"].as_str() {
        spec.prompt = prompt.to_string();
    }
    if let Some(tools) = args["tools"].as_array() {
        spec.tools = tools
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    if let Some(mcp_ids) = args["mcp_ids"].as_array() {
        spec.mcp_ids = mcp_ids
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    spec.updated_at = Utc::now().to_rfc3339();
    let spec_json = serde_json::to_string(&spec)?;
    store.insert_agent(
        id,
        &spec.name,
        &spec_json,
        &spec.created_at,
        &spec.updated_at,
    )?;
    Ok(format!("agent {id} updated"))
}

pub(super) fn memory_write(store: &Store, args: &serde_json::Value) -> Result<String> {
    let agent_id = args["agent_id"].as_str().context("agent_id is required")?;
    let section = args["section"].as_str().context("section is required")?;
    let content = args["content"].as_str().context("content is required")?;
    let rationale = args["rationale"].as_str().unwrap_or_default();

    let mut memory = match store.get_agent_memory(agent_id)? {
        Some(memory) => memory,
        None => {
            let memory = crate::agent_memory::AgentMemory::new(agent_id.to_string(), "");
            store.put_agent_memory(&memory)?;
            memory
        }
    };

    match section {
        "brief" => memory.update_brief(content),
        "fact" => memory.add_fact(content),
        "goal" => {
            memory.add_goal(content);
        }
        "complete_goal" => {
            if let Some(goal) = memory
                .goals
                .iter_mut()
                .find(|g| memory_text_matches(&g.text, content))
            {
                goal.done = true;
                memory.bump_version();
            } else {
                anyhow::bail!("goal not found: {content}");
            }
        }
        "constraint" => memory.add_constraint(content),
        "decision" => {
            memory.record_decision(content, rationale);
        }
        "milestone" => {
            memory.add_milestone(content);
        }
        "complete_milestone" => {
            if let Some(m) = memory
                .progress
                .iter_mut()
                .find(|m| memory_text_matches(&m.text, content))
            {
                m.done = true;
                memory.bump_version();
            } else {
                anyhow::bail!("milestone not found: {content}");
            }
        }
        "open_question" => memory.add_open_question(content),
        other => anyhow::bail!("unknown memory section: {other}"),
    }

    store.put_agent_memory(&memory)?;
    Ok(format!("memory updated for agent {agent_id}"))
}
pub(super) fn memory_read(store: &Store, args: &serde_json::Value) -> Result<String> {
    let agent_id = args["agent_id"].as_str().context("agent_id is required")?;
    match store.get_agent_memory(agent_id)? {
        Some(memory) => Ok(memory.render_block()),
        None => Ok(format!("no memory for agent {agent_id}")),
    }
}

/// Fuzzy equality used by the memory tools to locate goals/milestones.
fn memory_text_matches(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    a == b || a.contains(&b) || b.contains(&a)
}

pub(super) fn delete_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    store.delete_agent(id)?;
    Ok(format!("agent {id} deleted"))
}
pub(super) fn run_agent(store: &Store, args: &serde_json::Value) -> Result<String> {
    let agent_id = args["agent_id"].as_str().context("agent_id is required")?;
    let input = args["input"].as_str().unwrap_or("");
    let (_, _, spec_json, _, _) = store.get_agent(agent_id)?.context("agent not found")?;
    let spec: AgentSpec = serde_json::from_str(&spec_json)?;
    Ok(format!(
        "ran agent {} with input '{}'\nprompt: {}\ntools: {:?}",
        spec.id, input, spec.prompt, spec.tools
    ))
}

/// Spawn a real child run (S2 + S3): the child gets its own `chats` row keyed by
/// its `SubAgentId` and carrying this conversation's id as `parent_chat_id`, and
/// every message it writes is persisted under its own id, never into the
/// parent's transcript. Its record goes into the harness registry before the
/// child starts, which is what the surfaces and the wire read.
///
/// Foreground (the default): the call awaits the child up to the host's budget
/// and returns its final text, or the reason it ended `Failed`/`Cancelled`. Past
/// that budget the child is not killed - it moves to the background, keeps
/// running and keeps updating its record, and the call returns its id.
/// Background (`run_in_background: true`): the call returns the id at once, with
/// the child's live record registered behind it.
///
/// The child's handles come from the turn's [`SubAgentHost`], not from the
/// borrowed arguments of [`execute_tool_call`]: a child that runs on a task of
/// its own has to own them, and the host is built from the very same store,
/// runner, provider, MCP manager and docs directory the parent's turn is using.
pub(super) async fn spawn_subagent(
    workspace_dir: &std::path::Path,
    call: &LlmToolCall,
    web_search_config: &WebSearchConfig,
    turn: &ToolTurnContext<'_>,
) -> Result<String> {
    let store = &turn.host.store;
    let args = &call.arguments;
    let description = args["description"]
        .as_str()
        .context("description is required")?
        .to_string();
    let prompt = args["prompt"]
        .as_str()
        .context("prompt is required")?
        .to_string();
    let subagent_type = args["subagent_type"]
        .as_str()
        .context("subagent_type is required")?
        .to_string();
    let run_in_background = args["run_in_background"].as_bool().unwrap_or(false);
    if !can_spawn_subagent(turn.depth) {
        anyhow::bail!(
            "sub-agent spawn refused at depth {} (limit {})",
            turn.depth,
            MAX_SUBAGENT_DEPTH
        );
    }
    let id = SubAgentId::generate();
    // The child's cwd is its per-child subdir of the spawner's workspace dir;
    // an explicit `cwd` may relocate it inside the same root, never outside.
    let cwd = match args["cwd"].as_str() {
        Some(path) => resolve_path(path, workspace_dir)?,
        None => id.child_cwd(workspace_dir),
    };
    std::fs::create_dir_all(&cwd).with_context(|| format!("failed to create child cwd {cwd:?}"))?;
    let now = Utc::now().to_rfc3339();
    store.insert_subagent_chat(
        &id.0,
        &description,
        turn.chat_id,
        Some(turn.provider),
        Some(turn.model),
        &now,
        &now,
    )?;
    let spec = SubAgentSpec {
        id: id.clone(),
        parent_chat_id: turn.chat_id.to_string(),
        parent_call_id: call.id.clone(),
        description,
        subagent_type,
        prompt,
        cwd,
        run_in_background,
        depth: turn.depth + 1,
        budget: crate::subagent_run::default_child_budget(),
    };
    let record = SubAgentRecord::new(spec);
    // The record is registered and the loop put on a task before this returns, so
    // a background caller already has a live child for the surfaces to read.
    let done = turn
        .host
        .spawn_child(record, turn.provider, turn.model, web_search_config);
    let label = || {
        turn.host
            .registry
            .snapshot(&id)
            .map(|child| child.status.activity_label())
            .unwrap_or_else(|| "initializing".to_string())
    };
    if run_in_background {
        return Ok(format!(
            "sub-agent {id} running in the background ({}). Its record carries its \
             activity and output; read it instead of waiting.",
            label()
        ));
    }
    let wait = turn.host.foreground_wait();
    let record = match tokio::time::timeout(wait, done).await {
        Ok(Ok(record)) => record,
        // The child outlived the foreground budget: it is not killed, it keeps
        // running on its task and keeps writing its record, and the parent's
        // turn is released with the id to read it by.
        Err(_elapsed) => {
            return Ok(format!(
                "sub-agent {id} is still running after {}s and moved to the background \
                 ({}). Its record carries its activity and output.",
                wait.as_secs(),
                label()
            ))
        }
        // Only an aborted child task ends without sending; its record is the source.
        Ok(Err(_)) => turn
            .host
            .registry
            .snapshot(&id)
            .ok_or_else(|| anyhow::anyhow!("sub-agent {id} ended without a record"))?,
    };
    match record.status {
        SubAgentStatus::Completed { output, .. } => Ok(output),
        SubAgentStatus::Failed { error } => Err(anyhow::anyhow!(error)),
        SubAgentStatus::Cancelled { reason } => Err(anyhow::anyhow!(reason)),
        other => Err(anyhow::anyhow!(
            "sub-agent ended without a terminal status: {other:?}"
        )),
    }
}
