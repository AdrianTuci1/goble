//! UI builder for the native Goble app.
//!
//! Builds the complete element tree from a [`UiSnapshot`] / [`AiSnapshot`]
//! every frame. Lives in the executable (matching warp-new's "thick app"
//! model) so the app owns both the state and the tree; there is no separate
//! hot-reusable dylib or ABI boundary anymore.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use goble_harness_types::MediumKind;
use goble_ui::elements::{
    AgentCardUi, AppContext, ChatMessage as UiChatMessage, Container, ConversationEntry,
    CrossAxisAlignment, Divider, Element, Expanded, Fill, Flex, MainAxisSize, PopupMenuItem,
};
use goble_ui::theme::ColorToken;
use goble_ui::{
    vec2f, Dialog, SettingsPage, Sheet, Stack, DIALOG_DEFAULT_WIDTH, SHEET_DEFAULT_WIDTH,
};

use crate::terminal::TerminalRegistry;

pub mod chat;
pub mod connectors;
pub mod crons;
pub mod media;
pub mod model_form;
pub mod palette;
pub mod panes;
use palette::PaletteCommand;
pub mod projects;
pub mod screen;
pub mod shell;
pub mod sidebar;
pub mod space_bar;
pub mod terminal;
pub mod vault;

/// Width of the left conversation sidebar.
pub const SIDEBAR_WIDTH: f32 = 300.0;

/// Width of the connectors sheet (wider than the default panels).
pub const CONNECTORS_WIDTH: f32 = 480.0;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTab {
    Chat,
    Settings,
    Projects,
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

/// Which kind of content a pane hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PaneKind {
    Chat,
    Terminal,
}

/// Which way a pane tree splits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SplitDir {
    /// First pane on the left / top; a horizontal split ("in 2").
    Horizontal,
    /// First pane on top / bottom; a vertical split ("down").
    Vertical,
}

/// A compass direction used to move focus between panes (Ctrl/Cmd+Arrow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavDir {
    Up,
    Down,
    Left,
    Right,
}

/// A pane in a space. A leaf hosts content; a `Split` divides into two panes.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Pane {
    Leaf { id: u64, kind: PaneKind },
    Split {
        id: u64,
        dir: SplitDir,
        ratio: f32,
        first: Box<Pane>,
        second: Box<Pane>,
    },
}

impl Pane {
    fn leaf(id: u64, kind: PaneKind) -> Self {
        Pane::Leaf { id, kind }
    }

    /// This pane's own id (leaf id or split id).
    pub fn id(&self) -> u64 {
        match self {
            Pane::Leaf { id, .. } | Pane::Split { id, .. } => *id,
        }
    }

    /// The id of the first (top/leftmost) leaf under this pane.
    pub fn first_leaf_id(&self) -> u64 {
        match self {
            Pane::Leaf { id, .. } => *id,
            Pane::Split { first, .. } => first.first_leaf_id(),
        }
    }

    /// The maximum pane id anywhere in the subtree.
    pub fn max_id(&self) -> u64 {
        match self {
            Pane::Leaf { id, .. } => *id,
            Pane::Split {
                id,
                first,
                second,
                ..
            } => (*id).max(first.max_id()).max(second.max_id()),
        }
    }

    /// Whether a leaf with id `target` exists in the subtree.
    pub fn contains_leaf(&self, target: u64) -> bool {
        match self {
            Pane::Leaf { id, .. } => *id == target,
            Pane::Split { first, second, .. } => first.contains_leaf(target) || second.contains_leaf(target),
        }
    }

    fn is_leaf(&self, target: u64) -> bool {
        matches!(self, Pane::Leaf { id, .. } if *id == target)
    }

    /// Replace the leaf `target` with a `Split` whose first child is the old
    /// leaf and whose second child is a fresh leaf of the same kind. Returns the
    /// new leaf id on success.
    fn split_leaf(&mut self, target: u64, dir: SplitDir, split_id: u64, second_id: u64) -> bool {
        match self {
            Pane::Leaf { id, kind } if *id == target => {
                let id = *id;
                let kind = *kind;
                *self = Pane::Split {
                    id: split_id,
                    dir,
                    ratio: 0.5,
                    first: Box::new(Pane::leaf(id, kind)),
                    second: Box::new(Pane::leaf(second_id, kind)),
                };
                true
            }
            Pane::Split { first, second, .. } => {
                first.split_leaf(target, dir, split_id, second_id)
                    || second.split_leaf(target, dir, split_id, second_id)
            }
            _ => false,
        }
    }

    /// Like [`Pane::split_leaf`] but the fresh leaf has kind `second_kind`, so
    /// splitting into a terminal pane can use a different kind from the source.
    fn split_leaf_into(
        &mut self,
        target: u64,
        dir: SplitDir,
        split_id: u64,
        second_id: u64,
        second_kind: PaneKind,
    ) -> bool {
        match self {
            Pane::Leaf { id, kind } if *id == target => {
                let id = *id;
                let kind = *kind;
                *self = Pane::Split {
                    id: split_id,
                    dir,
                    ratio: 0.5,
                    first: Box::new(Pane::leaf(id, kind)),
                    second: Box::new(Pane::leaf(second_id, second_kind)),
                };
                true
            }
            Pane::Split { first, second, .. } => {
                first.split_leaf_into(target, dir, split_id, second_id, second_kind)
                    || second.split_leaf_into(target, dir, split_id, second_id, second_kind)
            }
            _ => false,
        }
    }

    /// Update the ratio of the split node with id `target`.
    fn set_ratio(&mut self, target: u64, ratio: f32) -> bool {
        match self {
            Pane::Split { id, ratio: r, .. } if *id == target => {
                *r = ratio.clamp(0.05, 0.95);
                true
            }
            Pane::Split { first, second, .. } => {
                first.set_ratio(target, ratio) || second.set_ratio(target, ratio)
            }
            _ => false,
        }
    }

    /// The approximate normalized bounding rect (0..1 root space) of every leaf
    /// pane in the subtree, in traversal order. Used for spatial focus
    /// navigation (Ctrl/Cmd+Arrows), where the exact pixel geometry is not
    /// available at the tree level but the split ratios are enough to order
    /// leaves left-to-right / top-to-bottom.
    fn collect_regions(&self, rect: goble_ui::RectF, out: &mut Vec<(u64, goble_ui::RectF)>) {
        match self {
            Pane::Leaf { id, .. } => out.push((*id, rect)),
            Pane::Split {
                dir, ratio, first, second, ..
            } => {
                let (first_rect, second_rect) = match dir {
                    SplitDir::Horizontal => {
                        let split_x = rect.min_x() + rect.width() * *ratio;
                        (
                            goble_ui::rectf(
                                rect.min_x(),
                                rect.min_y(),
                                split_x - rect.min_x(),
                                rect.height(),
                            ),
                            goble_ui::rectf(
                                split_x,
                                rect.min_y(),
                                rect.max_x() - split_x,
                                rect.height(),
                            ),
                        )
                    }
                    SplitDir::Vertical => {
                        let split_y = rect.min_y() + rect.height() * *ratio;
                        (
                            goble_ui::rectf(
                                rect.min_x(),
                                rect.min_y(),
                                rect.width(),
                                split_y - rect.min_y(),
                            ),
                            goble_ui::rectf(
                                rect.min_x(),
                                split_y,
                                rect.width(),
                                rect.max_y() - split_y,
                            ),
                        )
                    }
                };
                first.collect_regions(first_rect, out);
                second.collect_regions(second_rect, out);
            }
        }
    }

    fn leaf_regions(&self) -> Vec<(u64, goble_ui::RectF)> {
        let mut out = Vec::new();
        self.collect_regions(goble_ui::rectf(0.0, 0.0, 1.0, 1.0), &mut out);
        out
    }

    /// Remove the leaf `target`, replacing its parent split with the surviving
    /// sibling. Returns the id of a leaf in the surviving side, or `None` when
    /// `target` is the root (nothing to remove).
    fn remove_leaf(&mut self, target: u64) -> Option<u64> {
        match self {
            Pane::Leaf { .. } => None,
            Pane::Split { first, second, .. } => {
                if first.is_leaf(target) {
                    let survivor = std::mem::replace(
                        second,
                        Box::new(Pane::leaf(0, PaneKind::Chat)),
                    );
                    *self = *survivor;
                    Some(self.first_leaf_id())
                } else if second.is_leaf(target) {
                    let survivor = std::mem::replace(
                        first,
                        Box::new(Pane::leaf(0, PaneKind::Chat)),
                    );
                    *self = *survivor;
                    Some(self.first_leaf_id())
                } else {
                    first.remove_leaf(target).or_else(|| second.remove_leaf(target))
                }
            }
        }
    }
}

/// A "space": a named pane tree shown as a tab in the top bar (warp-new style).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Space {
    pub name: String,
    pub root: Pane,
    /// The environment medium this space runs on (a [`goble_harness_types::MediumId`]
    /// string). A space created through the topbar "+" menu picks its default
    /// environment here; `#[serde(default)]` keeps older persisted layouts parseable.
    #[serde(default)]
    pub medium: String,
}

impl Space {
    pub fn new(name: impl Into<String>, root: Pane) -> Self {
        Self {
            name: name.into(),
            root,
            medium: "local".to_string(),
        }
    }

    /// Set this space's default environment medium (used when the space is
    /// created through the topbar "+" menu).
    pub fn with_medium(mut self, medium: impl Into<String>) -> Self {
        self.medium = medium.into();
        self
    }

    /// Split the leaf `target`, allocating fresh ids from `next_id`. Returns
    /// the id of the newly created pane (which becomes the active pane).
    pub fn split(&mut self, target: u64, dir: SplitDir, next_id: &mut u64) -> Option<u64> {
        let split_id = *next_id;
        *next_id += 1;
        let second_id = *next_id;
        *next_id += 1;
        if self.root.split_leaf(target, dir, split_id, second_id) {
            Some(second_id)
        } else {
            None
        }
    }

    /// Split the leaf `target` into a `Split` whose new second leaf has
    /// `second_kind` (used to open a terminal pane off a chat pane).
    pub fn split_with_kind(
        &mut self,
        target: u64,
        dir: SplitDir,
        next_id: &mut u64,
        second_kind: PaneKind,
    ) -> Option<u64> {
        let split_id = *next_id;
        *next_id += 1;
        let second_id = *next_id;
        *next_id += 1;
        if self.root.split_leaf_into(target, dir, split_id, second_id, second_kind) {
            Some(second_id)
        } else {
            None
        }
    }

    pub fn set_ratio(&mut self, split_id: u64, ratio: f32) -> bool {
        self.root.set_ratio(split_id, ratio)
    }

    /// Close the leaf `target`, returning the id of the pane to focus next.
    pub fn close(&mut self, target: u64) -> Option<u64> {
        self.root.remove_leaf(target)
    }

    /// Find the leaf pane spatially adjacent to `from` in the given `dir`,
    /// using approximate normalized leaf regions. Returns `None` when there is
    /// no leaf in that direction (e.g. `from` is on the window edge). Ties are
    /// broken by distance along the perpendicular axis so the nearest
    /// same-row/same-column pane wins.
    pub fn navigate(&self, from: u64, dir: NavDir) -> Option<u64> {
        let regions = self.root.leaf_regions();
        let current = regions.iter().find_map(|(id, r)| (*id == from).then_some(*r))?;

        let center_x = |r: &goble_ui::RectF| r.min_x() + r.width() * 0.5;
        let center_y = |r: &goble_ui::RectF| r.min_y() + r.height() * 0.5;

        let mut best: Option<(u64, f32, f32)> = None;
        for (id, r) in &regions {
            if *id == from {
                continue;
            }
            let (primary, secondary) = match dir {
                NavDir::Up => {
                    if r.max_y() <= current.min_y() {
                        (
                            current.min_y() - r.max_y(),
                            (center_x(r) - center_x(&current)).abs(),
                        )
                    } else {
                        continue;
                    }
                }
                NavDir::Down => {
                    if r.min_y() >= current.max_y() {
                        (
                            r.min_y() - current.max_y(),
                            (center_x(r) - center_x(&current)).abs(),
                        )
                    } else {
                        continue;
                    }
                }
                NavDir::Left => {
                    if r.max_x() <= current.min_x() {
                        (
                            current.min_x() - r.max_x(),
                            (center_y(r) - center_y(&current)).abs(),
                        )
                    } else {
                        continue;
                    }
                }
                NavDir::Right => {
                    if r.min_x() >= current.max_x() {
                        (
                            r.min_x() - current.max_x(),
                            (center_y(r) - center_y(&current)).abs(),
                        )
                    } else {
                        continue;
                    }
                }
            };
            let better = match best {
                None => true,
                Some((_, bp, bs)) => {
                    primary < bp - 1e-6
                        || ((primary - bp).abs() < 1e-6 && secondary < bs - 1e-6)
                }
            };
            if better {
                best = Some((*id, primary, secondary));
            }
        }
        best.map(|(id, _, _)| id)
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

/// Per-pane chat data used to render a chat leaf: its own transcript, composer
/// draft, working-directory label, suspended ask and queued prompt. Each chat
/// pane is an independent session, so this is keyed by pane id and never shared.
#[derive(Clone, Debug)]
pub struct PaneChatSnapshot {
    pub conversation_id: String,
    pub messages: Vec<UiChatMessage>,
    pub composer_draft: String,
    pub composer_path: String,
    pub pending_ask: Option<goble_ui::AskUserUi>,
    pub queued_prompt: Option<String>,
    pub agent_busy: bool,
    /// A live remote-desktop frame the harness handed off, rendered inline.
    pub inline_screen: Option<ScreenFrameSnapshot>,
    /// A detected BYOH handoff URI (`rdp://…` / `goble://desktop?…`), rendered
    /// as a clickable "open remote desktop" affordance when no live frame yet.
    pub screen_link: Option<String>,
}

/// The composer's warp-new style context pills: which harness (the selected
/// environment medium), which working directory (a session's path) and which
/// git branch the next turn is scoped to. The labels/menu items are derived
/// from the environment tree each frame; the menu open flags are app-owned so
/// they survive the per-frame element rebuild.
#[derive(Clone, Debug)]
pub struct ComposerContext {
    pub harness_label: String,
    pub harness_ids: Vec<String>,
    pub harness_items: Vec<PopupMenuItem>,
    pub harness_menu_open: Rc<RefCell<bool>>,
    pub dir_label: String,
    pub dir_ids: Vec<String>,
    pub dir_items: Vec<PopupMenuItem>,
    pub dir_menu_open: Rc<RefCell<bool>>,
    pub branch_label: String,
    pub branch_ids: Vec<String>,
    pub branch_items: Vec<PopupMenuItem>,
    pub branch_menu_open: Rc<RefCell<bool>>,
}

/// Plain snapshot of the main UI state used to build the tree. Owned by the
/// host app; rendered from scratch every frame so state changes show up live.
#[derive(Clone, Debug)]
pub struct UiSnapshot {
    pub current_tab: AppTab,
    pub conversations: Vec<ConversationEntry>,
    pub selected_id: Option<String>,
    pub search_query: String,
    pub search_focused: bool,
    pub new_conversation_draft: String,
    pub create_focused: bool,
    pub chat_messages: Vec<UiChatMessage>,
    /// A suspended agent question shown inline at the end of the transcript.
    pub pending_ask: Option<goble_ui::AskUserUi>,
    /// A prompt the user sent while the agent was busy, shown as a pending block.
    pub queued_prompt: Option<String>,
    pub composer_draft: String,
    pub composer_focused: bool,
    /// Model choices shown in the composer's model dropdown.
    pub models: Vec<String>,
    /// Currently selected model (shown as the composer's model label).
    pub selected_model: String,
    /// Harnesses the composer can route a turn to (native + registered BYOH).
    pub harnesses: Vec<HarnessEntry>,
    /// The harness the active pane routes turns to (default `internal`).
    pub selected_harness: String,
    /// App-owned open flags for the composer model / account menus, so open
    /// state survives the per-frame element rebuild.
    pub model_menu_open: Rc<RefCell<bool>>,
    pub profile_menu_open: Rc<RefCell<bool>>,
    pub agent_name: String,
    pub agent_busy: bool,
    /// Whether the agent auto-approves `ask_user` questions.
    pub auto_approve: bool,
    pub right_sidebar_open: bool,
    /// Whether the agent/window is fullscreen (borderless). Rendered as the
    /// checked state of the agent header menu's fullscreen item.
    pub fullscreen: bool,
    /// App-owned open flag for the agent header's 3-dots menu.
    pub agent_header_menu_open: Rc<RefCell<bool>>,
    pub crons_open: bool,
    pub crons: Vec<CronEntry>,
    pub settings_page: SettingsPage,
    pub settings_profile_name: String,
    pub settings_profile_email: String,
    pub settings_dark_mode: bool,
    pub settings_llm_provider: String,
    pub settings_llm_model: String,
    pub settings_llm_api_key: String,
    pub settings_llm_base_url: String,
    pub settings_llm_temperature: String,
    pub settings_workers: Vec<(String, String, String, bool)>,
    pub settings_cluster_name: String,
    pub settings_cluster_configured: bool,
    pub settings_authorized_keys: Vec<(String, String, String)>,
    pub settings_vault_unlocked: bool,
    /// First-run: whether the "configure a model key" banner is shown in chat.
    pub show_llm_key_banner: bool,
    /// First-run: whether the "local or remote workspace?" choice is shown.
    pub show_workspace_choice: bool,
    /// First-run routing decision once the user picks Local or Remote.
    pub workspace_routing: Option<WorkspaceRouting>,
    /// First-run: whether the model-provider dialog is open over the chat.
    pub llm_dialog_open: bool,
    /// Editable model-form fields, held in app-owned ref cells so the text and
    /// focus survive the per-frame element rebuild while the dialog is open.
    pub llm_dialog_provider: Rc<RefCell<String>>,
    pub llm_dialog_model: Rc<RefCell<String>>,
    pub llm_dialog_api_key: Rc<RefCell<String>>,
    pub llm_dialog_base_url: Rc<RefCell<String>>,
    pub llm_dialog_temperature: Rc<RefCell<String>>,
    pub llm_dialog_focus: Rc<RefCell<Option<LlmFormField>>>,
    pub sidebar_width: f32,
    pub sidebar_dragging: bool,
    /// Per-card interaction state (hover / delete menu), shared with the
    /// card elements so selections and menus persist across frames.
    pub agent_cards: HashMap<String, Rc<RefCell<AgentCardUi>>>,
    /// Hover flag for the sidebar's "New agent" row, owned here so the row's
    /// highlight survives the per-frame element rebuild.
    pub new_agent_hover: Rc<RefCell<bool>>,
    /// Multiple "spaces": each is a pane tree rendered as a top-bar tab. The
    /// active space's tree is what `build_ui` renders as the main content.
    pub spaces: Vec<Space>,
    pub active_space: usize,
    /// Hovered space tab index (app state so it survives the per-frame rebuild).
    pub space_hover: Option<usize>,
    /// Space tab currently pressed (MouseDown), used to detect a drag start.
    pub space_press: Option<usize>,
    /// Space tab currently being dragged (reordering in progress).
    pub space_drag: Option<usize>,
    /// The focused leaf pane (target for splits/closes, highlighted).
    pub active_pane_id: u64,
    /// The split node currently being dragged, if any.
    pub dragging_pane_id: Option<u64>,
    /// The current working path shown in the composer.
    pub composer_path: String,
    /// Warp-new composer context pills (harness / working dir / git branch).
    pub composer_context: ComposerContext,
    /// Per-pane hover flags (keyed by pane id) so pane-header hover survives
    /// the per-frame element rebuild.
    pub pane_hover: HashMap<u64, Rc<RefCell<bool>>>,
    /// Per-pane chat data (transcript + draft + path) keyed by pane id, so each
    /// chat pane renders its own independent session.
    pub pane_chat: HashMap<u64, PaneChatSnapshot>,
    /// Live terminal sessions + input mirrors, shared with the terminal pane
    /// elements so they can spawn/read/write a PTY without blocking the UI
    /// thread. Cloned by `Rc` each frame (never the sessions themselves).
    pub terminal: Rc<RefCell<TerminalRegistry>>,
    /// Whether the Cmd+K command palette overlay is open.
    pub command_palette_open: bool,
    /// The palette's filter text, used to narrow the command list.
    pub command_palette_query: String,
    /// The palette's selected row index.
    pub command_palette_index: usize,
    /// First run: a compact "getting started" hint shown once the workspace
    /// choice is made, so the first-run flow lands on the chat with a tip.
    pub show_onboarding_tip: bool,
    /// App-owned open flag for the topbar "+" menu (choose an environment to
    /// open a new space, or add a new medium).
    pub add_space_menu_open: Rc<RefCell<bool>>,
    /// App-owned open flag for the topbar compact environment selector.
    pub env_selector_open: Rc<RefCell<bool>>,
    /// Whether the "add a new medium" dialog is open over the app.
    pub add_medium_dialog_open: bool,
    /// The new medium's name as typed in the add-medium dialog.
    pub add_medium_draft: String,
    /// Whether the add-medium dialog's text field is focused.
    pub add_medium_focused: bool,
}

/// Plain snapshot of the AI domain state (vault + MCP connectors) used to
/// build the auxiliary panels. Owned by the host app.
#[derive(Clone, Debug)]
pub struct AiSnapshot {
    pub connectors_open: bool,
    pub vault_open: bool,
    pub vault_unlocked: bool,
    pub vault_secrets: Vec<VaultSecretEntry>,
    pub vault_unlock_draft: String,
    pub vault_new_key: String,
    pub vault_new_value: String,
    pub vault_error: Option<String>,
    pub connector_search: String,
    pub connectors: Vec<McpServerEntry>,
    pub install_open: bool,
    pub install_editing_id: Option<String>,
    pub install_name: String,
    pub install_source: String,
    pub install_source_value: String,
    pub install_search_query: String,
    pub install_search_results: Vec<McpSearchEntry>,
    pub install_selected_secrets: Vec<String>,
    pub install_error: Option<String>,
    pub installing: bool,
}

/// Callbacks supplied by the host app for the main view. Created fresh on
/// every rebuild; they mutate app-owned state, which is rendered back on the
/// next frame.
pub struct UiActions {
    pub on_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_search_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_create_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_create_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_create_submit: Rc<RefCell<dyn FnMut()>>,
    pub on_select_conversation: Rc<RefCell<dyn FnMut(String)>>,
    pub on_select_tab: Rc<RefCell<dyn FnMut(AppTab)>>,
    pub on_composer_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_composer_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_send_message: Rc<RefCell<dyn FnMut(String)>>,
    /// Fire when the composer draft starts with `/` (slash command): the host
    /// opens the command palette.
    pub on_composer_slash: Rc<RefCell<dyn FnMut()>>,
    pub on_attach: Rc<RefCell<dyn FnMut()>>,
    pub on_voice: Rc<RefCell<dyn FnMut()>>,
    pub on_select_model: Rc<RefCell<dyn FnMut()>>,
    /// Select a specific model from the composer dropdown by display name.
    pub on_model_select: Rc<RefCell<dyn FnMut(String)>>,
    /// Select the composer harness (environment medium) by medium id.
    pub on_select_harness: Rc<RefCell<dyn FnMut(String)>>,
    /// Select the composer working directory (session) by session id.
    pub on_select_dir: Rc<RefCell<dyn FnMut(String)>>,
    /// Select the composer git branch by branch name.
    pub on_select_branch: Rc<RefCell<dyn FnMut(String)>>,
    pub on_copy: Rc<RefCell<dyn FnMut()>>,
    pub on_restart: Rc<RefCell<dyn FnMut()>>,
    /// Rename the current agent/conversation (agent-header 3-dots menu).
    pub on_rename_agent: Rc<RefCell<dyn FnMut()>>,
    /// Clear the active pane's transcript (agent-header 3-dots menu).
    pub on_clear_transcript: Rc<RefCell<dyn FnMut()>>,
    pub on_stop: Rc<RefCell<dyn FnMut()>>,
    /// Submit the composed answer of the inline ask-user card. The optional
    /// `(name, value)` carries a credential entered in the card, threaded out
    /// separately from the answer so the secret never enters the transcript.
    pub on_answer_ask: Rc<RefCell<dyn FnMut(String, Option<(String, String)>)>>,
    /// Skip the inline ask-user card.
    pub on_skip_ask: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the auto-approve (autonomy) switch above the composer.
    pub on_toggle_auto_approve: Rc<RefCell<dyn FnMut(bool)>>,
    /// Send a queued prompt now (interrupting the in-flight turn).
    pub on_send_queued: Rc<RefCell<dyn FnMut()>>,
    /// Dismiss a queued prompt.
    pub on_dismiss_queued: Rc<RefCell<dyn FnMut()>>,
    pub on_menu: Rc<RefCell<dyn FnMut()>>,
    pub on_inbox: Rc<RefCell<dyn FnMut()>>,
    pub on_settings: Rc<RefCell<dyn FnMut()>>,
    pub on_projects: Rc<RefCell<dyn FnMut()>>,
    pub on_plugins: Rc<RefCell<dyn FnMut()>>,
    pub on_open_crons: Rc<RefCell<dyn FnMut()>>,
    pub on_close_crons: Rc<RefCell<dyn FnMut()>>,
    pub on_toggle_right_sidebar: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the agent/window fullscreen (borderless). Flips app state and
    /// requests the platform window to enter/leave fullscreen.
    pub on_toggle_fullscreen: Rc<RefCell<dyn FnMut()>>,
    pub on_cron_create: Rc<RefCell<dyn FnMut()>>,
    pub on_cron_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_cron_trigger: Rc<RefCell<dyn FnMut(String)>>,
    /// Begin dragging the sidebar divider at the given pointer x.
    pub on_sidebar_drag_start: Rc<RefCell<dyn FnMut(f32)>>,
    /// Move the divider while dragging, given the pointer x.
    pub on_sidebar_drag_move: Rc<RefCell<dyn FnMut(f32)>>,
    /// Finish a drag; the divider settles at the last width.
    pub on_sidebar_drag_end: Rc<RefCell<dyn FnMut()>>,
    /// Split the active pane side-by-side (Ctrl/Cmd+Space).
    pub on_split_right: Rc<RefCell<dyn FnMut()>>,
    /// Split the active pane downward (Cmd+Shift+D).
    pub on_split_down: Rc<RefCell<dyn FnMut()>>,
    /// Close the active pane, focusing the surviving sibling.
    pub on_close_pane: Rc<RefCell<dyn FnMut()>>,
    /// Create a new terminal pane by splitting the active pane downward.
    pub on_new_terminal: Rc<RefCell<dyn FnMut()>>,
    /// Route a terminal pane's current input to the agent turn path (Cmd+Enter),
    /// using that pane's own conversation + cwd + medium/project scope.
    pub on_terminal_command: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Switch the active space by index.
    pub on_select_space: Rc<RefCell<dyn FnMut(usize)>>,
    /// Add a new space (single chat pane) and make it active.
    pub on_add_space: Rc<RefCell<dyn FnMut()>>,
    /// Add a new space whose default environment medium is `medium_id`, making
    /// it active and setting the new thread's workspace routing to that medium.
    pub on_add_space_with_medium: Rc<RefCell<dyn FnMut(String)>>,
    /// Open the "add a new medium" dialog (from the topbar "+" menu).
    pub on_open_add_medium: Rc<RefCell<dyn FnMut()>>,
    /// Update the add-medium dialog's text field.
    pub on_add_medium_draft_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Track whether the add-medium dialog's text field is focused.
    pub on_add_medium_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    /// Close the add-medium dialog without saving.
    pub on_add_medium_close: Rc<RefCell<dyn FnMut()>>,
    /// Pointer pressed down on a space tab (begins a potential drag).
    pub on_space_press: Rc<RefCell<dyn FnMut(usize)>>,
    /// Pointer moved over the space bar (updates the hovered tab).
    pub on_space_hover: Rc<RefCell<dyn FnMut(Option<usize>)>>,
    /// Drag a space tab to a new position (reorder in the tab list).
    pub on_space_reorder: Rc<RefCell<dyn FnMut(usize, usize)>>,
    /// Pointer released (ends a press/drag and clears the drag state).
    pub on_space_release: Rc<RefCell<dyn FnMut()>>,
    /// Focus a specific leaf pane (e.g. by clicking its header).
    pub on_pane_activate: Rc<RefCell<dyn FnMut(u64)>>,
    /// Move focus to the pane adjacent to the active one (Ctrl/Cmd+Arrow).
    pub on_pane_navigate: Rc<RefCell<dyn FnMut(NavDir)>>,
    /// Begin dragging the split divider for pane `id`.
    pub on_pane_drag_start: Rc<RefCell<dyn FnMut(u64)>>,
    /// Drag the split divider for pane `id` to the given ratio.
    pub on_pane_drag_move: Rc<RefCell<dyn FnMut(u64, f32)>>,
    /// Finish dragging a split divider.
    pub on_pane_drag_end: Rc<RefCell<dyn FnMut()>>,
    /// Delete a conversation/agent card from the list.
    pub on_agent_delete: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: return to the previous view (chat).
    pub on_settings_back: Rc<RefCell<dyn FnMut()>>,
    /// Settings: switch the active settings page.
    pub on_settings_navigate: Rc<RefCell<dyn FnMut(SettingsPage)>>,
    /// Settings: toggle dark mode.
    pub on_toggle_dark_mode: Rc<RefCell<dyn FnMut(bool)>>,
    /// Settings: save the profile (name, email).
    pub on_save_profile: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: save the LLM config (provider, model, api_key, base_url, temperature).
    pub on_save_llm: Rc<RefCell<dyn FnMut(String, String, String, String, String)>>,
    /// Settings: register a worker (name, url).
    pub on_add_worker: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: remove a worker by id.
    pub on_remove_worker: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: unlock the vault with a passphrase.
    pub on_vault_unlock: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: create a cluster identity (name, passphrase).
    pub on_create_cluster: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: unlock the cluster identity (passphrase).
    pub on_unlock_cluster: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: add an authorized key (name, pem, fingerprint).
    pub on_add_authorized_key: Rc<RefCell<dyn FnMut(String, String, String)>>,
    /// Settings: remove an authorized key by id.
    pub on_remove_authorized_key: Rc<RefCell<dyn FnMut(String)>>,
    /// Close the inline remote-desktop block the harness handed off to.
    pub on_close_inline_screen: Rc<RefCell<dyn FnMut()>>,
    /// Open a remote desktop from a detected BYOH handoff URI.
    pub on_open_screen_link: Rc<RefCell<dyn FnMut(String)>>,
    /// First-run: open Settings->LLM to configure a model key (banner click).
    pub on_config_llm_key: Rc<RefCell<dyn FnMut()>>,
    /// First-run: choose the workspace routing (Local or Remote).
    pub on_choose_workspace: Rc<RefCell<dyn FnMut(WorkspaceRouting)>>,
    /// First-run: close the model-provider dialog without saving.
    pub on_close_llm_dialog: Rc<RefCell<dyn FnMut()>>,
    /// First-run: dismiss the model-key banner without configuring a key,
    /// marking onboarding complete so the flow is not a dead end.
    pub on_dismiss_llm_key_banner: Rc<RefCell<dyn FnMut()>>,
    /// First-run: dismiss the workspace choice without deciding, marking
    /// onboarding complete.
    pub on_dismiss_workspace_choice: Rc<RefCell<dyn FnMut()>>,
    /// First-run: dismiss the compact getting-started tip.
    pub on_dismiss_onboarding_tip: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the Cmd+K command palette overlay.
    pub on_toggle_command_palette: Rc<RefCell<dyn FnMut()>>,
    /// Close the Cmd+K command palette overlay.
    pub on_close_command_palette: Rc<RefCell<dyn FnMut()>>,
    /// Update the command palette's filter text, resetting the selection.
    pub on_command_palette_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Set the command palette's selected index (clamped by the palette, which
    /// knows the filtered list length).
    pub on_command_palette_move: Rc<RefCell<dyn FnMut(usize)>>,
}

/// Callbacks supplied by the host app for the AI domain (vault + connectors).
pub struct AiActions {
    pub on_open_connectors: Rc<RefCell<dyn FnMut()>>,
    pub on_close_connectors: Rc<RefCell<dyn FnMut()>>,
    pub on_open_vault: Rc<RefCell<dyn FnMut()>>,
    pub on_close_vault: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_unlock_draft_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_unlock: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_new_key_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_new_value_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_secret_add: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_secret_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_open: Rc<RefCell<dyn FnMut()>>,
    pub on_install_edit: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_pick: Rc<RefCell<dyn FnMut(String, String, String)>>,
    pub on_install_close: Rc<RefCell<dyn FnMut()>>,
    pub on_install_name_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_source_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_source_value_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_secret_toggle: Rc<RefCell<dyn FnMut(String, bool)>>,
    pub on_install_submit: Rc<RefCell<dyn FnMut()>>,
    pub on_connector_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_discover: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_toggle: Rc<RefCell<dyn FnMut(String, bool)>>,
}

/// A single session/run shown in the per-project observability panel.
#[derive(Clone, Debug)]
pub struct ProjectSessionEntry {
    pub session_id: String,
    pub medium: String,
    pub created_at: String,
    pub status: String,
}

/// A project group in the projects panel: its sessions and a "what's running"
/// status derived from the project's tasks.
#[derive(Clone, Debug)]
pub struct ProjectEntry {
    pub project_id: String,
    pub directory: String,
    pub status: String,
    pub sessions: Vec<ProjectSessionEntry>,
}

/// Plain snapshot of the per-project observability panel. Owned by the host
/// app and rebuilt from [`crate::projects::ProjectsState`] every frame.
#[derive(Clone, Debug)]
pub struct ProjectsSnapshot {
    pub projects: Vec<ProjectEntry>,
}

/// Callbacks supplied by the host app for the projects panel.
pub struct ProjectsActions {
    /// Reload sessions/per-project status from the backend.
    pub on_refresh: Rc<RefCell<dyn FnMut()>>,
}

/// A session leaf in the environment tree: one run scoped to a project on a
/// medium. `path` is the working directory the turn runs in (and the active
/// pane's cwd is set to it when the session is chosen).
#[derive(Clone, Debug)]
pub struct MediaSession {
    pub id: String,
    pub label: String,
    pub path: String,
}

/// A project branch in the environment tree, holding the sessions that run on
/// that project through the parent medium.
#[derive(Clone, Debug)]
pub struct MediaProject {
    pub id: String,
    pub label: String,
    pub sessions: Vec<MediaSession>,
}

/// A top-level medium node in the environment tree (Local, VM, Remote xrdp,
/// Browser, Container), each expandable to its projects, each project
/// expandable to its sessions. `id` is the [`goble_harness_types::MediumId`]
/// string carried on the turn.
#[derive(Clone, Debug)]
pub struct MediaNode {
    pub kind: MediumKind,
    pub id: String,
    pub label: String,
    pub projects: Vec<MediaProject>,
}

/// Plain snapshot of the environment-tree selector state. Owned by the host
/// app and rebuilt from [`crate::media::MediaState`] every frame.
#[derive(Clone, Debug)]
pub struct MediaSnapshot {
    pub mediums: Vec<MediaNode>,
    /// The id of the currently selected medium.
    pub selected_medium: String,
    /// The id of the currently selected project (under the selected medium).
    pub selected_project: String,
    /// The id of the currently selected session leaf, or empty when the user
    /// has not picked one yet (the turn then runs on the pane's own session).
    pub selected_session: String,
    /// Keys of the tree branches currently expanded (medium / project nodes).
    pub expanded: Vec<String>,
}

/// Callbacks supplied by the host app for the environment selector.
pub struct MediaActions {
    /// Expand or collapse a branch node (medium or project) by its key.
    pub on_toggle: Rc<RefCell<dyn FnMut(String)>>,
    /// Select a session leaf: `(medium_id, project_id, session_id)`. Sets the
    /// active medium+project+session for the next turn.
    pub on_select_session: Rc<RefCell<dyn FnMut(String, String, String)>>,
    /// Select a work environment (medium) by id; resets the project to that
    /// medium's default and clears the session.
    pub on_select_medium: Rc<RefCell<dyn FnMut(String)>>,
    /// Add a custom environment medium by label (the id is derived from the
    /// label). Selects the new medium, persists it so it reappears, and closes
    /// the add-medium dialog.
    pub on_add_medium: Rc<RefCell<dyn FnMut(String)>>,
}

/// A capturable/controllable screen source shown in the screen panel. Read live
/// from the backend's [`goble_screen_core::ScreenRegistry`] (exposed via
/// [`goble_desktop_service::DesktopState::screen_registry`]).
#[derive(Clone, Debug)]
pub struct ScreenSourceEntry {
    /// Source identifier, e.g. `local`.
    pub source: String,
    /// Whether the source has a registered [`goble_screen_core::ScreenCapturer`].
    pub capturable: bool,
    /// Whether the source has a registered [`goble_screen_core::ScreenController`].
    pub controllable: bool,
}

/// Read-only mirror of the held live frame, passed to a [`goble_ui::elements::FrameView`].
#[derive(Clone, Debug)]
pub struct ScreenFrameSnapshot {
    /// Monotonic parity used to detect pixel changes.
    pub frame_seq: u64,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// RGBA8 pixel data (`width * height * 4` bytes).
    pub data: Arc<[u8]>,
}

/// Plain snapshot of the screen panel state. Owned by the host app and rebuilt
/// from [`crate::screen::ScreenState`] every frame.
#[derive(Clone, Debug)]
pub struct ScreenSnapshot {
    /// Whether the screen sheet is open.
    pub open: bool,
    /// Live capture: whether broadcast is streaming the selected source.
    pub broadcast: bool,
    /// Computer-use: whether click/type/scroll input is enabled.
    pub computer_use: bool,
    /// The source the panel is operating on.
    pub selected_source: String,
    /// The capturable/controllable sources from the screen registry.
    pub sources: Vec<ScreenSourceEntry>,
    /// Short status of the last broadcast capture (dimensions or an error).
    pub last_capture: Option<String>,
    /// Whether screen-event recording is active.
    pub recording: bool,
    /// Whether a screen-event replay is running.
    pub replaying: bool,
    /// Whether the replay loops instead of running once.
    pub replay_loop: bool,
    /// Number of recorded events.
    pub recorded_count: usize,
    /// Non-empty status from the last replay step (e.g. an input error).
    pub replay_status: Option<String>,
    /// The held live frame, when broadcast is capturing a source.
    pub frame: Option<ScreenFrameSnapshot>,
}

/// Callbacks supplied by the host app for the screen panel.
pub struct ScreenActions {
    /// Open the screen sheet.
    pub on_open: Rc<RefCell<dyn FnMut()>>,
    /// Close the screen sheet.
    pub on_close: Rc<RefCell<dyn FnMut()>>,
    /// Toggle live capture (broadcast) of the selected source.
    pub on_toggle_broadcast: Rc<RefCell<dyn FnMut(bool)>>,
    /// Toggle computer-use (click/type/scroll) input.
    pub on_toggle_computer_use: Rc<RefCell<dyn FnMut(bool)>>,
    /// Select a source by its id.
    pub on_select_source: Rc<RefCell<dyn FnMut(String)>>,
    /// Reload the capturable/controllable source list from the registry.
    pub on_refresh_sources: Rc<RefCell<dyn FnMut()>>,
    /// Start recording screen observation + input events.
    pub on_record_start: Rc<RefCell<dyn FnMut()>>,
    /// Stop recording (the events remain available for replay).
    pub on_record_stop: Rc<RefCell<dyn FnMut()>>,
    /// Replay the recorded events once, scheduling them by their timing.
    pub on_replay_once: Rc<RefCell<dyn FnMut()>>,
    /// Replay the recorded events in a loop.
    pub on_replay_loop: Rc<RefCell<dyn FnMut()>>,
    /// Stop a running replay.
    pub on_replay_stop: Rc<RefCell<dyn FnMut()>>,
    /// Discard the recorded events.
    pub on_clear_recording: Rc<RefCell<dyn FnMut()>>,
    /// A pointer click at source pixel `(x, y)` on the selected source.
    pub on_screen_click: Rc<RefCell<dyn FnMut(u32, u32)>>,
    /// Typed text forwarded to the selected source's focused field.
    pub on_screen_type: Rc<RefCell<dyn FnMut(String)>>,
    /// A scroll of `(dx, dy)` ticks on the selected source.
    pub on_screen_scroll: Rc<RefCell<dyn FnMut(i32, i32)>>,
}

/// Build the complete app UI: topbar, sidebar, main content, and the
/// auxiliary sheets (crons, connectors, vault) stacked on top.
pub fn build_ui(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
    projects: &ProjectsSnapshot,
    projects_actions: &ProjectsActions,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
    screen: &ScreenSnapshot,
    screen_actions: &ScreenActions,
) -> Box<dyn Element> {
    let topbar = shell::build_topbar(app, state, actions, media, media_actions);
    let sidebar = sidebar::build_sidebar(
        app,
        state,
        actions,
        ai_actions,
        screen_actions,
        &media.selected_medium,
    );
    let main = shell::build_main(
        app,
        state,
        actions,
        projects,
        projects_actions,
        media,
        media_actions,
    );
    let on_drag_start = actions.on_sidebar_drag_start.clone();
    let on_drag_move = actions.on_sidebar_drag_move.clone();
    let on_drag_end = actions.on_sidebar_drag_end.clone();
    let body = shell::SidebarLayout::new(sidebar, main, state.sidebar_width)
        .with_dragging(state.sidebar_dragging)
        .with_on_drag_start(move |x| (on_drag_start.borrow_mut())(x))
        .with_on_drag_move(move |x| (on_drag_move.borrow_mut())(x))
        .with_on_drag_end(move || (on_drag_end.borrow_mut())());

    let mut shell_col = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    shell_col = shell_col.with_child(topbar);
    // Separator below the topbar, like the sidebar separators. The topbar is
    // a fixed height and is not resizable, so this is purely a visual rule.
    shell_col = shell_col.with_child(Divider::horizontal().finish());
    // First-run: a compact getting-started hint on the chat workspace, shown
    // after the workspace choice is made, until dismissed.
    if state.current_tab == AppTab::Chat && state.show_onboarding_tip {
        shell_col = shell_col.with_child(chat::build_onboarding_tip(app, actions));
        shell_col = shell_col.with_child(Divider::horizontal().finish());
    }
    // The body must consume only the *remaining* height below the topbar, so
    // wrap it in `Expanded`; otherwise the chat composer would be pushed past
    // the bottom edge of the window.
    shell_col = shell_col.with_child(Expanded::new(body.finish()).finish());

    let on_close_crons = actions.on_close_crons.clone();
    let crons_sheet = Sheet::new(crons::build_crons_drawer(app, state, actions))
        .with_expanded(state.crons_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_crons.borrow_mut())())
        .finish();

    let on_close_connectors = ai_actions.on_close_connectors.clone();
    let connectors_sheet = Sheet::new(connectors::build_connectors_sheet(app, ai, ai_actions))
        .with_expanded(ai.connectors_open)
        .with_width(CONNECTORS_WIDTH)
        .with_on_close(move || (on_close_connectors.borrow_mut())())
        .finish();

    let on_close_vault = ai_actions.on_close_vault.clone();
    let vault_sheet = Sheet::new(vault::build_vault_sheet(app, ai, ai_actions))
        .with_expanded(ai.vault_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_vault.borrow_mut())())
        .finish();

    let on_close_screen = screen_actions.on_close.clone();
    let screen_sheet = Sheet::new(screen::build_screen_sheet(app, screen, screen_actions))
        .with_expanded(screen.open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_screen.borrow_mut())())
        .finish();

    let mut stack = Stack::new().with_children(vec![
        shell_col.finish(),
        crons_sheet,
        connectors_sheet,
        vault_sheet,
        screen_sheet,
    ]);

    // First-run onboarding overlays: the model-key banner, then (once a key is
    // set) the local/remote workspace choice. Both are centered modal dialogs
    // with an `on_close` so a returning/dismissive user is never left at a dead
    // end: dismissing marks the flow complete (see the actions).
    let on_dismiss_key = actions.on_dismiss_llm_key_banner.clone();
    let on_dismiss_workspace = actions.on_dismiss_workspace_choice.clone();
    if state.show_llm_key_banner {
        stack = stack.with_overlay(
            Dialog::new(chat::build_llm_key_banner(app, actions))
                .with_open(true)
                .with_width(DIALOG_DEFAULT_WIDTH)
                .with_on_close(move || (on_dismiss_key.borrow_mut())())
                .finish(),
            vec2f(0.0, 0.0),
        );
    } else if state.show_workspace_choice {
        stack = stack.with_overlay(
            Dialog::new(chat::build_workspace_choice(app, actions))
                .with_open(true)
                .with_width(DIALOG_DEFAULT_WIDTH)
                .with_on_close(move || (on_dismiss_workspace.borrow_mut())())
                .finish(),
            vec2f(0.0, 0.0),
        );
    }

    // The model-provider dialog floats above everything (including the sheets).
    stack = stack.with_overlay(
        model_form::build_llm_dialog(app, state, actions),
        vec2f(0.0, 0.0),
    );
    // "Add a new environment" dialog, opened from the topbar "+" menu. It only
    // paints/intercepts events while open (see [`super::media::build_add_medium_dialog`]).
    stack = stack.with_overlay(
        self::media::build_add_medium_dialog(app, state, media_actions, actions),
        vec2f(0.0, 0.0),
    );

    // The Cmd+K command palette floats above every other overlay. Each command
    // is a closure that runs the underlying UiAction (or a toggle) and then
    // closes the palette.
    let close_palette = actions.on_close_command_palette.clone();
    let run = |action: &Rc<RefCell<dyn FnMut()>>| -> PaletteCommand {
        let act = action.clone();
        let close = close_palette.clone();
        PaletteCommand::new("", "", move || {
            (act.borrow_mut())();
            (close.borrow_mut())();
        })
    };
    let mut command_list: Vec<PaletteCommand> = vec![
        run(&actions.on_add_space).with_label("New space", "Cmd+Shift+N"),
        run(&actions.on_split_right).with_label("Split right", "Ctrl/Cmd+Space"),
        run(&actions.on_split_down).with_label("Split down", "Cmd+Shift+D"),
        run(&actions.on_new_terminal).with_label("New terminal", "Cmd+Shift+T"),
        run(&actions.on_close_pane).with_label("Close pane", "Ctrl/Cmd+W"),
        run(&actions.on_create_submit).with_label("New conversation", ""),
        run(&actions.on_settings).with_label("Open settings", ""),
        run(&actions.on_projects).with_label("Open projects", ""),
        run(&actions.on_open_crons).with_label("Open scheduled tasks", ""),
        run(&actions.on_toggle_right_sidebar).with_label("Toggle right sidebar", ""),
    ];
    {
        let toggle_dark = actions.on_toggle_dark_mode.clone();
        let dark_mode = state.settings_dark_mode;
        let close = close_palette.clone();
        command_list.push(PaletteCommand::new("Toggle dark mode", "", move || {
            (toggle_dark.borrow_mut())(!dark_mode);
            (close.borrow_mut())();
        }));
    }
    command_list.push(
        run(&screen_actions.on_open).with_label("Open screen", ""),
    );
    command_list.push(
        run(&ai_actions.on_open_connectors).with_label("Open connectors", ""),
    );
    command_list.push(run(&ai_actions.on_open_vault).with_label("Open vault", ""));
    // One "switch to space N" entry per existing space.
    for (i, space) in state.spaces.iter().enumerate() {
        let on_select_space = actions.on_select_space.clone();
        let close = close_palette.clone();
        let label = format!("Switch to {}", space.name);
        command_list.push(PaletteCommand::new(label, "", move || {
            (on_select_space.borrow_mut())(i);
            (close.borrow_mut())();
        }));
    }
    command_list.push(
        run(&screen_actions.on_open).with_label("Open screen", ""),
    );
    command_list.push(
        run(&ai_actions.on_open_connectors).with_label("Open connectors", ""),
    );
    command_list.push(run(&ai_actions.on_open_vault).with_label("Open vault", ""));

    stack = stack.with_overlay(
        palette::build_command_palette(
            app,
            state.command_palette_open,
            &state.command_palette_query,
            state.command_palette_index,
            command_list,
            actions.on_command_palette_change.clone(),
            actions.on_command_palette_move.clone(),
            actions.on_close_command_palette.clone(),
        ),
        vec2f(0.0, 0.0),
    );

    Container::new(stack.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}

#[cfg(test)]
mod pane_tests {
    use super::*;

    fn chat_leaf(id: u64) -> Pane {
        Pane::Leaf { id, kind: PaneKind::Chat }
    }

    #[test]
    fn split_adds_second_pane_and_returns_its_id() {
        let mut space = Space::new("S", chat_leaf(1));
        let mut next = 2;
        let new_id = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
        assert_eq!(new_id, 3); // split node = 2, new leaf = 3
        assert_eq!(space.root.id(), 2);
        assert_eq!(space.root.max_id(), 3);
        assert_eq!(space.root.first_leaf_id(), 1);
    }

    #[test]
    fn close_replaces_split_with_sibling() {
        let mut space = Space::new("S", chat_leaf(1));
        let mut next = 2;
        space.split(1, SplitDir::Vertical, &mut next).unwrap();
        // Close the new leaf (3); the split collapses to the surviving leaf (1).
        let focus = space.close(3).unwrap();
        assert_eq!(focus, 1);
        assert!(matches!(space.root, Pane::Leaf { id: 1, .. }));
    }

    #[test]
    fn set_ratio_updates_the_split_node() {
        let mut space = Space::new("S", chat_leaf(1));
        let mut next = 2;
        space.split(1, SplitDir::Horizontal, &mut next).unwrap();
        let split_id = space.root.id();
        assert!(space.set_ratio(split_id, 0.25));
        match &space.root {
            Pane::Split { ratio, .. } => assert!((*ratio - 0.25).abs() < 1e-6),
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn set_ratio_clamps_to_the_ratio_floor() {
        let mut space = Space::new("S", chat_leaf(1));
        let mut next = 2;
        space.split(1, SplitDir::Horizontal, &mut next).unwrap();
        let split_id = space.root.id();
        // Out-of-range ratios are clamped into [0.05, 0.95] (the coarse ratio
        // floor); the per-pixel minimum is enforced by the split widget, which
        // knows the actual size.
        assert!(space.set_ratio(split_id, 0.0));
        match &space.root {
            Pane::Split { ratio, .. } => assert!((*ratio - 0.05).abs() < 1e-6),
            _ => panic!("expected split"),
        }
        assert!(space.set_ratio(split_id, 1.0));
        match &space.root {
            Pane::Split { ratio, .. } => assert!((*ratio - 0.95).abs() < 1e-6),
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn split_creates_two_distinct_chat_leaves() {
        let mut space = Space::new("S", chat_leaf(1));
        let mut next = 2;
        let new_id = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
        // The two leaves have distinct ids, so each can host an independent
        // session (per-pane conversation transcript) in app state.
        assert_ne!(space.root.first_leaf_id(), new_id);
        match &space.root {
            Pane::Split { first, second, .. } => {
                assert!(matches!(**first, Pane::Leaf { kind: PaneKind::Chat, .. }));
                assert!(matches!(**second, Pane::Leaf { kind: PaneKind::Chat, .. }));
            }
            _ => panic!("expected a split"),
        }
    }

    #[test]
    fn navigate_right_moves_between_horizontal_leaves() {
        let mut space = Space::new("S", chat_leaf(1));
        let mut next = 2;
        let right = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
        assert_eq!(space.navigate(1, NavDir::Right), Some(right));
        assert_eq!(space.navigate(right, NavDir::Left), Some(1));
    }

    #[test]
    fn navigate_down_moves_between_vertical_leaves() {
        let mut space = Space::new("S", chat_leaf(1));
        let mut next = 2;
        let down = space.split(1, SplitDir::Vertical, &mut next).unwrap();
        assert_eq!(space.navigate(1, NavDir::Down), Some(down));
        assert_eq!(space.navigate(down, NavDir::Up), Some(1));
    }

    #[test]
    fn navigate_edge_returns_none_for_out_of_bounds() {
        let space = Space::new("S", chat_leaf(1));
        // A single pane has no neighbor in any direction.
        assert_eq!(space.navigate(1, NavDir::Left), None);
        assert_eq!(space.navigate(1, NavDir::Right), None);
        assert_eq!(space.navigate(1, NavDir::Up), None);
        assert_eq!(space.navigate(1, NavDir::Down), None);
    }

    #[test]
    fn nested_split_navigates_to_nearest_sibling() {
        // Space 1 split right -> [A | B]. Split B downward: [A | (B / C)].
        // From C, Up lands on B; from B, Down lands on C; from A, Right lands
        // on the nearest leaf to the right (B).
        let mut space = Space::new("S", chat_leaf(1));
        let mut next = 2;
        let b = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
        let c = space.split(b, SplitDir::Vertical, &mut next).unwrap();
        assert_eq!(space.navigate(c, NavDir::Up), Some(b));
        assert_eq!(space.navigate(b, NavDir::Down), Some(c));
        assert_eq!(space.navigate(1, NavDir::Right), Some(b));
        // From C, left goes to A (the only leaf to the left).
        assert_eq!(space.navigate(c, NavDir::Left), Some(1));
    }
}
