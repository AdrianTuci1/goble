/// A scheduled task (cron) shown in the agent's crons drawer.
#[derive(Clone, Debug)]
pub struct CronEntry {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub enabled: bool,
    pub last_run: String,
}

impl CronEntry {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        schedule: impl Into<String>,
        last_run: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            schedule: schedule.into(),
            enabled: true,
            last_run: last_run.into(),
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// The kind of harness a composer entry refers to. In this build the only
/// harness is the native internal one; external harnesses are launched from a
/// terminal rather than registered here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessKind {
    Internal,
}

/// A harness the composer can route a turn to. The composer's harness pill
/// lists these; selecting one sets the active pane's `selected_harness`.
#[derive(Clone, Debug)]
pub struct HarnessEntry {
    pub id: String,
    pub name: String,
    pub kind: HarnessKind,
}

impl HarnessEntry {
    pub fn internal(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: HarnessKind::Internal,
        }
    }
}

/// A vault secret shown in the vault panel.
#[derive(Clone, Debug)]
pub struct VaultSecretEntry {
    pub key: String,
    pub updated_at: String,
}

/// An installed MCP server shown in the connectors panel.
#[derive(Clone, Debug)]
pub struct McpServerEntry {
    pub id: String,
    pub name: String,
    pub source: String,
    pub source_value: Option<String>,
    pub capabilities: Vec<String>,
    pub auth_required: bool,
    pub discovered_tools: Vec<String>,
    pub secret_ids: Vec<String>,
    pub enabled_tools: Vec<String>,
}

/// A registry search result shown in the install drawer.
#[derive(Clone, Debug)]
pub struct McpSearchEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub auth_required: bool,
    pub source_kind: String,
}

/// A workflow shown in the harness workflows page. Real data from the embedded
/// daemon's workflow store ([`goble_desktop_service::DesktopState`]).
#[derive(Clone, Debug)]
pub struct WorkflowEntry {
    pub id: String,
    pub name: String,
    pub trigger: String,
    pub enabled: bool,
    pub created_at: String,
}

/// An execution (agent run) shown in the tasks/executions page. Real data from
/// the daemon execution ledger ([`goble_desktop_service::DesktopState`]).
#[derive(Clone, Debug)]
pub struct ExecutionEntry {
    pub id: String,
    pub agent_id: String,
    pub worker_id: Option<String>,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub step_count: usize,
}

/// A durable task (scheduled or manual unit of work) shown in the
/// tasks/executions page. Real data from the persistence layer.
#[derive(Clone, Debug)]
pub struct TaskEntry {
    pub id: String,
    pub session_id: String,
    pub trigger: String,
    pub status: String,
    pub created_at: String,
}

/// One chronologically ordered record in the timeline page. Derived from real
/// execution / session / task records merged by timestamp.
#[derive(Clone, Debug)]
pub struct TimelineEntry {
    pub at: String,
    pub kind: String,
    pub label: String,
    pub status: Option<String>,
}

/// One cost row in the costs page. There is no cost backend yet, so these are
/// derived from real execution/usage records when a cost metric exists, else an
/// honest aggregate is shown instead of fabricated numbers.
#[derive(Clone, Debug)]
pub struct CostEntry {
    pub id: String,
    pub label: String,
    pub amount: String,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTab {
    Chat,
    Settings,
    Projects,
    Workflows,
    Executions,
    Timeline,
    Costs,
    Mcps,
}

/// Settings overlay categories, mirroring the grok-build settings pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsCategory {
    Appearance,
    Mouse,
    EditorInput,
    AgentApproval,
    Models,
    Advanced,
}

impl SettingsCategory {
    pub const ALL: &'static [SettingsCategory] = &[
        SettingsCategory::Appearance,
        SettingsCategory::Mouse,
        SettingsCategory::EditorInput,
        SettingsCategory::AgentApproval,
        SettingsCategory::Models,
        SettingsCategory::Advanced,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsCategory::Appearance => "Appearance",
            SettingsCategory::Mouse => "Mouse",
            SettingsCategory::EditorInput => "Editor & Input",
            SettingsCategory::AgentApproval => "Agent & Approval",
            SettingsCategory::Models => "Models",
            SettingsCategory::Advanced => "Advanced",
        }
    }
}

/// Where the first-run agent should run its execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceRouting {
    Local,
    Remote,
}

impl WorkspaceRouting {
    /// Map a `"local"` / `"remote"` routing string (see [`crate::media::medium_routing`])
    /// to the corresponding variant, defaulting to `Local`.
    pub fn from_routing(routing: &str) -> Self {
        if routing == "remote" {
            WorkspaceRouting::Remote
        } else {
            WorkspaceRouting::Local
        }
    }
}

/// The text field currently focused inside the model-provider dialog. Tracked
/// in app state so focus survives the per-frame element rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmFormField {
    Model,
    ApiKey,
    BaseUrl,
    Temperature,
}
