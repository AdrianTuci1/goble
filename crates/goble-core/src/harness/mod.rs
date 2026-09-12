//! The agent harness: one turn's dispatch, and every surface a tool reaches.
//!
//! One module per surface: the command runners the tools execute through
//! ([`runner`]), the values a turn exchanges ([`types`]), the built-in tool
//! definitions and system prompt ([`definitions`]), how an invocation presents
//! in the transcript ([`presentation`]) and the routing from a tool call to its
//! handler ([`dispatch`]). The handlers themselves are one module per tool
//! family: [`agents`], [`workflows`], [`credentials`], [`guide`], [`entities`],
//! [`files`], [`commands`], [`handoff`], [`mcp`] and [`web`]. What the surfaces
//! share — [`Harness`] itself, its builder, its turn loop and its sub-agent
//! accessors — lives here.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::llm::{CompletionResponse, LlmProvider};
use crate::mcp_manager::McpManager;
use crate::protocol::DesktopMessage;
use crate::store::Store;
use crate::subagent::{SubAgentId, SubAgentRecord};
use crate::subagent_run::{
    SubAgentHost, SubAgentRegistry, DEFAULT_FOREGROUND_CHILD_WAIT, PARENT_CANCEL_REASON,
};
use crate::worker::WorkerId;
use anyhow::Result;
use futures::Stream;

mod agents;
mod commands;
mod credentials;
mod definitions;
mod dispatch;
mod entities;
mod files;
mod guide;
mod handoff;
mod mcp;
mod presentation;
mod runner;
mod types;
mod web;
mod workflows;

#[cfg(test)]
mod tests;

pub use presentation::{
    edit_change_counts, edit_diffstat, one_line, skill_name, string_argument, subagent_subject,
    tool_kind_for, tool_row, web_search_sources, ToolKind, ToolRow,
};
pub use runner::{
    harness_sandbox, harness_sandbox_profile, CommandRunner, MockCommandRunner, PaneSession,
    RoutedCommandRunner, SandboxedCommandRunner,
};
pub use types::{
    ChatToolCall, CommandDecision, HarnessEvent, ThinkingMode, ToolCallStatus, ToolSchema,
    WebSearchConfig,
};

pub(crate) use commands::{command_candidates, run_approved_command, COMMAND_TOOL};
pub(crate) use definitions::{
    harness_tool_definitions, HARNESS_SYSTEM_PROMPT, SPAWN_SUBAGENT_TOOL,
};
pub(crate) use dispatch::{arc_to_sender_ref, execute_tool_call, ToolTurnContext};

pub struct Harness {
    store: Store,
    runner: Arc<dyn CommandRunner>,
    llm: Arc<dyn LlmProvider>,
    deploy_sender: Option<Arc<dyn Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync>>,
    mcp_manager: McpManager,
    cancel: Arc<AtomicBool>,
    workspace_dir: PathBuf,
    docs_dir: PathBuf,
    reasoning_enabled: bool,
    auto_approve: bool,
    web_search: WebSearchConfig,
    /// The sub-agent children this harness has spawned (S3). Shared with every
    /// turn and every child, so the records outlive the turn that spawned them.
    subagents: SubAgentRegistry,
}

impl Harness {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            runner: Arc::new(MockCommandRunner),
            llm: Arc::new(crate::llm::MockProvider::new(
                "mock",
                CompletionResponse::new("I can help with that.", Vec::new()),
            )),
            deploy_sender: None,
            mcp_manager: McpManager::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            workspace_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            docs_dir: crate::app_home::GobleHome::locate()
                .map(|h| h.docs_user_guide_dir())
                .unwrap_or_else(|_| PathBuf::from(".goble/docs/user-guide")),
            reasoning_enabled: false,
            auto_approve: false,
            web_search: WebSearchConfig::default(),
            subagents: SubAgentRegistry::default(),
        }
    }

    pub fn with_reasoning(mut self, enabled: bool) -> Self {
        self.reasoning_enabled = enabled;
        self
    }

    /// When set, the agent does not suspend to ask the user a question: an
    /// `ask_user` tool call is auto-skipped and the mission continues.
    pub fn with_auto_approve(mut self, auto_approve: bool) -> Self {
        self.auto_approve = auto_approve;
        self
    }

    pub fn with_runner(mut self, runner: Arc<dyn CommandRunner>) -> Self {
        self.runner = runner;
        self
    }

    pub fn with_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = llm;
        self
    }

    pub fn with_cancel(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }

    pub fn with_workspace_dir(mut self, workspace_dir: impl Into<std::path::PathBuf>) -> Self {
        self.workspace_dir = workspace_dir.into();
        self
    }

    /// The configured workspace directory.
    pub fn workspace_dir(&self) -> &std::path::Path {
        &self.workspace_dir
    }

    /// Directory holding the seeded user guide (`~/.goble/docs/user-guide`), used
    /// by the `user_guide` tool. Defaults to the workspace home's docs dir.
    pub fn with_docs_dir(mut self, docs_dir: impl Into<std::path::PathBuf>) -> Self {
        self.docs_dir = docs_dir.into();
        self
    }

    pub fn with_deploy_sender<F>(mut self, sender: F) -> Self
    where
        F: Fn(&WorkerId, DesktopMessage) -> Result<()> + Send + Sync + 'static,
    {
        self.deploy_sender = Some(Arc::new(sender));
        self
    }

    pub fn with_mcp_manager(mut self, manager: McpManager) -> Self {
        self.mcp_manager = manager;
        self
    }

    /// Configure the web-search backend. When `api_key` + `base_url` are set the
    /// hosted backend is used; otherwise `web_search` falls back to DuckDuckGo.
    pub fn with_web_search(mut self, config: WebSearchConfig) -> Self {
        self.web_search = config;
        self
    }

    /// Stop the turn. The bit is the one every child reads too, so a parent stop
    /// reaches the whole tree; marking the records here is what lets the
    /// overlay show them stopped without waiting for each task's next check.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.subagents.cancel_all(PARENT_CANCEL_REASON);
    }

    /// The children this harness has spawned, in spawn order, as their records
    /// stand right now. The transcript row (S5), the child view (S6) and the
    /// work overlay (S7) read this; so does a kill by id.
    pub fn subagent_records(&self) -> Vec<SubAgentRecord> {
        self.subagents.records()
    }

    /// One child's record, by its id. `None` when this harness never spawned it.
    pub fn subagent_record(&self, id: &SubAgentId) -> Option<SubAgentRecord> {
        self.subagents.snapshot(id)
    }

    /// Kill one child, leaving its siblings running (the overlay's per-child
    /// stop). The child's record goes `Cancelled` with `reason` at once and its
    /// loop stops at its next check. `false` when the id is unknown or the
    /// child already reached a terminal status.
    pub fn cancel_subagent(&self, id: &SubAgentId, reason: &str) -> bool {
        self.subagents.cancel(id, reason)
    }

    /// The handles a child run gets, from the harness's current config. Built
    /// per turn and threaded through tool execution, so a child never rebuilds
    /// a client, a runner or a store of its own, and so a child of a child
    /// registers in the same registry the harness exposes.
    pub(crate) fn subagent_host(&self) -> SubAgentHost {
        SubAgentHost {
            store: self.store.clone(),
            runner: Arc::clone(&self.runner),
            llm: Arc::clone(&self.llm),
            deploy_sender: self.deploy_sender.clone(),
            mcp_manager: self.mcp_manager.clone(),
            docs_dir: self.docs_dir.clone(),
            registry: self.subagents.clone(),
            parent_cancel: Arc::clone(&self.cancel),
            foreground_wait: DEFAULT_FOREGROUND_CHILD_WAIT,
        }
    }

    pub fn list_tools(&self) -> Vec<ToolSchema> {
        let mut tools = harness_tool_definitions();
        if let Ok(mcp_tools) = self.mcp_manager.refresh_from_store(&self.store) {
            tools.extend(mcp_tools);
        }
        tools.push(crate::mcp_manager::McpManager::generic_mcp_call_tool());
        tools
            .into_iter()
            .map(|t| ToolSchema {
                name: t.name,
                description: t.description,
                parameters: t.parameters,
            })
            .collect()
    }

    /// Run one conversational turn. Emits streaming events, including the
    /// sub-agent lifecycle the registry emits for children this turn runs.
    pub fn run_turn(
        &self,
        chat_id: &str,
        prompt: &str,
        provider: &str,
        model: &str,
    ) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
        crate::subagent_run::with_subagent_lifecycle(
            &self.subagents,
            crate::reasoning::run_mission_turn(
                self.store.clone(),
                Arc::clone(&self.runner),
                Arc::clone(&self.llm),
                self.deploy_sender.clone(),
                self.mcp_manager.clone(),
                self.cancel.clone(),
                self.workspace_dir.clone(),
                self.docs_dir.clone(),
                chat_id.to_string(),
                prompt.to_string(),
                provider.to_string(),
                model.to_string(),
                self.reasoning_enabled,
                self.auto_approve,
                self.web_search.clone(),
                self.subagent_host(),
            ),
        )
    }

    /// Resume a turn that was suspended waiting for a user answer.
    pub fn resume_turn(
        &self,
        chat_id: &str,
        response: &str,
        credential: Option<(String, String)>,
        provider: &str,
        model: &str,
    ) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
        crate::subagent_run::with_subagent_lifecycle(
            &self.subagents,
            crate::reasoning::resume_mission_turn(
                self.store.clone(),
                Arc::clone(&self.runner),
                Arc::clone(&self.llm),
                self.deploy_sender.clone(),
                self.mcp_manager.clone(),
                self.cancel.clone(),
                self.workspace_dir.clone(),
                self.docs_dir.clone(),
                chat_id.to_string(),
                response.to_string(),
                credential,
                provider.to_string(),
                model.to_string(),
                self.auto_approve,
                self.web_search.clone(),
                self.subagent_host(),
            ),
        )
    }

    /// Resume a turn that suspended on a proposed command, executing the user's
    /// decision (A6): approve runs the chosen text, edit runs the edited text,
    /// and reject fails the tool call instead of running anything.
    pub fn resume_command(
        &self,
        chat_id: &str,
        decision: CommandDecision,
    ) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
        crate::subagent_run::with_subagent_lifecycle(
            &self.subagents,
            crate::reasoning::resume_command_turn(
                self.store.clone(),
                Arc::clone(&self.runner),
                chat_id.to_string(),
                decision,
            ),
        )
    }
}
