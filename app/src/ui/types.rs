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
    /// Groups of secrets a remote session is meant to start with. It sits
    /// directly after Agent & Approval because that page is where the user
    /// picks Local or Remote: the secrets belong next to the choice that makes
    /// them matter, before the model and machine-config pages.
    Environment,
    Models,
    Advanced,
}

impl SettingsCategory {
    pub const ALL: &'static [SettingsCategory] = &[
        SettingsCategory::Appearance,
        SettingsCategory::Mouse,
        SettingsCategory::EditorInput,
        SettingsCategory::AgentApproval,
        SettingsCategory::Environment,
        SettingsCategory::Models,
        SettingsCategory::Advanced,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsCategory::Appearance => "Appearance",
            SettingsCategory::Mouse => "Mouse",
            SettingsCategory::EditorInput => "Editor & Input",
            SettingsCategory::AgentApproval => "Agent & Approval",
            SettingsCategory::Environment => "Environment",
            SettingsCategory::Models => "Models",
            SettingsCategory::Advanced => "Advanced",
        }
    }
}

/// Which of the settings overlay's two regions holds the keyboard: the
/// category rail on the left, or the content pane beside it. Opening the
/// overlay starts in the rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsFocus {
    #[default]
    Rail,
    Pane,
}

/// One focusable control inside a settings pane, in the pane's focus order.
///
/// The pane's order is produced by
/// [`crate::ui::settings::pane_controls`], which the pane itself walks while
/// drawing its rows, so the order and the drawn rows cannot drift apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsControl {
    /// Appearance: the dark-mode switch.
    DarkMode,
    /// Appearance: one channel of the colour-target column.
    ThemeChannel(super::color_picker::ColorTarget),
    /// Appearance: the colour wheel; it is a drag surface, so it takes a focus
    /// stop and the ring but no arrow adjustment.
    ColorWheel,
    /// Mouse: invert the scroll direction.
    InvertScroll,
    /// Mouse: the scroll-speed stepper.
    ScrollSpeed,
    /// Editor & Input: the font-size stepper.
    FontSize,
    /// Agent & Approval: the auto-approve switch of the active pane.
    AutoApprove,
    /// Agent & Approval: the vim-mode switch.
    VimMode,
    /// Agent & Approval: where the agent runs.
    Route(WorkspaceRouting),
    /// Models: reload the list from `~/.goble/config.toml`.
    ReloadModels,
    /// Environment: the new-group name field.
    EnvironmentGroupName,
    /// Environment: create the group the name field holds.
    EnvironmentCreateGroup,
    /// Environment: one group in the list; Enter opens it.
    EnvironmentGroup(String),
    /// Environment: delete a group and its secrets.
    EnvironmentDeleteGroup(String),
    /// Environment: the secret-name field.
    EnvironmentSecretName,
    /// Environment: the secret-value field.
    EnvironmentSecretValue,
    /// Environment: add the secret the fields hold, or save the edited one.
    EnvironmentSaveSecret,
    /// Environment: one secret of the open group; Enter loads it for editing.
    EnvironmentSecret(String),
    /// Environment: remove one secret.
    EnvironmentDeleteSecret(String),
}

impl SettingsControl {
    /// Whether the control holds a discrete value, so Left/Right change it
    /// (a stepper, the theme-channel column, the Local/Remote choice). A
    /// control that does not hands the keyboard back to the rail on
    /// `ArrowLeft` instead. The colour wheel is deliberately not one of these:
    /// it is a drag surface, and there is no keyboard value to step.
    pub fn has_discrete_values(&self) -> bool {
        matches!(
            self,
            SettingsControl::ScrollSpeed
                | SettingsControl::FontSize
                | SettingsControl::ThemeChannel(_)
                | SettingsControl::Route(_)
        )
    }

    /// Whether the control is a text field, so Enter/Space puts the caret in it
    /// and every key the overlay does not reserve reaches it.
    pub fn is_text_field(&self) -> bool {
        matches!(
            self,
            SettingsControl::EnvironmentGroupName
                | SettingsControl::EnvironmentSecretName
                | SettingsControl::EnvironmentSecretValue
        )
    }
}

/// Which of the sidebar's views is showing. The conversations list is the
/// default; the other two are windows onto the active pane's working directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarView {
    /// The agent conversations: search, Starred, the folders and their cards.
    #[default]
    Agents,
    /// The project explorer: the working directory as a real file tree.
    Explorer,
    /// Global search: the query, and what it matches in the files under the
    /// working directory.
    Search,
}

impl SidebarView {
    /// The three views, in the order the sidebar's tab strip draws them.
    pub const ALL: &'static [SidebarView] = &[
        SidebarView::Agents,
        SidebarView::Explorer,
        SidebarView::Search,
    ];

    /// The tab's label.
    pub fn label(self) -> &'static str {
        match self {
            SidebarView::Agents => "Agents",
            SidebarView::Explorer => "Explorer",
            SidebarView::Search => "Search",
        }
    }

    /// What the tab does, spelled out where the pointer rests on it.
    pub fn tooltip(self) -> &'static str {
        match self {
            SidebarView::Agents => "Agent conversations",
            SidebarView::Explorer => "Project explorer",
            SidebarView::Search => "Global search",
        }
    }
}

/// One row of the project explorer, already flattened: the entry, how deep it
/// sits under the root, and whether a directory is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerRow {
    /// Absolute path of the entry.
    pub path: String,
    /// The name drawn on the row.
    pub name: String,
    /// Nesting depth: 0 is a direct child of the root.
    pub depth: usize,
    pub is_dir: bool,
    /// Whether an open directory's children are drawn under it.
    pub expanded: bool,
}

/// One row of the global search: a file header, or one matched line inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchRow {
    /// Absolute path of the file the row belongs to.
    pub path: String,
    /// The name on a file header row; empty on a line row.
    pub name: String,
    /// The line number of a matched line; `None` on a file header row.
    pub line: Option<u32>,
    /// The matched line's text.
    pub text: String,
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
