use anyhow::Result;
use chrono::Utc;

use crate::harness::{ChatToolCall, ThinkingMode};
use crate::store::Store;

use super::types::{MissionState, PendingAsk, ReasoningDecision, ReasoningStep};

pub(super) fn load_or_create_mission(
    store: &Store,
    chat_id: &str,
    goal: &str,
) -> Result<MissionState> {
    let missions = store.list_missions()?;
    if let Some(mission) = missions
        .into_iter()
        .find(|m| m.1 == chat_id && m.3 != "done")
    {
        let reasoning = store
            .list_reasoning_steps(&mission.0)?
            .into_iter()
            .map(|row| ReasoningStep {
                step: row.1 as usize,
                mode: row.2.parse().unwrap_or(ThinkingMode::Direct),
                content: row.3,
                decision: row
                    .4
                    .as_deref()
                    .and_then(|d| serde_json::from_str(d).ok())
                    .unwrap_or(ReasoningDecision::Continue),
                tool_calls: row
                    .5
                    .as_deref()
                    .map(|t| serde_json::from_str(t).unwrap_or_default())
                    .unwrap_or_default(),
            })
            .collect();
        let pending = store.get_pending_ask(chat_id)?;
        return Ok(MissionState {
            id: mission.0,
            chat_id: mission.1,
            goal: mission.2,
            status: mission.3,
            plan: mission.4,
            workflow_id: mission.5,
            reasoning_steps: reasoning,
            pending_ask: pending.map(|p| PendingAsk {
                id: p.0,
                question: p.3,
                quick_replies: p.4.split('\n').map(|s| s.to_string()).collect(),
            }),
        });
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    store.insert_mission(&id, chat_id, goal, "clarifying", None, None, &now, &now)?;
    Ok(MissionState {
        id,
        chat_id: chat_id.to_string(),
        goal: goal.to_string(),
        status: "clarifying".to_string(),
        plan: None,
        workflow_id: None,
        reasoning_steps: Vec::new(),
        pending_ask: None,
    })
}

pub(super) fn persist_mission(store: &Store, mission: &MissionState) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    store.insert_mission(
        &mission.id,
        &mission.chat_id,
        &mission.goal,
        &mission.status,
        mission.plan.as_deref(),
        mission.workflow_id.as_deref(),
        &now,
        &now,
    )?;
    Ok(())
}

pub(super) fn persist_reasoning_step(
    store: &Store,
    mission_id: &str,
    step: &ReasoningStep,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    store.insert_reasoning_step(
        &uuid::Uuid::new_v4().to_string(),
        mission_id,
        step.step as i32,
        step.mode.as_str(),
        &step.content,
        Some(&serde_json::to_string(&step.decision)?),
        Some(&serde_json::to_string(&step.tool_calls)?),
        &now,
    )?;
    Ok(())
}

/// Rewrite the assistant row's `tool_calls` column with the current records, so
/// a call's status/result is readable from the store while the turn still runs.
pub(super) fn persist_tool_call_records(
    store: &Store,
    message_id: &str,
    records: &[ChatToolCall],
) -> Result<()> {
    let json = serde_json::to_string(records)?;
    store.set_chat_message_tool_calls(message_id, &json)
}
