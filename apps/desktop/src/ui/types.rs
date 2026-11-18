use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::ScrollState;

/// The sidebar's archived-conversation filter: which archived rows the
/// Workspaces list shows. It lives in the sidebar's view options (the sliders
/// menu) and is read by both the list predicate and the empty-state copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArchivedFilter {
    /// Archived conversations stay out of the list (DSH's default).
    #[default]
    Default,
    /// Every conversation, archived ones in their normal slots.
    Show,
    /// Only archived conversations.
    Only,
}

impl ArchivedFilter {
    /// The sentence the view-options menu draws under the label.
    pub fn description(self) -> &'static str {
        match self {
            Self::Default => "Hidden from the list",
            Self::Show => "Archived included",
            Self::Only => "Only archived conversations",
        }
    }
}

/// Which dimension the agent-management panel groups its roster by. The
/// panel's grouping control switches between the two, matching grok-build's
/// dashboard (`Ctrl+G`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgentDashboardGrouping {
    /// Awaiting input, working, idle, done and failed, in that order.
    #[default]
    State,
    /// The directory each agent works in, alphabetically.
    Directory,
}

impl AgentDashboardGrouping {
    /// The grouping a click on the panel's control switches to.
    pub fn next(self) -> Self {
        match self {
            Self::State => Self::Directory,
            Self::Directory => Self::State,
        }
    }

    /// What the panel's control reads: the grouping in force.
    pub fn label(self) -> &'static str {
        match self {
            Self::State => "Group: state",
            Self::Directory => "Group: directory",
        }
    }
}

/// The agent-management panel's own state: whether it is up, what narrows the
/// roster, and the drafts the panel owns across frames.
///
/// The panel is an overlay over the workspace, so the state lives with the
/// app rather than with an element: the tree is rebuilt every frame and only
/// this value survives it.
#[derive(Clone, Debug)]
pub struct AgentDashboardState {
    /// Whether the panel is up.
    pub open: bool,
    /// The roster's search text.
    pub query: String,
    pub search_focused: bool,
    /// The conversation the panel's cursor is on; `None` leaves the dispatch
    /// box in the panel's footer slot.
    pub selected: Option<String>,
    pub grouping: AgentDashboardGrouping,
    /// Scroll offset of the roster, shared with the app so a position survives
    /// the per-frame rebuild.
    pub scroll: Rc<RefCell<ScrollState>>,
    /// Scroll offset of the response card's output, kept apart from the
    /// roster's so reading a long response does not move the list.
    pub response_scroll: Rc<RefCell<ScrollState>>,
    /// The roster row the pointer was over on the last painted frame. Rows are
    /// built before the paint that learns the hover, so the row's hover-only
    /// state label and close control read it from here.
    pub hovered_row: Rc<RefCell<Option<String>>>,
    /// The reply draft for [`Self::selected`].
    pub reply: String,
    pub reply_focused: bool,
    /// The prompt the dispatch box holds for a new agent.
    pub dispatch: String,
    pub dispatch_focused: bool,
    /// Whether the prompt line is in dispatch mode rather than reply mode.
    /// The line is always drawn; this flag says a caret in it starts a new
    /// agent, and keeps the header's new-agent control lit while it does.
    pub dispatch_open: bool,
    /// The conversation being renamed, with its draft name. `None` while the
    /// pane is not renaming anything.
    pub rename: Option<(String, String)>,
    pub rename_focused: bool,
    /// The conversation whose delete button is armed: a second click removes
    /// it. Any other interaction disarms it.
    pub confirm_delete: Option<String>,
}

impl Default for AgentDashboardState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            search_focused: false,
            selected: None,
            grouping: AgentDashboardGrouping::State,
            scroll: Rc::new(RefCell::new(ScrollState::default())),
            response_scroll: Rc::new(RefCell::new(ScrollState::default())),
            hovered_row: Rc::new(RefCell::new(None)),
            reply: String::new(),
            reply_focused: false,
            dispatch: String::new(),
            dispatch_focused: false,
            dispatch_open: false,
            rename: None,
            rename_focused: false,
            confirm_delete: None,
        }
    }
}

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
    /// What the workflow is for, as its author wrote it: the objective a
    /// workflow-runs row shows under the name.
    pub description: String,
    /// The workflow's phases, in the order the record declares them.
    pub steps: Vec<WorkflowStepEntry>,
    pub trigger: String,
    pub enabled: bool,
    pub created_at: String,
}

/// One phase of a registered workflow: a named step that one agent runs.
#[derive(Clone, Debug)]
pub struct WorkflowStepEntry {
    pub name: String,
    /// The agent the step runs as. Empty when the record names none, which
    /// means no execution can be matched to the step.
    pub agent_id: String,
}

/// One settled run of a workflow, from the persisted run ledger.
#[derive(Clone, Debug)]
pub struct WorkflowRunEntry {
    pub id: String,
    pub workflow_id: String,
    /// `running`, `succeeded` or `failed`.
    pub status: String,
    /// Why the run failed, when it did.
    pub error: Option<String>,
    pub steps: Vec<WorkflowRunStepEntry>,
    /// Relative label, e.g. `2m ago`.
    pub started_at: String,
    /// How long the run has been going when the ledger was read. The work dock
    /// and the run view state it on a running row; a run with no parseable
    /// start states none rather than inventing a clock.
    pub elapsed: Option<std::time::Duration>,
    /// When `elapsed` was measured on this process's clock. A run still running
    /// has no `finished_at`, so the row keeps counting from here between ledger
    /// refreshes ([`WorkflowRunEntry::live_elapsed`]).
    pub elapsed_measured_at: Option<std::time::Instant>,
    pub finished_at: Option<String>,
    /// The value a script passed to `complete()`.
    pub output: Option<serde_json::Value>,
    pub log: Vec<String>,
}

impl WorkflowRunEntry {
    /// The run's clock as it stands now: a settled run's measured span, or a
    /// running run's span at the last read plus the time since.
    pub fn live_elapsed(&self) -> Option<std::time::Duration> {
        match (self.elapsed, self.elapsed_measured_at) {
            (Some(measured), Some(at)) if self.finished_at.is_none() => {
                Some(measured + at.elapsed())
            }
            _ => self.elapsed,
        }
    }
}

/// One phase of a run, in run order.
#[derive(Clone, Debug)]
pub struct WorkflowRunStepEntry {
    pub name: String,
    /// `skipped`, `succeeded` or `failed`.
    pub status: String,
    pub detail: Option<String>,
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
    Projects,
    Workflows,
    Executions,
    Timeline,
    Costs,
    Mcps,
}

/// Settings tab pages: the DSH information architecture.
///
/// Serialized with the pane that shows one: the page rides in the leaf (see
/// `PaneKind::Settings`), so it survives a save/load and a pane lift. The order
/// of [`Self::ALL`] is the rail's order; [`crate::ui::settings::rail_groups`]
/// adds the headings on top of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SettingsCategory {
    /// Profile, notifications and the behavioural switches.
    General,
    /// Theme, palette, text scale and the editor's save choice.
    Appearance,
    /// The models `[model.*]` declare, plus the agent CLIs `[harness.*]`.
    Providers,
    /// The MCP servers `[mcp_servers.*]` declare.
    Connectors,
    /// Approval policy, input mode and the remote configuration.
    Agent,
    /// The groups in `environment.toml`.
    Environments,
    /// The saved hosts in `hosts.toml`, plus the read-only `~/.ssh` view.
    Hosts,
    /// The encrypted vault's secrets.
    Secrets,
    /// The keyboard's bindings (read-only in this build).
    Shortcuts,
    /// This build, the last check and the offered release.
    Updates,
    /// The settings files themselves: path, mode, revision, parse problems.
    Advanced,
    /// The version, the home directory and the files.
    About,
}

impl SettingsCategory {
    /// Every page the settings surface can open, in rail order.
    pub const ALL: &'static [SettingsCategory] = &[
        SettingsCategory::General,
        SettingsCategory::Appearance,
        SettingsCategory::Providers,
        SettingsCategory::Connectors,
        SettingsCategory::Agent,
        SettingsCategory::Environments,
        SettingsCategory::Hosts,
        SettingsCategory::Secrets,
        SettingsCategory::Shortcuts,
        SettingsCategory::Updates,
        SettingsCategory::Advanced,
        SettingsCategory::About,
    ];

    /// The label the rail and the header breadcrumb show.
    pub fn label(self) -> &'static str {
        match self {
            SettingsCategory::General => "General",
            SettingsCategory::Appearance => "Appearance",
            SettingsCategory::Providers => "Providers",
            SettingsCategory::Connectors => "Connectors",
            SettingsCategory::Agent => "Agent",
            SettingsCategory::Environments => "Environments",
            SettingsCategory::Hosts => "Hosts",
            SettingsCategory::Secrets => "Secrets",
            SettingsCategory::Shortcuts => "Shortcuts",
            SettingsCategory::Updates => "Updates",
            SettingsCategory::Advanced => "Advanced",
            SettingsCategory::About => "About",
        }
    }

    /// The old spelling of a page, so a layout saved before the rework reads
    /// back to the page it meant.
    pub fn from_legacy(name: &str) -> Option<Self> {
        match name {
            "Profile" => Some(Self::General),
            "Mouse" | "EditorInput" | "Editor & Input" => Some(Self::Appearance),
            "AgentApproval" | "Agent & Approval" => Some(Self::Agent),
            "Environment" => Some(Self::Environments),
            "Connections" => Some(Self::Hosts),
            "Models" => Some(Self::Providers),
            label => Self::ALL.iter().copied().find(|page| page.label() == label),
        }
    }
}

/// One interaction in the first-run onboarding overlay. The action layer owns
/// the store handle, so finishing and replaying run there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnboardingAction {
    SelectUsage(String),
    NameChanged(String),
    SetDataSharing(bool),
    Next,
    Back,
    Replay,
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
    /// The word the settings page uses for this routing.
    pub fn label(self) -> &'static str {
        match self {
            WorkspaceRouting::Local => "Local",
            WorkspaceRouting::Remote => "Remote",
        }
    }

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

/// One `[remote_agents.<name>]` table as Settings → Agent & Approval draws it:
/// the name the user gave the agent, the provider behind it, and the
/// credentials that provider still needs. Nothing here provisions a sandbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteAgentEntry {
    pub name: String,
    pub provider: String,
    pub enabled: bool,
    /// The credential keys the table has not set, named as environment
    /// variables a user would supply them as.
    pub missing: Vec<&'static str>,
    /// `ready`, `disabled`, or the missing keys: the config's own words, so
    /// the page cannot come to say something the config does not.
    pub status: String,
}

impl RemoteAgentEntry {
    pub fn from_config(name: &str, config: &goble_core::config::RemoteAgentConfig) -> Self {
        Self {
            name: name.to_string(),
            provider: config.provider.as_str().to_string(),
            enabled: config.enabled,
            missing: config.missing_credentials(),
            status: config.status_label(),
        }
    }

    /// The right of the row: `provider · status`.
    pub fn detail(&self) -> String {
        format!("{} · {}", self.provider, self.status)
    }
}

/// What the last cloud-agent deploy reported, for the Remote row Settings →
/// Agent & Approval already draws. The action starts one; its outcome arrives
/// from the desktop service as `remote:deploy`, so this is the row's whole view
/// of a deploy that is in flight or has landed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RemoteDeploy {
    /// This run has not deployed a sandbox.
    #[default]
    Idle,
    /// A deploy was started for `agent` on `provider` and has not reported back.
    Deploying { agent: String, provider: String },
    /// The sandbox exists and its binding is persisted: `provider · name`.
    Ready { sandbox: String },
    /// The deploy stopped, with the sentence naming the cause.
    Failed { message: String },
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
