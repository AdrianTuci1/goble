use crate::agent::{AgentId, AgentSpec, Trigger};
use crate::protocol::DesktopMessage;
use crate::store::Store;
use crate::worker::WorkerId;
use crate::workflow::{Workflow, WorkflowStep};
use anyhow::{Context as _, Result};
use chrono::Utc;

pub(super) fn create_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let name = args["name"].as_str().context("name is required")?;
    let description = args["description"].as_str().unwrap_or_default();
    let steps = parse_workflow_steps(&args["steps"]);
    let workflow = Workflow::new(name, description).with_steps(steps);
    let spec_json = serde_json::to_string(&workflow)?;
    let trigger_str = serde_json::to_string(&workflow.trigger)?;
    let now = workflow.created_at.clone();
    store.insert_workflow(
        &workflow.id.to_string(),
        &workflow.name,
        &workflow.description,
        &spec_json,
        &trigger_str,
        workflow.enabled,
        &now,
        &now,
    )?;
    Ok(format!("workflow {} created", workflow.id))
}

pub(super) fn update_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    let rows = store.list_workflows()?;
    let existing = rows
        .into_iter()
        .find(|(i, _, _, _, _, _, _, _)| i == id)
        .context("workflow not found")?;
    let mut workflow: Workflow = serde_json::from_str(&existing.3)?;
    if let Some(name) = args["name"].as_str() {
        workflow.name = name.to_string();
    }
    if let Some(description) = args["description"].as_str() {
        workflow.description = description.to_string();
    }
    if args["steps"].is_array() {
        workflow.steps = parse_workflow_steps(&args["steps"]);
    }
    workflow.updated_at = Utc::now().to_rfc3339();
    let spec_json = serde_json::to_string(&workflow)?;
    let trigger_str = serde_json::to_string(&workflow.trigger)?;
    store.insert_workflow(
        &workflow.id.to_string(),
        &workflow.name,
        &workflow.description,
        &spec_json,
        &trigger_str,
        workflow.enabled,
        &workflow.created_at,
        &workflow.updated_at,
    )?;
    Ok(format!("workflow {id} updated"))
}

fn parse_workflow_steps(value: &serde_json::Value) -> Vec<WorkflowStep> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|v| WorkflowStep {
                    id: v["id"]
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    name: v["name"].as_str().unwrap_or("step").to_string(),
                    agent_id: AgentId(v["agent_id"].as_str().unwrap_or_default().to_string()),
                    input_template: v["input_template"].as_str().unwrap_or("").to_string(),
                    depends_on: v["depends_on"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn create_team(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = args["name"].as_str().context("name is required")?;
    let metadata = args["metadata"].as_object().cloned().unwrap_or_default();
    let agent_ids: Vec<String> = args["agent_ids"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let metadata_json = serde_json::to_string(&metadata)?;
    let now = Utc::now().to_rfc3339();
    store.insert_team(&id, name, &metadata_json, &now)?;
    for agent_id in &agent_ids {
        store.insert_team_member(&id, agent_id)?;
    }
    Ok(format!(
        "team {id} created/updated with {} members",
        agent_ids.len()
    ))
}

pub(super) fn update_team(store: &Store, args: &serde_json::Value) -> Result<String> {
    create_team(store, args)
}
pub(super) fn deploy_agent(
    store: &Store,
    deploy_sender: Option<&(dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync)>,
    args: &serde_json::Value,
) -> Result<String> {
    let agent_id = args["agent_id"].as_str().context("agent_id is required")?;
    let worker_id = args["worker_id"]
        .as_str()
        .context("worker_id is required")?;
    let (_, _, spec_json, _, _) = store.get_agent(agent_id)?.context("agent not found")?;
    let spec: AgentSpec = serde_json::from_str(&spec_json)?;
    let worker = WorkerId(worker_id.to_string());
    let message = DesktopMessage::UpdateAgent {
        agent_id: AgentId(agent_id.to_string()),
        spec,
    };
    if let Some(sender) = deploy_sender {
        sender(&worker, message)?;
        Ok(format!("deployed agent {agent_id} to worker {worker_id}"))
    } else {
        Ok(format!(
            "no deploy channel configured; would deploy agent {agent_id} to worker {worker_id}"
        ))
    }
}

pub(super) fn deploy_workflow(
    store: &Store,
    deploy_sender: Option<&(dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync)>,
    args: &serde_json::Value,
) -> Result<String> {
    let workflow_id = args["workflow_id"]
        .as_str()
        .context("workflow_id is required")?;
    let worker_id = args["worker_id"]
        .as_str()
        .context("worker_id is required")?;
    let rows = store.list_workflows()?;
    let existing = rows
        .into_iter()
        .find(|(id, _, _, _, _, _, _, _)| id == workflow_id)
        .context("workflow not found")?;
    let workflow: Workflow = serde_json::from_str(&existing.3)?;
    let worker = WorkerId(worker_id.to_string());
    let message = DesktopMessage::RunAgent {
        trace_id: uuid::Uuid::new_v4().to_string(),
        agent_id: AgentId(workflow.id.to_string()),
        spec: AgentSpec {
            id: AgentId(workflow.id.to_string()),
            name: workflow.name.clone(),
            description: workflow.description.clone(),
            prompt: serde_json::to_string(&workflow.steps)?,
            tools: Vec::new(),
            triggers: vec![workflow.trigger.clone()],
            mcp_ids: Vec::new(),
            created_at: workflow.created_at.clone(),
            updated_at: workflow.updated_at.clone(),
        },
        mcp_servers: vec![],
    };
    if let Some(sender) = deploy_sender {
        sender(&worker, message)?;
        Ok(format!(
            "deployed workflow {workflow_id} to worker {worker_id}"
        ))
    } else {
        Ok(format!("no deploy channel configured; would deploy workflow {workflow_id} to worker {worker_id}"))
    }
}

pub(super) fn schedule_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let workflow_id = args["workflow_id"]
        .as_str()
        .context("workflow_id is required")?;
    let trigger_type = args["trigger_type"]
        .as_str()
        .context("trigger_type is required")?;
    let trigger_value = args["trigger_value"]
        .as_str()
        .context("trigger_value is required")?;
    let rows = store.list_workflows()?;
    let existing = rows
        .into_iter()
        .find(|(id, _, _, _, _, _, _, _)| id == workflow_id)
        .context("workflow not found")?;
    let mut workflow: Workflow = serde_json::from_str(&existing.3)?;
    workflow.trigger = match trigger_type {
        "cron" => Trigger::Cron {
            expression: trigger_value.to_string(),
        },
        "http" => Trigger::Http {
            path: trigger_value.to_string(),
        },
        "heartbeat" => {
            let interval = trigger_value
                .parse::<u64>()
                .context("heartbeat interval must be a number")?;
            Trigger::Heartbeat {
                interval_seconds: interval,
            }
        }
        _ => anyhow::bail!("unknown trigger type {trigger_type}"),
    };
    workflow.updated_at = Utc::now().to_rfc3339();
    let spec_json = serde_json::to_string(&workflow)?;
    let trigger_str = serde_json::to_string(&workflow.trigger)?;
    store.insert_workflow(
        &workflow.id.to_string(),
        &workflow.name,
        &workflow.description,
        &spec_json,
        &trigger_str,
        workflow.enabled,
        &workflow.created_at,
        &workflow.updated_at,
    )?;
    Ok(format!(
        "workflow {workflow_id} scheduled with {trigger_type}={trigger_value}"
    ))
}

pub(super) fn get_execution_status(store: &Store, args: &serde_json::Value) -> Result<String> {
    let execution_id = args["execution_id"]
        .as_str()
        .context("execution_id is required")?;
    let rows = store.list_executions()?;
    let exec = rows
        .into_iter()
        .find(|(id, _, _, _, _, _, _)| id == execution_id)
        .context("execution not found")?;
    Ok(format!(
        "execution {execution_id}: status={}, agent={}, worker={}, started_at={}",
        exec.3,
        exec.1.as_deref().unwrap_or("-"),
        exec.2.as_deref().unwrap_or("-"),
        exec.5
    ))
}
pub(super) fn delete_workflow(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    store.delete_workflow(id)?;
    Ok(format!("workflow {id} deleted"))
}

pub(super) fn delete_team(store: &Store, args: &serde_json::Value) -> Result<String> {
    let id = args["id"].as_str().context("id is required")?;
    store.delete_team(id)?;

    Ok(format!("team {id} deleted"))
}
