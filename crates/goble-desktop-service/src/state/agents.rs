use chrono::Utc;
use goble_core::agent::{AgentId, AgentSpec, Trigger};
use goble_core::execution::ExecutionTrace;
use goble_core::protocol::DesktopMessage;
use goble_core::thread::ThreadId;
use goble_core::worker::WorkerId;
use goble_core::workflow::{Workflow, WorkflowId, WorkflowStep};
use goble_persistence::{Project, Session, Task};

use super::{AgentInfo, DesktopState, ExecutionInfo, TeamInfo, WorkflowInfo};

impl DesktopState {
    pub fn create_agent(
        &self,
        name: &str,
        prompt: &str,
        description: Option<&str>,
        tools: Vec<String>,
    ) -> anyhow::Result<AgentInfo> {
        let mut spec = AgentSpec::new(name, prompt);
        if let Some(d) = description {
            spec = spec.with_description(d);
        }
        spec = spec.with_tools(tools);
        let id = spec.id.clone();
        let now = Utc::now().to_rfc3339();
        let spec_json = serde_json::to_string(&spec)?;
        self.store
            .lock()
            .insert_agent(&id.to_string(), name, &spec_json, &now, &now)?;
        let info = AgentInfo {
            id: id.to_string(),
            name: name.to_string(),
            spec,
            created_at: now.clone(),
            updated_at: now,
        };
        self.agents.lock().insert(id, info.clone());
        self.emit("agents:updated", ());
        Ok(info)
    }

    pub fn delete_agent(&self, id: &AgentId) -> anyhow::Result<()> {
        self.store.lock().delete_agent(&id.to_string())?;
        self.agents.lock().remove(id);
        self.emit("agents:updated", ());
        Ok(())
    }

    pub fn update_agent(
        &self,
        id: &AgentId,
        name: &str,
        prompt: &str,
        description: Option<&str>,
        tools: Vec<String>,
    ) -> anyhow::Result<AgentInfo> {
        let mut spec = AgentSpec::new(name, prompt);
        if let Some(d) = description {
            spec = spec.with_description(d);
        }
        spec = spec.with_tools(tools);
        spec.id = id.clone();
        let now = Utc::now().to_rfc3339();
        let spec_json = serde_json::to_string(&spec)?;
        self.store
            .lock()
            .update_agent(&id.to_string(), name, &spec_json, &now)?;
        let info = AgentInfo {
            id: id.to_string(),
            name: name.to_string(),
            spec,
            created_at: self
                .agents
                .lock()
                .get(id)
                .map(|a| a.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        self.agents.lock().insert(id.clone(), info.clone());
        self.emit("agents:updated", ());
        Ok(info)
    }

    pub fn list_agents(&self) -> Vec<AgentInfo> {
        self.agents.lock().values().cloned().collect()
    }

    pub fn list_executions(&self) -> Vec<ExecutionInfo> {
        self.executions.lock().values().cloned().collect()
    }

    pub fn get_execution_trace(&self, trace_id: &str) -> Option<ExecutionTrace> {
        self.executions
            .lock()
            .get(trace_id)
            .map(|e| e.trace.clone())
    }

    pub fn create_workflow(
        &self,
        name: &str,
        description: &str,
        steps: Vec<WorkflowStep>,
        trigger: Trigger,
    ) -> anyhow::Result<WorkflowInfo> {
        let mut wf = Workflow::new(name, description).with_trigger(trigger);
        for step in steps {
            wf = wf.with_step(step);
        }
        let id = wf.id.clone();
        let now = Utc::now().to_rfc3339();
        let spec_json = serde_json::to_string(&wf)?;
        let trigger_json = serde_json::to_string(&wf.trigger)?;
        self.store.lock().insert_workflow(
            &id.to_string(),
            name,
            description,
            &spec_json,
            &trigger_json,
            wf.enabled,
            &now,
            &now,
        )?;
        let info = WorkflowInfo {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            steps: wf.steps,
            trigger: wf.trigger,
            enabled: wf.enabled,
            created_at: now.clone(),
            updated_at: now,
        };
        self.workflows.lock().insert(id, info.clone());
        self.emit("workflows:updated", ());
        Ok(info)
    }

    pub fn delete_workflow(&self, id: &WorkflowId) -> anyhow::Result<()> {
        self.store.lock().delete_workflow(&id.to_string())?;
        self.workflows.lock().remove(id);
        self.emit("workflows:updated", ());
        Ok(())
    }

    pub fn list_workflows(&self) -> Vec<WorkflowInfo> {
        self.workflows.lock().values().cloned().collect()
    }

    pub fn create_team(
        &self,
        id: &str,
        name: &str,
        metadata: &str,
        agent_ids: Vec<String>,
    ) -> anyhow::Result<TeamInfo> {
        let now = Utc::now().to_rfc3339();
        self.store.lock().insert_team(id, name, metadata, &now)?;
        for agent_id in &agent_ids {
            self.store.lock().insert_team_member(id, agent_id)?;
        }
        let info = TeamInfo {
            id: id.to_string(),
            name: name.to_string(),
            metadata: metadata.to_string(),
            created_at: now,
            members: agent_ids,
        };
        self.teams.lock().insert(id.to_string(), info.clone());
        self.emit("teams:updated", ());
        Ok(info)
    }

    pub fn list_teams(&self) -> Vec<TeamInfo> {
        self.teams.lock().values().cloned().collect()
    }

    /// The durable projects seeded from the persistence layer on startup.
    pub fn list_projects(&self) -> Vec<Project> {
        self.projects.lock().values().cloned().collect()
    }

    /// The durable sessions seeded from the persistence layer on startup.
    pub fn list_sessions(&self) -> Vec<Session> {
        self.sessions.lock().values().cloned().collect()
    }

    /// The durable tasks seeded from the persistence layer on startup.
    pub fn list_tasks(&self) -> Vec<Task> {
        self.tasks.lock().values().cloned().collect()
    }

    pub fn run_agent(
        &self,
        worker_id: &WorkerId,
        agent_id: &AgentId,
        prompt: &str,
    ) -> anyhow::Result<()> {
        let spec = if let Some(agent) = self.agents.lock().get(agent_id) {
            agent.spec.clone()
        } else {
            AgentSpec::new(&agent_id.0, prompt)
        };
        let mcp_servers = self.resolve_mcp_servers_for_agent(&spec)?;
        self.send_to_worker(
            worker_id,
            DesktopMessage::RunAgent {
                trace_id: format!("desktop-{}", uuid::Uuid::new_v4()),
                agent_id: agent_id.clone(),
                spec,
                mcp_servers,
            },
        )
    }

    pub fn schedule_agent(
        &self,
        worker_id: &WorkerId,
        agent_id: &AgentId,
        trigger: Trigger,
    ) -> anyhow::Result<()> {
        let spec = self.agents.lock().get(agent_id).cloned();
        let mcp_servers = if let Some(ref s) = spec {
            self.resolve_mcp_servers_for_agent(&s.spec)?
        } else {
            vec![]
        };
        self.send_to_worker(
            worker_id,
            DesktopMessage::ScheduleAgent {
                agent_id: agent_id.clone(),
                trigger,
                mcp_servers,
            },
        )
    }

    /// Convert legacy chat conversations into threads with a single participant (the user).
    pub fn run_agent_for_thread_reply(
        &self,
        worker_id: &WorkerId,
        thread_id: &ThreadId,
        agent_id: &AgentId,
        prompt: &str,
    ) -> anyhow::Result<()> {
        let (id, name, spec, created_at, updated_at) = self
            .store
            .lock()
            .get_agent(&agent_id.0)?
            .ok_or_else(|| anyhow::anyhow!("agent not found: {}", agent_id.0))?;
        let agent_spec = AgentSpec {
            id: AgentId(id),
            name: name.clone(),
            description: name.clone(),
            prompt: spec,
            tools: vec![],
            triggers: vec![],
            mcp_ids: vec![],
            created_at,
            updated_at,
        };
        let mcp_servers = self.resolve_mcp_servers_for_agent(&agent_spec)?;
        let msg = DesktopMessage::RunAgentForThreadReply {
            trace_id: format!("{}-{}", thread_id.0, uuid::Uuid::new_v4()),
            thread_id: thread_id.0.clone(),
            agent_id: agent_id.clone(),
            prompt: prompt.to_string(),
            spec: agent_spec,
            mcp_servers,
        };
        self.send_to_worker(worker_id, msg)
    }
}
