use goble_core::agent::{AgentId, AgentSpec, Trigger};
use goble_core::execution::ExecutionTrace;
use goble_core::thread::{Participant, ThreadKind, UserId};
use goble_core::vault::CredentialVault;
use goble_core::worker::{WorkerConfig, WorkerId};
use goble_core::workflow::{Workflow, WorkflowId};

use super::{
    AgentInfo, Chat, DesktopState, ExecutionInfo, TeamInfo, ThreadSummary, WorkerConnection,
    WorkflowInfo,
};

impl DesktopState {
    pub fn load_from_store(&self) -> anyhow::Result<()> {
        let workers = self.store.lock().list_workers()?;
        let mut map = self.workers.lock();
        for (id, name, host, status, _pk, config, _created, _updated) in workers {
            let tags = serde_json::from_str::<WorkerConfig>(&config)
                .map(|c| c.tags)
                .unwrap_or_default();
            map.insert(
                WorkerId(id.clone()),
                WorkerConnection {
                    id,
                    name,
                    url: host.unwrap_or_default(),
                    paired: status == "paired",
                    tags,
                },
            );
        }
        drop(map);

        let agents = self.store.lock().list_agents()?;
        let mut agents_map = self.agents.lock();
        for (id, name, spec_json, created_at, updated_at) in agents {
            if let Ok(spec) = serde_json::from_str::<AgentSpec>(&spec_json) {
                agents_map.insert(
                    AgentId(id.clone()),
                    AgentInfo {
                        id: id.clone(),
                        name,
                        spec,
                        created_at,
                        updated_at,
                    },
                );
            }
        }
        drop(agents_map);

        let workflows = self.store.lock().list_workflows()?;
        let mut wf_map = self.workflows.lock();
        for (id, name, description, spec_json, trigger_json, enabled, created_at, updated_at) in
            workflows
        {
            if let (Ok(wf), Ok(trigger)) = (
                serde_json::from_str::<Workflow>(&spec_json),
                serde_json::from_str::<Trigger>(&trigger_json),
            ) {
                wf_map.insert(
                    WorkflowId(id.clone()),
                    WorkflowInfo {
                        id: id.clone(),
                        name,
                        description,
                        steps: wf.steps,
                        trigger,
                        enabled,
                        created_at,
                        updated_at,
                    },
                );
            }
        }
        drop(wf_map);

        let teams = self.store.lock().list_teams()?;
        let mut team_map = self.teams.lock();
        for (id, name, metadata, created_at) in teams {
            let members = self.store.lock().list_team_members(&id).unwrap_or_default();
            team_map.insert(
                id.clone(),
                TeamInfo {
                    id: id.clone(),
                    name,
                    metadata,
                    created_at,
                    members: members.into_iter().map(|(_, a)| a).collect(),
                },
            );
        }
        drop(team_map);

        let executions = self.store.lock().list_executions()?;
        let mut exec_map = self.executions.lock();
        for (id, agent_id, worker_id, status, trace_json, started_at, finished_at) in executions {
            if let Ok(trace) = serde_json::from_str::<ExecutionTrace>(&trace_json) {
                exec_map.insert(
                    id.clone(),
                    ExecutionInfo {
                        id,
                        agent_id,
                        worker_id,
                        status,
                        trace,
                        started_at,
                        finished_at,
                    },
                );
            }
        }
        drop(exec_map);

        let chats = self.store.lock().list_chats()?;
        let mut chats_vec = self.chats.lock();
        for (id, title, provider, model, _created_at, updated_at) in chats {
            let workspace_routing = self.store.lock().get_chat_workspace_routing(&id)?;
            chats_vec.push(Chat {
                id,
                title,
                provider,
                model,
                agent_id: None,
                worker_id: None,
                workspace_routing,
                updated_at,
            });
        }
        drop(chats_vec);

        if let Some(blob) = self.store.lock().get_setting("vault_blob")? {
            if let Ok(vault) = CredentialVault::from_bytes(blob.as_bytes()) {
                *self.vault.lock() = vault;
            }
        }

        Ok(())
    }

    pub fn migrate_legacy_chats_to_threads(&self) -> Result<Vec<ThreadSummary>, String> {
        let owner = self
            .thread_store()
            .get_profile()
            .map(|p| UserId(p.id.0))
            .unwrap_or_else(UserId::generate);
        let mut summaries = Vec::new();
        for conv in self.list_chats() {
            let participants = vec![Participant::User(owner.clone())];
            let thread = self
                .thread_store()
                .create_thread(
                    ThreadKind::Chat,
                    conv.title,
                    owner.clone(),
                    false,
                    participants,
                    vec![],
                )
                .map_err(|e| e.to_string())?;
            if let Some(messages) = self.messages.lock().get(&conv.id).cloned() {
                for msg in messages {
                    let author = if msg.role == "user" {
                        Participant::User(owner.clone())
                    } else {
                        Participant::Agent(AgentId(
                            conv.agent_id.clone().unwrap_or_else(|| "agent".to_string()),
                        ))
                    };
                    let _ = self.thread_store().post_message(
                        &thread.id,
                        author,
                        msg.content,
                        None,
                        vec![],
                        vec![],
                        None,
                    );
                }
            }
            summaries.push(ThreadSummary::from(thread));
        }
        Ok(summaries)
    }
}
