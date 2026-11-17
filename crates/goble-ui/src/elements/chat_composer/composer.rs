use std::cell::RefCell;
use std::rc::Rc;

use super::proposal::CommandProposalUi;
use crate::elements::{
    harness_bar_mark, AppContext, ApprovalMode, Border, Clipped, ComposerButton,
    ComposerModelTray, ConstrainedBox, Container, ContextPill, CrossAxisAlignment, DiffSummary,
    EdgeInsets, Element, Expanded, Fill, Flex, Icon, LayoutContext, MainAxisSize, ModelTrayKey,
    Padding, PaintContext, PanelScroll, PillTraySide, Point, PopupMenu, PopupMenuItem,
    PopupMenuPosition, ShortcutHint, ShortcutHints, SizeConstraint, SlashMenuItem, Spacer, Text,
    TextArea, Tooltip, TooltipPosition, Wrap, COMPOSER_CONTROL_RADIUS, CONTEXT_PILL_HEIGHT,
    CONTEXT_PILL_LABEL_SIZE, MENU_MAX_VISIBLE_ROWS,
};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::{vec2f, PointF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};
use crate::vim::{Clipboard, VimMode, VimState};
use goble_core::harness::CommandDecision;
use goble_core::ssh_command::SshSession;

/// Cap for the working-directory pill's label. A long path ellipsizes inside the
/// pill instead of pushing the other context pills across the row.
const DIR_PILL_MAX_WIDTH: f32 = 280.0;

/// The same cap for the session chip: a long `user@host (address)` narrows
/// inside its own box instead of pushing the context row out of the pane.
const SESSION_CHIP_MAX_WIDTH: f32 = 280.0;

/// What the model control reads when the pane has no model configured: the
/// turn would run on nothing, so naming a model there would be a claim nobody
/// made. The host's own resolved label says the same thing (its model value is
/// empty until a model is configured), and this covers the label it passes.
pub const MODEL_NOT_CONFIGURED: &str = "not configured";

/// The rich input's controls share one label size and one side inset, so every
/// button in the band and footer — the model, the attach and stop controls, the
/// directory, environment and branch pills — reads as part of one compact
/// strip. The height itself is [`CONTEXT_PILL_HEIGHT`], the pill's own.
const CONTROL_TEXT_SIZE: f32 = CONTEXT_PILL_LABEL_SIZE;
const CONTROL_H_PAD: f32 = 8.0;

/// The composer card's corner radius. The card is the rounded panel every
/// surface draws — agent chat, shell bar, thread view, custom harness. The
/// reference photo's arc is far rounder than a conventional card: measured at
/// 26-30 logical, and the input segment is concentric with it.
pub const COMPOSER_CARD_RADIUS: f32 = 26.0;
/// The input segment's radius: concentric with the card one inset in (card 26
/// minus the 6pt casing inset), so the two curves read as one rounded panel
/// holding a field.
pub const COMPOSER_INPUT_RADIUS: f32 = 20.0;
/// How far the input segment is inset from the card's sides and bottom.
pub const COMPOSER_CARD_INSET: f32 = 6.0;
/// The card's horizontal gutter from the surface it is drawn on, and the margin
/// it keeps under itself. Every surface draws the same card, so the shell bar
/// and the agent input sit the same distance from the pane's edges.
pub const COMPOSER_CARD_GUTTER: f32 = 12.0;
/// The context band's resting height: 8 above the controls, the controls' own
/// [`CONTEXT_PILL_HEIGHT`], and 8 below them before the input segment.
pub const CONTEXT_SEGMENT_HEIGHT: f32 = 40.0;
/// The footer row's height. The row reserves it so the card's layout is the
/// same whether or not a control sits at its right end: the send circle was
/// removed, and only the stop circle (and the optional trailing action) use the
/// row now.
pub const COMPOSER_FOOTER_HEIGHT: f32 = 32.0;
/// The diameter of the stop circle. It used to size the send circle too; the
/// owner removed the send button, and the stop control keeps the round slot.
pub const COMPOSER_SEND_SIZE: f32 = 32.0;
/// The gap between the editor and the footer row under it.
pub const COMPOSER_FOOTER_GAP: f32 = 10.0;
/// The editor's resting height: one 14pt line (line height 1.4) and the room
/// the field keeps around it.
pub const COMPOSER_EDITOR_MIN_HEIGHT: f32 = 24.0;
/// The tallest the editor grows with a draft before it scrolls.
pub const COMPOSER_EDITOR_MAX_HEIGHT: f32 = 160.0;
/// The input segment's height at rest, with one line of draft and no
/// attachments or hints: 14 above + the editor + the footer gap + the footer +
/// 8 below. (The card and the segment are filled, not stroked, so no outline
/// adds to the height.) A host that reserves the bar's height uses this.
pub const COMPOSER_INPUT_MIN_HEIGHT: f32 =
    14.0 + COMPOSER_EDITOR_MIN_HEIGHT + COMPOSER_FOOTER_GAP + COMPOSER_FOOTER_HEIGHT + 8.0;
/// The instruction strip's own height when a host passes hints inside the
/// input: the capped key's box, whose 11pt label is a 13.2pt line (11 x 1.2).
/// A host that reserves the bar's height adds this row and the footer gap above
/// it, exactly as the element lays them out.
pub const COMPOSER_HINTS_ROW_HEIGHT: f32 = 13.2;
/// The height a host reserves for this card when it draws a context band and one
/// instruction row: the shell pane tells its pty this, so the rows the program
/// paints are the rows the user can see. It must equal the height the element
/// lays out for that configuration; the composer test
/// `the_reserved_bar_height_is_the_cards_own_height` pins the two together.
pub const COMPOSER_BAR_RESERVED_HEIGHT: f32 = COMPOSER_CARD_GUTTER
    + COMPOSER_CARD_INSET
    + CONTEXT_SEGMENT_HEIGHT
    + COMPOSER_INPUT_MIN_HEIGHT
    + COMPOSER_FOOTER_GAP
    + COMPOSER_HINTS_ROW_HEIGHT;
/// The environment menu is wider than the other dropdowns: its rows carry a
/// description line under the label ("Clones this repo in a cloud
/// environment…"), and the reference shows it cut with an ellipsis on one line
/// rather than wrapped inside the row.
const ENVIRONMENT_MENU_WIDTH: f32 = 308.0;

/// The branch menu is nearly as wide as the environment's: the reference's
/// branch tray is a long casing too, and a branch name is a deeper path than a
/// model or a medium label.
const BRANCH_MENU_WIDTH: f32 = 292.0;

pub struct ChatComposer {
    pub(super) value: Rc<RefCell<String>>,
    /// Where the editor's insertion beam sits (a character index into `value`).
    /// App-owned, like the draft itself: the composer element is rebuilt every
    /// frame, so an index kept inside it would reset on every one of them.
    caret: Rc<RefCell<usize>>,
    placeholder: String,
    attachments: Vec<String>,
    model_label: Option<String>,
    /// The CLI harness the draft goes to, when this pane runs a custom harness:
    /// the footer draws its mark in place of the model and approval controls,
    /// which are the native agent's own. `None` on the native surface.
    agent_harness: Option<&'static str>,
    pub(super) path_label: Option<String>,
    /// The SSH session this pane's shell is bound to, when a submitted `ssh`
    /// line put it on a host (S1). While it is set the context row draws a chip
    /// naming the host; a local shell has none, so the chip's absence is itself
    /// the statement that the session is local.
    ssh_session: Option<SshSession>,
    pub(super) focused: bool,
    /// Whether the input is waiting on something outside itself — a command
    /// approval, whose card owns the keyboard until the user decides. A
    /// disabled input draws its waiting placeholder and takes no keys or
    /// presses, so typing cannot land in a draft the turn will not read.
    disabled: bool,
    /// Whether a command approval is holding the turn. The input stays
    /// writable — a follow-up can be queued while the decision waits — but its
    /// placeholder says what the turn is waiting for, and Enter keeps deciding
    /// the proposal until the user types or a prompt is already queued.
    waiting: bool,
    /// Whether a prompt is already queued for this turn. A second Enter then
    /// means "send now": Enter must fall through to the submit path instead of
    /// approving the waiting command.
    pub(super) queued_prompt_present: bool,
    /// Whether a press outside the editor blurs it. Off for a host that makes
    /// this composer the only typing surface of its pane (the terminal pane):
    /// clicking the output above the bar must leave the caret where it was.
    blur_on_outside_click: bool,
    /// The instructions drawn in the rich input's own bottom row, under the
    /// editor and the action row — the gestures this surface answers that the
    /// host keeps inside the input. An empty list draws nothing.
    ///
    /// The strip a pane shows over its input is the host's, drawn above the
    /// separator: what belongs *in* the input is the one entry the host marks
    /// as its own, which is the shell bar's `⌘↵ new conversation`.
    hints: Vec<ShortcutHint>,
    /// The slash-command menu: the commands the draft can run, drawn above the
    /// editor while the draft starts with `/`. `slash_enabled` records that the
    /// host offers one at all, so a query that matches nothing still shows the
    /// menu (with its own empty state) instead of silently closing it.
    slash_enabled: bool,
    slash_items: Vec<SlashMenuItem>,
    slash_index: Rc<RefCell<usize>>,
    /// Whether Escape has put the menu away for the draft as it stands. Shared
    /// with the host so the dismissal survives the per-frame rebuild.
    slash_dismissed: Rc<RefCell<bool>>,
    on_slash_move: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_slash_accept: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_slash_dismiss: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    stop_visible: bool,
    /// warp-new style context pills: the agent harness, the working directory
    /// and the git branch, each with its own dropdown selector. The working
    /// directory reuses `path_label` (set via `with_path_label`) as its label.
    harness_label: Option<String>,
    /// The glyph beside the environment label: T3 Code's computer for a local
    /// environment and its cloud for a worker. The host names it, so the pill
    /// says where the work runs as well as what it is called; the default is
    /// the local environment's mark.
    harness_icon: &'static str,
    harness_menu_items: Vec<PopupMenuItem>,
    harness_menu_open: Rc<RefCell<bool>>,
    on_select_harness_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    dir_menu_items: Vec<PopupMenuItem>,
    dir_menu_open: Rc<RefCell<bool>>,
    /// The directory tray's keyboard highlight, app-owned like its open flag so
    /// the row Up/Down moved to survives the per-frame rebuild.
    dir_menu_highlight: Rc<RefCell<Option<usize>>>,
    /// The directory tray's scroll offset, owned by the app for the same reason
    /// its open flag is: the tray is capped at [`MENU_MAX_VISIBLE_ROWS`] rows, so
    /// a deep directory scrolls, and the per-frame rebuild would otherwise put
    /// it back at the top.
    dir_menu_scroll: PanelScroll,
    on_select_dir_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    branch_label: Option<String>,
    branch_menu_items: Vec<PopupMenuItem>,
    branch_menu_open: Rc<RefCell<bool>>,
    /// The branch tray's keyboard highlight, app-owned like its open flag.
    branch_menu_highlight: Rc<RefCell<Option<usize>>>,
    on_select_branch_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    /// The command the harness is waiting on an approval for (A6). The approval
    /// card itself is drawn by the transcript at the end of the agent's output;
    /// the composer keeps the proposal so its keys answer the decision.
    pub(super) proposal: Option<CommandProposalUi>,
    /// Which candidate is selected. App-owned, like the footer menus' `open`
    /// flags, because the composer element is rebuilt every frame.
    pub(super) proposal_selected: Rc<RefCell<usize>>,
    pub(super) on_decision: Option<Rc<RefCell<dyn FnMut(String, CommandDecision) + 'static>>>,
    /// The pane's approval mode, when the host draws its control here. `None`
    /// on a surface that has no agent turn to police (the shell bar).
    approval_mode: Option<ApprovalMode>,
    /// The mode menu's open flag and the callback a picked or cycled mode goes
    /// through. Host-owned, like the model menu's, so both survive the rebuild.
    approval_menu_open: Rc<RefCell<bool>>,
    /// The mode menu's keyboard highlight, app-owned like its open flag.
    approval_menu_highlight: Rc<RefCell<Option<usize>>>,
    on_select_approval_mode: Option<Rc<RefCell<dyn FnMut(ApprovalMode) + 'static>>>,
    pub(super) on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    /// Cmd/Ctrl+Enter submit: start a NEW agent conversation (warp-new). The
    /// host decides what "new conversation" means; the composer only reports
    /// the keybinding.
    on_cmd_enter: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    /// Cmd/Ctrl+Alt+Enter submit: hand the draft to the cloud agent
    /// (warp-new's `⌘⌥⏎`). The host owns what "cloud" means — the routing
    /// choice and the transport — so the composer only reports the chord.
    on_send_to_cloud: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_attach: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_model: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_key: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_variant: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_voice: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_image: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_code: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_link: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_stop: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    /// The documented trailing seat on the footer row, where an optional host
    /// control (the live-speech button the owner will enable from Settings) can
    /// sit. The rich input draws no send control, so `None` leaves the row's
    /// right end empty and nothing replaces the removed circle. A factory, not
    /// an element: the composer rebuilds its tree every frame, so the seat has
    /// to produce a fresh element for each one.
    footer_action: Option<Rc<RefCell<dyn FnMut() -> Box<dyn Element> + 'static>>>,
    model_menu_items: Vec<PopupMenuItem>,
    model_menu_open: Rc<RefCell<bool>>,
    on_select_model_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    /// The column tray the model control opens, when the host wired one: the
    /// harnesses a pane can run on, the models inside each and their thinking
    /// levels. The band itself is the host's, drawn in the slot over the input;
    /// the control toggles its open flag and the composer routes its keys.
    model_tray: Option<ComposerModelTray>,
    /// The row the model band has chosen, host-owned like the open flag: the
    /// composer is rebuilt every frame, so an index kept inside it would reset
    /// on every one of them. Shared with the model menu when the host wires
    /// one, so the highlight survives between frames.
    model_index: Rc<RefCell<usize>>,
    /// Modal (vim) editing for the editor, when the host turned it on. The
    /// state lives with the host (the element is rebuilt every frame) and the
    /// footer draws the mode badge from it.
    vim: Option<Rc<RefCell<VimState>>>,
    /// The system clipboard the engine's `"+`/`"*` registers reach, and the
    /// one a copy, a cut and a paste use.
    clipboard: Option<Rc<RefCell<dyn Clipboard>>>,
    /// Where the editor's selection began, when the host shares it: a copy and
    /// a cut take what is between the anchor and the beam, and a selection has
    /// to survive the per-frame rebuild like the beam does.
    anchor: Option<Rc<RefCell<Option<usize>>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            value: Rc::new(RefCell::new(String::new())),
            caret: Rc::new(RefCell::new(0)),
            placeholder: String::from("Ask anything..."),
            attachments: Vec::new(),
            model_label: None,
            agent_harness: None,
            path_label: None,
            ssh_session: None,
            focused: false,
            disabled: false,
            waiting: false,
            queued_prompt_present: false,
            blur_on_outside_click: true,
            hints: Vec::new(),
            slash_enabled: false,
            slash_items: Vec::new(),
            slash_index: Rc::new(RefCell::new(0)),
            slash_dismissed: Rc::new(RefCell::new(false)),
            on_slash_move: None,
            on_slash_accept: None,
            on_slash_dismiss: None,
            stop_visible: false,
            on_change: None,
            on_send: None,
            on_cmd_enter: None,
            on_send_to_cloud: None,
            on_attach: None,
            on_select_model: None,
            on_select_key: None,
            on_select_variant: None,
            on_voice: None,
            on_image: None,
            on_code: None,
            on_link: None,
            on_stop: None,
            on_focus_change: None,
            footer_action: None,
            model_menu_items: Vec::new(),
            model_menu_open: Rc::new(RefCell::new(false)),
            on_select_model_item: None,
            model_tray: None,
            model_index: Rc::new(RefCell::new(0)),
            vim: None,
            clipboard: None,
            anchor: None,
            harness_label: None,
            harness_icon: "environment-computer",
            harness_menu_items: Vec::new(),
            harness_menu_open: Rc::new(RefCell::new(false)),
            on_select_harness_item: None,
            dir_menu_items: Vec::new(),
            dir_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_highlight: Rc::new(RefCell::new(None)),
            dir_menu_scroll: PanelScroll::default(),
            on_select_dir_item: None,
            branch_label: None,
            branch_menu_items: Vec::new(),
            branch_menu_open: Rc::new(RefCell::new(false)),
            branch_menu_highlight: Rc::new(RefCell::new(None)),
            on_select_branch_item: None,
            proposal: None,
            proposal_selected: Rc::new(RefCell::new(0)),
            on_decision: None,
            approval_mode: None,
            approval_menu_open: Rc::new(RefCell::new(false)),
            approval_menu_highlight: Rc::new(RefCell::new(None)),
            on_select_approval_mode: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_value(self, value: impl Into<String>) -> Self {
        *self.value.borrow_mut() = value.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_attachments(mut self, attachments: Vec<String>) -> Self {
        self.attachments = attachments;
        self
    }

    /// Name the CLI harness this pane's draft goes to. A custom harness owns
    /// its own model and approval policy, so its mark is the footer's only
    /// agent control; the model and approval switchers are the native agent's
    /// and stay off that surface.
    pub fn with_agent_harness(mut self, harness: &'static str) -> Self {
        self.agent_harness = Some(harness);
        self
    }

    pub fn with_model_label(mut self, label: impl Into<String>) -> Self {
        self.model_label = Some(label.into());
        self
    }

    pub fn with_path_label(mut self, path: impl Into<String>) -> Self {
        self.path_label = Some(path.into());
        self
    }

    /// The SSH session the pane's shell is bound to, or `None` for a local
    /// shell. It is the bound session itself, not a label: the chip's words are
    /// the session's own ([`SshSession::chip_label`]). A local session draws no
    /// chip at all.
    pub fn with_ssh_session(mut self, session: Option<SshSession>) -> Self {
        self.ssh_session = session;
        self
    }

    /// Label for the environment (medium) context pill: the environment the
    /// draft's turn will run on. The host draws it only where the environment is
    /// the user's to choose — a conversation running on a worker — so a host
    /// that passes no label draws no pill.
    pub fn with_harness_label(mut self, label: impl Into<String>) -> Self {
        self.harness_label = Some(label.into());
        self
    }

    /// The glyph that names the environment beside its label (see
    /// [`Self::with_harness_label`]): T3 Code's computer (`environment-computer`)
    /// for a local environment and its cloud (`environment-cloud`) for a worker.
    /// The host knows which; the composer only draws it.
    pub fn with_harness_icon(mut self, icon: &'static str) -> Self {
        self.harness_icon = icon;
        self
    }

    /// Set the environment (medium) dropdown: items, the app-owned `open` flag,
    /// and a select callback (same contract as `with_model_menu`).
    pub fn with_harness_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.harness_menu_items = items;
        self.harness_menu_open = open;
        self.on_select_harness_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the working-directory dropdown (items, open flag, select callback).
    pub fn with_dir_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.dir_menu_items = items;
        self.dir_menu_open = open;
        self.on_select_dir_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Share the directory tray's scroll offset with the app, so a tray taller
    /// than its cap (`MENU_MAX_VISIBLE_ROWS` rows) keeps the position the wheel
    /// left it at across the per-frame rebuild.
    pub fn with_dir_menu_scroll(mut self, scroll: PanelScroll) -> Self {
        self.dir_menu_scroll = scroll;
        self
    }

    /// Share the directory tray's keyboard highlight with the app, so the row
    /// Up/Down moved to survives the per-frame rebuild.
    pub fn with_dir_menu_highlight(mut self, highlight: Rc<RefCell<Option<usize>>>) -> Self {
        self.dir_menu_highlight = highlight;
        self
    }

    /// Label for the fork/branch context pill.
    pub fn with_branch_label(mut self, label: impl Into<String>) -> Self {
        self.branch_label = Some(label.into());
        self
    }

    /// Set the branch dropdown (items, open flag, select callback).
    pub fn with_branch_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.branch_menu_items = items;
        self.branch_menu_open = open;
        self.on_select_branch_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Share the branch tray's keyboard highlight with the app, like the
    /// directory tray's.
    pub fn with_branch_menu_highlight(mut self, highlight: Rc<RefCell<Option<usize>>>) -> Self {
        self.branch_menu_highlight = highlight;
        self
    }

    /// Kept only so the hosts that still pass the working tree's changed-file
    /// summary (the chat view and the desktop app) keep compiling. The rich
    /// input no longer draws a diff control: the owner moved "Open changes" to
    /// the app topbar, so the summary is accepted and ignored here. Remove this
    /// shim once those call sites stop passing it.
    #[doc(hidden)]
    pub fn with_diff_menu(self, _summary: DiffSummary, _open: Rc<RefCell<bool>>) -> Self {
        self
    }

    /// Kept for the same call sites as [`Self::with_diff_menu`]: there is no
    /// diff menu left to scroll, so the offset is accepted and ignored.
    #[doc(hidden)]
    pub fn with_diff_menu_scroll(self, _scroll: PanelScroll) -> Self {
        self
    }

    /// Kept for the same call sites as [`Self::with_diff_menu`]: no diff menu
    /// row is drawn, so the review-panel callback is accepted and ignored.
    #[doc(hidden)]
    pub fn with_diff_open<F: FnMut(Option<String>) + 'static>(self, _callback: F) -> Self {
        self
    }

    /// Show the slash-command menu above the editor while the draft starts
    /// with `/`. `items` are the commands the host offers for the current
    /// draft, `index` is the chosen row (host-owned, like every other menu's
    /// open flag), `dismissed` is the host's Escape flag, and the callbacks are
    /// the host's: move the selection, run it, put the menu away.
    pub fn with_slash_menu(
        mut self,
        items: Vec<SlashMenuItem>,
        index: Rc<RefCell<usize>>,
        dismissed: Rc<RefCell<bool>>,
    ) -> Self {
        self.slash_enabled = true;
        self.slash_items = items;
        self.slash_index = index;
        self.slash_dismissed = dismissed;
        self
    }

    /// Fired when the selection moves (arrow keys, or the pointer onto a row).
    pub fn with_on_slash_move<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_slash_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when a row is run: Enter, Tab, or a click on it.
    pub fn with_on_slash_accept<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_slash_accept = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired on Escape: the menu closes and the draft stays as typed.
    pub fn with_on_slash_dismiss<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_slash_dismiss = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether the draft's own command list is the band the input shows: the
    /// draft is a command and Escape has not put its list away. Both bands are
    /// the host's to draw in that one slot, and both the host and this composer
    /// read this predicate, so the two can never be up together (see `rebuild`).
    fn slash_band_open(&self) -> bool {
        self.slash_enabled
            && crate::elements::slash_menu_open(
                &self.value.borrow(),
                *self.slash_dismissed.borrow(),
            )
    }

    /// The keys that pick from the models, taken before the editor sees them:
    /// Up/Down move the selection, Enter or Tab runs it, Escape puts the list
    /// away. The list itself is the host's, drawn in the slot the command list
    /// takes over the input; what the composer keeps of it is the key routing,
    /// because the editor owns the keyboard.
    ///
    /// The command list takes the keys while it is up: the two bands share one
    /// slot over the input, and the draft's own list is the one that holds it.
    fn handle_model_key(&mut self, event: &DispatchedEvent) -> bool {
        // Only the composer whose pane is active holds the keyboard: a band
        // left open in a background pane must not take the keys of the pane the
        // user is typing in.
        if !self.focused || !*self.model_menu_open.borrow()
            || self.on_select_model_item.is_none()
            || self.slash_band_open()
        {
            return false;
        }
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        match key.as_str() {
            "ArrowDown" | "ArrowUp" => {
                let len = self.model_menu_items.len();
                if len == 0 {
                    return true;
                }
                let selected = (*self.model_index.borrow()).min(len - 1);
                let target = if key == "ArrowDown" {
                    (selected + 1).min(len - 1)
                } else {
                    selected.saturating_sub(1)
                };
                *self.model_index.borrow_mut() = target;
                true
            }
            // Cmd/Ctrl+Enter is the host's own gesture (a new conversation), so
            // it falls through to the editor even with the band up.
            "Enter" | "Tab" if !modifiers.command && !modifiers.ctrl && !modifiers.shift => {
                let len = self.model_menu_items.len();
                let selected = if len == 0 {
                    0
                } else {
                    (*self.model_index.borrow()).min(len - 1)
                };
                let Some(cb) = self.on_select_model_item.clone() else {
                    return false;
                };
                *self.model_menu_open.borrow_mut() = false;
                (cb.borrow_mut())(selected);
                true
            }
            "Escape" => {
                *self.model_menu_open.borrow_mut() = false;
                true
            }
            _ => false,
        }
    }

    /// The keys the DSH model card takes while it is up: Up/Down move the
    /// highlight (root cells, model rows, effort rows), Enter/Tab drill or take
    /// a row, Escape/Shift+Tab leave the drilled pane first and otherwise close.
    /// The editor keeps the caret — the composer routes the keys, next to the
    /// chip that opens the card.
    ///
    /// The command list wins the slot while the draft is a command, exactly as
    /// it does over the flat model list.
    fn handle_model_tray_key(&mut self, event: &DispatchedEvent) -> bool {
        let Some(tray) = self.model_tray.clone() else {
            return false;
        };
        // As with the flat list: a background pane's card never takes the
        // active pane's keys.
        if !self.focused || !*tray.open.borrow() || self.slash_band_open() {
            return false;
        }
        let model = tray.model.clone();
        let level = tray.level.clone();
        match crate::elements::handle_tray_key(
            event,
            tray.harnesses.as_slice(),
            &model,
            &level,
            &tray.state,
        ) {
            ModelTrayKey::Ignored => false,
            ModelTrayKey::Moved
            | ModelTrayKey::DrillModel
            | ModelTrayKey::DrillEffort
            | ModelTrayKey::Back => true,
            ModelTrayKey::AcceptModel(row) => {
                *tray.open.borrow_mut() = false;
                if let Some(cb) = tray.on_select_model.clone() {
                    (cb.borrow_mut())(row);
                }
                true
            }
            ModelTrayKey::AcceptLevel(level) => {
                *tray.open.borrow_mut() = false;
                if !level.is_empty() {
                    if let Some(cb) = tray.on_select_level.clone() {
                        (cb.borrow_mut())(level);
                    }
                }
                true
            }
            ModelTrayKey::Close => {
                *tray.open.borrow_mut() = false;
                true
            }
        }
    }

    /// The keys that pick from the slash menu, taken before the editor sees
    /// them: Up/Down move the selection, Enter or Tab runs it, Escape puts the
    /// menu away. Every other key belongs to the editor, because what it types
    /// is the menu's query — which is why the menu is not a modal.
    fn handle_slash_key(&mut self, event: &DispatchedEvent) -> bool {
        // The draft's command list answers the keys of the pane that owns the
        // caret, not of a background pane whose draft happens to be a command.
        if !self.focused {
            return false;
        }
        if !self.slash_enabled
            || !crate::elements::slash_menu_open(
                &self.value.borrow(),
                *self.slash_dismissed.borrow(),
            )
        {
            return false;
        }
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        match key.as_str() {
            "ArrowDown" | "ArrowUp" => {
                let len = self.slash_items.len();
                if len == 0 {
                    return true;
                }
                let selected = (*self.slash_index.borrow()).min(len - 1);
                let target = if key == "ArrowDown" {
                    (selected + 1).min(len - 1)
                } else {
                    selected.saturating_sub(1)
                };
                *self.slash_index.borrow_mut() = target;
                if let Some(cb) = self.on_slash_move.clone() {
                    (cb.borrow_mut())(target);
                }
                true
            }
            // Cmd/Ctrl+Enter is the host's own gesture (a new conversation),
            // so it falls through to the editor even with the menu up.
            "Enter" | "Tab" if !modifiers.command && !modifiers.ctrl && !modifiers.shift => {
                let len = self.slash_items.len();
                let selected = if len == 0 {
                    0
                } else {
                    (*self.slash_index.borrow()).min(len - 1)
                };
                if let Some(cb) = self.on_slash_accept.clone() {
                    (cb.borrow_mut())(selected);
                }
                true
            }
            "Escape" => {
                if let Some(cb) = self.on_slash_dismiss.clone() {
                    (cb.borrow_mut())();
                }
                true
            }
            _ => false,
        }
    }

    /// Cmd/Ctrl+Alt+Enter submits the draft to the cloud agent (warp-new's
    /// `⌘⌥⏎`). Taken before the editor sees it, like the menu's keys: the chord
    /// belongs to the host, which owns the routing the cloud means. Shift is
    /// not part of the gesture, and a host that wired no callback keeps the key.
    fn handle_send_to_cloud_key(&mut self, event: &DispatchedEvent) -> bool {
        // The chord belongs to the pane that holds the caret: a background
        // composer must not route its stale draft to the cloud under the keys
        // meant for the active pane.
        if !self.focused {
            return false;
        }
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        if key != "Enter"
            || modifiers.shift
            || !modifiers.alt
            || !(modifiers.command || modifiers.ctrl)
        {
            return false;
        }
        let Some(cb) = self.on_send_to_cloud.clone() else {
            return false;
        };
        let text = self.value.borrow().clone();
        (cb.borrow_mut())(text);
        *self.value.borrow_mut() = String::new();
        true
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Wait on a decision taken outside the input: the editor is not focused,
    /// its placeholder says what the bar is waiting for, and the input takes no
    /// keys or presses. A command approval is the only caller — its card owns
    /// the keyboard until the user decides.
    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Mark the turn as waiting on a command decision. The input keeps its
    /// keys (a follow-up may be queued while the card waits), but its
    /// placeholder names the decision the turn is suspended on.
    pub fn with_waiting(mut self, waiting: bool) -> Self {
        self.waiting = waiting;
        self
    }

    /// Tell the proposal key path that a prompt is already queued: Enter then
    /// belongs to the editor's submit (the "send now" gesture), not to the
    /// approval.
    pub fn with_queued_prompt_present(mut self, queued: bool) -> Self {
        self.queued_prompt_present = queued;
        self
    }

    /// Whether a press outside the editor blurs it (the default). A host whose
    /// pane types only here turns it off, so a click on what the pane shows
    /// above the bar does not take the keyboard away from the editor.
    pub fn with_blur_on_outside_click(mut self, blur: bool) -> Self {
        self.blur_on_outside_click = blur;
        self
    }

    /// Kept for hosts that still pass it: the unified composer card always
    /// draws its context band over the input segment, so the flag changes
    /// nothing. The terminal bar and the agent input are the same card.
    pub fn with_context_above_editor(self, _above: bool) -> Self {
        self
    }

    /// Kept for hosts that still pass it: the unified composer card always
    /// keeps the same gutter from the surface it is drawn on, so the flag
    /// changes nothing.
    pub fn with_edge_to_edge(self, _edge_to_edge: bool) -> Self {
        self
    }

    /// The instructions to draw in the input's own bottom row, under the editor
    /// and the action row. The strip that belongs over the input is the host's;
    /// this is the one entry the host keeps inside the input.
    pub fn with_hints(mut self, hints: Vec<ShortcutHint>) -> Self {
        self.hints = hints;
        self
    }

    /// Share the editor's insertion point with the host, so the beam survives
    /// the per-frame rebuild.
    pub fn with_caret(mut self, caret: Rc<RefCell<usize>>) -> Self {
        self.caret = caret;
        self
    }

    /// Turn on modal (vim) editing for the editor. The mode badge in the
    /// composer's footer reads the same state, so the surface says which mode
    /// the next key belongs to.
    pub fn with_vim(mut self, vim: Rc<RefCell<VimState>>) -> Self {
        self.vim = Some(vim);
        self
    }

    /// The system clipboard the `"+`/`"*` registers read and write, and the one
    /// a copy, a cut and a paste use.
    pub fn with_clipboard(mut self, clipboard: Rc<RefCell<dyn Clipboard>>) -> Self {
        self.clipboard = Some(clipboard);
        self
    }

    /// Share the editor's selection anchor with the host, so a shift-selection
    /// survives the per-frame rebuild.
    pub fn with_anchor(mut self, anchor: Rc<RefCell<Option<usize>>>) -> Self {
        self.anchor = Some(anchor);
        self
    }

    pub fn with_stop_visible(mut self, visible: bool) -> Self {
        self.stop_visible = visible;
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_send<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Cmd/Ctrl+Enter submits the draft as a NEW agent conversation (warp-new).
    pub fn with_on_cmd_enter<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_cmd_enter = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Cmd/Ctrl+Alt+Enter submits the draft to the cloud agent (warp-new's
    /// `⌘⌥⏎`). The host routes the conversation and runs the turn.
    pub fn with_on_send_to_cloud<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send_to_cloud = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_attach<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_attach = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_model<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_key<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_variant<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_variant = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_voice<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_voice = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_image<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_image = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_code<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_code = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_link<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_link = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_stop<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_stop = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Seat an optional host control at the footer's trailing edge: the
    /// documented place for the live-speech button the owner will enable from
    /// Settings (its design is owned by a separate change). The rich input never
    /// draws a send control, so this seat is the only trailing affordance and
    /// stays empty unless a host fills it. The builder runs once per frame, the
    /// way every other child of the composer's rebuilt tree is produced, and the
    /// card's footer still reserves [`COMPOSER_FOOTER_HEIGHT`] either way.
    pub fn with_footer_action<F>(mut self, build: F) -> Self
    where
        F: FnMut() -> Box<dyn Element> + 'static,
    {
        self.footer_action = Some(Rc::new(RefCell::new(build)));
        self
    }

    /// Draw the pane's approval mode beside the model select: the control opens
    /// the four-mode menu, and Shift+Tab cycles them from the keyboard.
    pub fn with_approval_mode(mut self, mode: ApprovalMode) -> Self {
        self.approval_mode = Some(mode);
        self
    }

    /// The mode menu's app-owned open flag, so it survives the per-frame
    /// rebuild the way the model menu's does.
    pub fn with_approval_menu_open(mut self, open: Rc<RefCell<bool>>) -> Self {
        self.approval_menu_open = open;
        self
    }

    /// The mode menu's app-owned keyboard highlight, like its open flag.
    pub fn with_approval_menu_highlight(mut self, highlight: Rc<RefCell<Option<usize>>>) -> Self {
        self.approval_menu_highlight = highlight;
        self
    }

    /// Report the mode the user picked (or cycled to). The host stores it on
    /// the pane and hands it back on the next frame.
    pub fn with_on_select_approval_mode<F: FnMut(ApprovalMode) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_select_approval_mode = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the model menu: `items` to show, the app-owned `open` flag (so
    /// open state survives the per-frame rebuild), and a select callback.
    ///
    /// The host draws the models as it draws the slash-command list: one
    /// full-width band over the input, in the same slot, opened by the control
    /// here and taken with Up/Down, Enter or Tab, Escape or a click on a row.
    pub fn with_model_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.model_menu_items = items;
        self.model_menu_open = open;
        self.on_select_model_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Share the model list's chosen row with the host, so the highlight
    /// survives the per-frame rebuild the way the open flag does. A host that
    /// wires no cell keeps the selection inside the composer, which shows the
    /// row the pointer is on but cannot hold the keyboard's own move.
    pub fn with_model_menu_index(mut self, index: Rc<RefCell<usize>>) -> Self {
        self.model_index = index;
        self
    }

    /// Attach the column tray the model control opens. Setting one replaces the
    /// flat model list as what the control opens: the same press toggles the
    /// tray, and the keys the arrows send go to its columns.
    pub fn with_model_tray(mut self, tray: ComposerModelTray) -> Self {
        self.model_tray = Some(tray);
        self
    }

    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn attachments(&self) -> &[String] {
        &self.attachments
    }

    /// The context band: the card's top segment, over the input segment. One
    /// row — borderless controls on the left (a bound shell's chip, the
    /// working directory, the branch) and the environment control at the right
    /// end — 16 horizontal padding inside the card, 12 above and 6 below. `None`
    /// when the host set no context control at all (a bare thread input), so the
    /// card is just the input segment.
    fn context_band(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let left = self.context_children(app);
        let environment = self.environment_control(app);
        if left.is_empty() && environment.is_none() {
            return None;
        }
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm);
        if left.is_empty() {
            row = row.with_child(Spacer::new().finish());
        } else {
            let left = Wrap::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_run_spacing(sm)
                .with_children(left)
                .finish();
            row = row.with_child(Expanded::new(left).finish());
        }
        if let Some(environment) = environment {
            row = row.with_child(environment);
        }
        Some(row.finish())
    }

    /// The footer row under the editor: the borderless controls on the left
    /// (attach, the model chip, the approval mode, the modal badge), the stop
    /// circle while a turn streams, and the documented trailing-action seat
    /// where the optional live-speech control will sit. The row keeps
    /// [`COMPOSER_FOOTER_HEIGHT`] so the card's layout is the same whether or
    /// not anything sits at its right end; the left group wraps on a narrow
    /// pane instead of running past the input's edge.
    ///
    /// **There is no send control.** The owner removed the circular send button:
    /// a draft is sent with Enter, Cmd/Ctrl+Enter or Cmd/Ctrl+Alt+Enter (the
    /// keyboard paths below and in the editor), never with a press on the bar.
    fn footer_row(&mut self, app: &AppContext) -> Option<Box<dyn Element>> {
        let left = self.action_children(app);
        let right = self.stop_control(app);
        // A submit surface keeps the footer row even when every control in it is
        // absent: the row is what reserves [`COMPOSER_FOOTER_HEIGHT`] inside the
        // card, and the hosts size their panes from that height (the shell pane's
        // pty rows, `COMPOSER_BAR_RESERVED_HEIGHT`). Removing the send circle
        // must not shrink the card.
        let submits = self.on_send.is_some() || self.on_cmd_enter.is_some();
        if left.is_empty() && right.is_none() && self.footer_action.is_none() && !submits {
            return None;
        }
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let wrap = Wrap::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_run_spacing(sm)
            .with_children(left)
            .finish();
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(Expanded::new(wrap).finish());
        if let Some(right) = right {
            row = row.with_child(right);
        }
        // The optional trailing control's seat: the live-speech button will sit
        // here, at the footer's right end, when the owner enables it from
        // Settings. Nothing is wired today, so the seat is empty.
        if let Some(build) = self.footer_action.clone() {
            row = row.with_child((build.borrow_mut())());
        }
        Some(
            ConstrainedBox::new(row.finish())
                .with_min_height(COMPOSER_FOOTER_HEIGHT)
                .finish(),
        )
    }

    /// The stop circle at the footer's right end, while a turn streams. A
    /// surface with no stop keeps the footer text-only. The error-red circle is
    /// the only round control left in the bar: the send circle it used to swap
    /// with was removed by the owner.
    fn stop_control(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        if !self.stop_visible {
            return None;
        }
        let on_stop = self.on_stop.clone()?;
        let circle = ComposerButton::new(
            Icon::new("stop")
                .with_size(14.0)
                .with_theme_color(ColorToken::Bg, app)
                .finish(),
        )
        .with_height(COMPOSER_SEND_SIZE)
        .with_h_pad((COMPOSER_SEND_SIZE - 14.0) / 2.0)
        .with_radius(COMPOSER_SEND_SIZE / 2.0)
        .with_fill(ColorToken::Error)
        .with_fill_alpha(0.9)
        .with_on_click(move || (on_stop.borrow_mut())())
        .finish();
        Some(
            Tooltip::new(circle, "Stop")
                .with_position(TooltipPosition::Above)
                .finish(),
        )
    }

    /// The environment control at the context band's right end: icon-only,
    /// naming where the draft's turn runs (a computer, a cloud). The host's
    /// label is the tooltip, and the tray opens leftwards from the band's edge.
    fn environment_control(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let label = self
            .harness_label
            .clone()
            .filter(|label| !label.trim().is_empty())?;
        let icon = ComposerButton::new(
            Icon::new(self.harness_icon)
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_height(CONTEXT_PILL_HEIGHT)
        .with_h_pad(6.0)
        .with_radius(COMPOSER_CONTROL_RADIUS)
        .finish();
        let trigger = Tooltip::new(icon, format!("Environment · {label}"))
            .with_position(TooltipPosition::Above)
            .finish();
        let Some(cb) = self.on_select_harness_item.clone() else {
            return Some(trigger);
        };
        Some(
            PopupMenu::new(trigger, self.harness_menu_items.clone())
                .with_open(self.harness_menu_open.clone())
                .with_position(PopupMenuPosition::AboveEnd)
                .with_max_width(ENVIRONMENT_MENU_WIDTH)
                // The trigger is the icon-only control inside the band's own
                // padding: nudging the casing right puts its edge ~6pt inside
                // the card, the reference's alignment, instead of 16 in.
                .with_panel_offset(vec2f(10.0, 0.0))
                .with_on_select(move |idx| (cb.borrow_mut())(idx))
                .finish(),
        )
    }

    /// The context band's left group, left to right: where the shell is (an
    /// SSH session's chip), the working directory and the branch. Each is drawn
    /// only when the surface set it, so a terminal composer that sets only the
    /// directory and the branch shows exactly those two, and a composer with no
    /// context at all contributes nothing. The environment control is not in
    /// this group: it sits at the band's right end, where the reference puts
    /// it. The working tree's diff control is not here either: the owner moved
    /// "Open changes" to the app topbar.
    fn context_children(&self, app: &AppContext) -> Vec<Box<dyn Element>> {
        let mut children: Vec<Box<dyn Element>> = Vec::new();
        // The session chip leads the row: where the shell *is* frames the
        // directory and the branch that follow it, which describe what is
        // running there. A local session contributes nothing.
        if let Some(session) = self.ssh_session.as_ref() {
            children.push(self.session_chip(app, &session.chip_label()));
        }
        if let Some(path) = self.path_label.clone() {
            // The directory pill is capped rather than flex-grown: a long path
            // ellipsizes inside the pill and the row's pills stay left-aligned
            // instead of being pushed apart by it.
            let dir_child = || {
                Flex::row()
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("folder")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        ConstrainedBox::new(
                            Clipped::new(
                                Text::new(path.clone())
                                    .with_theme_color(ColorToken::Muted, app)
                                    .with_font_size(CONTROL_TEXT_SIZE)
                                    .with_max_lines(1)
                                    .finish(),
                            )
                            .finish(),
                        )
                        .with_max_width(DIR_PILL_MAX_WIDTH)
                        .finish(),
                    )
                    .with_child(
                        Icon::new("chevron-down")
                            .with_size(12.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .finish()
            };
            let button = ComposerButton::new(dir_child())
                .with_height(CONTEXT_PILL_HEIGHT)
                .with_h_pad(CONTROL_H_PAD)
                .with_radius(COMPOSER_CONTROL_RADIUS)
                .with_outline(false)
                .finish();
            let trigger = Tooltip::new(button, "Select working directory")
                .with_position(TooltipPosition::Above)
                .finish();
            // The menu is the host's when it wired a select callback, rows or
            // not: the directory list is read when the menu opens, so it is
            // empty while closed and gating on the rows would leave the pill
            // with no way to open it.
            let dir = if let Some(cb) = self.on_select_dir_item.clone() {
                PopupMenu::new(trigger, self.dir_menu_items.clone())
                    .with_open(self.dir_menu_open.clone())
                    .with_highlight_index(self.dir_menu_highlight.clone())
                    .with_position(PopupMenuPosition::Above)
                    .with_max_visible_rows(MENU_MAX_VISIBLE_ROWS)
                    .with_panel_scroll(self.dir_menu_scroll.clone())
                    .with_on_select(move |idx| (cb.borrow_mut())(idx))
                    .finish()
            } else {
                trigger
            };
            children.push(dir);
        }
        if let Some(label) = self.branch_label.clone() {
            children.push(self.context_pill(
                app,
                "branch",
                &label,
                &self.branch_menu_items,
                self.branch_menu_open.clone(),
                Some(self.branch_menu_highlight.clone()),
                &self.on_select_branch_item,
                "Select branch",
                Some(BRANCH_MENU_WIDTH),
            ));
        }
        children
    }

    /// The model control: the model the draft runs on, which opens the DSH-style
    /// two-level menu anchored above it (see `elements::model_tray`).
    ///
    /// The chip is DeepSeek Harness's model seat: the model name in the label
    /// tone, the effort in the caption tone, a 4pt gap and a 12pt chevron that
    /// points up while the card is open (`ModelSelect.module.css`'s
    /// `.trigger`). A pane with no model configured reads
    /// [`MODEL_NOT_CONFIGURED`] rather than a model name, so the control never
    /// claims a turn would run on something nobody chose.
    fn model_control(&self, app: &AppContext, label: String) -> Box<dyn Element> {
        let text = if label.trim().is_empty() {
            MODEL_NOT_CONFIGURED.to_string()
        } else {
            label
        };
        // The card is open, so the chip's value reads in the text colour: the
        // name being picked is the one the card above is about.
        let open = *self.model_menu_open.borrow()
            || self
                .model_tray
                .as_ref()
                .is_some_and(|tray| *tray.open.borrow());
        let label_color = if open {
            ColorToken::Text
        } else {
            ColorToken::Muted
        };
        // The effort the chip shows: the level in force, or the provider default
        // when the model offers levels but none is set — DSH's caption after the
        // model name.
        let effort = self.model_tray.as_ref().and_then(|tray| {
            if !tray.level.trim().is_empty() {
                Some(crate::elements::level_label(&tray.level))
            } else {
                let has_levels = tray
                    .harnesses
                    .iter()
                    .flat_map(|group| group.models.iter())
                    .any(|model| model.id == tray.model && !model.thinking.is_empty());
                has_levels.then(|| "Default".to_string())
            }
        });
        let mut content = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0)
            .with_child(
                Text::new(text)
                    .with_theme_color(label_color, app)
                    .with_font_size(CONTROL_TEXT_SIZE)
                    .finish(),
            );
        if let Some(effort) = effort {
            content = content.with_child(
                Text::new(effort)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(CONTROL_TEXT_SIZE)
                    .finish(),
            );
        }
        content = content.with_child(
            Icon::new(if open { "chevron-up" } else { "chevron-down" })
                .with_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
        // DSH's chip padding is asymmetric — 8pt before the label, 4pt after the
        // chevron — so the content carries the inset and the button's own
        // symmetric pad is zero.
        let content = Padding::new(content.finish(), EdgeInsets::new(8.0, 0.0, 4.0, 0.0)).finish();
        let button = ComposerButton::new(content)
            .with_height(28.0)
            .with_h_pad(0.0)
            .with_radius(COMPOSER_CONTROL_RADIUS)
            .with_outline(false)
            .with_hover_fill(ColorToken::Selected);
        let button = if let Some(tray) = self.model_tray.clone() {
            // The chip toggles the host's open flag; the card it opens is the
            // one the composer wraps the chip in below.
            let dismissed = self.slash_dismissed.clone();
            button
                .with_on_click(move || {
                    let mut open = tray.open.borrow_mut();
                    if !*open {
                        // The card opens on the pane's own model and level, so
                        // the first row it marks is the one in force.
                        tray.state.borrow_mut().seeded = false;
                        // One band at a time: opening the card puts the draft's
                        // command list away, so the keys that follow belong to
                        // this band alone.
                        *dismissed.borrow_mut() = true;
                    }
                    *open = !*open;
                })
                .finish()
        } else {
            match self.on_select_model_item.clone() {
                // The list is the host's, drawn over the whole input block; the
                // control opens and closes it, so a second press on the control
                // puts it away the way Escape does.
                Some(_) => {
                    let open = self.model_menu_open.clone();
                    let index = self.model_index.clone();
                    let dismissed = self.slash_dismissed.clone();
                    let selected = self
                        .model_menu_items
                        .iter()
                        .position(|item| item.selected)
                        .unwrap_or(0);
                    button
                        .with_on_click(move || {
                            let mut open = open.borrow_mut();
                            if !*open {
                                // The band opens on the model in force, so Enter
                                // takes the model the pane already runs.
                                *index.borrow_mut() = selected;
                                // One band at a time: opening the models puts the
                                // draft's command list away the way Escape does, so
                                // the keys that follow belong to this band alone.
                                *dismissed.borrow_mut() = true;
                            }
                            *open = !*open;
                        })
                        .finish()
                }
                // A host that wired only the plain callback keeps a control that
                // hands the press on, with no list to draw.
                None => match self.on_select_model.clone() {
                    Some(cb) => button.with_on_click(move || (cb.borrow_mut())()).finish(),
                    None => button.finish(),
                },
            }
        };
        let trigger = Tooltip::new(button, "Select model")
            .with_position(TooltipPosition::Above)
            .finish();
        // The DSH menu is a card anchored to this chip, not a sibling band: the
        // chip is wrapped in the menu element, which draws the card above it.
        match self.model_tray.as_ref() {
            Some(tray) => tray.popup(trigger),
            None => trigger,
        }
    }

    /// The approval-mode control: the mode the pane runs under, next to the
    /// model it runs on. The control opens the four-mode menu; Shift+Tab cycles
    /// them without opening it. The label is the mode's own word, so the pane
    /// never has to open the menu to say what policy is in force.
    fn approval_control(&self, app: &AppContext, mode: ApprovalMode) -> Box<dyn Element> {
        // No glyph: the mode's word is the control, and its colour says which
        // policy is in force.
        let mut pill = ContextPill::new("sliders", mode.label())
            .without_icon()
            .with_label_color(mode.color())
            .with_hover_fill(ColorToken::Selected)
            .with_tooltip(format!(
                "Approval mode · {} · shift+tab to cycle",
                mode.description()
            ))
            .with_tray_side(PillTraySide::Above);
        if let Some(cb) = self.on_select_approval_mode.clone() {
            let items = ApprovalMode::ALL
                .iter()
                .map(|option| {
                    let mut item = PopupMenuItem::new(option.label());
                    if *option == mode {
                        item = item.selected();
                    }
                    item
                })
                .collect();
            pill = pill
                .with_menu(items, self.approval_menu_open.clone(), move |index| {
                    (cb.borrow_mut())(ApprovalMode::from_index(index))
                })
                .with_highlight_index(self.approval_menu_highlight.clone());
        }
        pill.finish(app)
    }

    /// Shift+Tab cycles the pane's approval mode. Taken before the editor sees
    /// the key, and only when the host wired the control and the callback.
    fn handle_approval_mode_key(&mut self, event: &DispatchedEvent) -> bool {
        if !self.focused {
            return false;
        }
        let (Some(mode), Some(cb)) = (self.approval_mode, self.on_select_approval_mode.clone())
        else {
            return false;
        };
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        if key != "Tab" || !modifiers.shift || modifiers.command || modifiers.ctrl || modifiers.alt {
            return false;
        }
        (cb.borrow_mut())(mode.next());
        true
    }

    /// The footer's left group, left to right: the attach control, the model
    /// chip or the custom harness's own mark, the approval mode and the modal
    /// badge. A plain shell command runs no model, so a terminal composer
    /// contributes only what its host wired. A custom harness owns its own model
    /// and approvals, so its mark stands alone where those two controls would
    /// be. The stop circle (and the optional trailing action) is not in this
    /// group: it is the footer's right end (see `stop_control`); the send
    /// circle was removed from the rich input entirely.
    fn action_children(&self, app: &AppContext) -> Vec<Box<dyn Element>> {
        let mut children: Vec<Box<dyn Element>> = Vec::new();
        let badge = self.vim.as_ref().map(|vim| self.mode_badge(app, vim));
        if let Some(cb) = self.on_attach.clone() {
            let attach = ComposerButton::new(
                Icon::new("plus")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_height(CONTEXT_PILL_HEIGHT)
            // No side inset: the `+` is the footer's left edge, the reference's
            // first glyph on the row.
            .with_h_pad(0.0)
            .with_radius(COMPOSER_CONTROL_RADIUS)
            .with_outline(false)
            .with_hover_fill(ColorToken::Selected)
            .with_on_click(move || (cb.borrow_mut())())
            .finish();
            children.push(
                Tooltip::new(attach, "Attach")
                    .with_position(TooltipPosition::Above)
                    .finish(),
            );
        }
        if let Some(harness) = self.agent_harness {
            let name = goble_core::harness::harness_for(harness)
                .map(|spec| spec.name)
                .unwrap_or(harness);
            let mark = Container::new(harness_bar_mark(harness, 16.0, app))
                .with_padding(EdgeInsets::new(CONTROL_H_PAD, 0.0, CONTROL_H_PAD, 0.0))
                .finish();
            children.push(
                Tooltip::new(mark, format!("{name} harness"))
                    .with_position(TooltipPosition::Above)
                    .finish(),
            );
        } else {
            if let Some(label) = self.model_label.clone() {
                children.push(self.model_control(app, label));
            }
            if let Some(mode) = self.approval_mode {
                children.push(self.approval_control(app, mode));
            }
        }
        if let Some(badge) = badge {
            children.push(badge);
        }
        children
    }

    /// The modal-editing badge, the last control of the footer: the half-typed
    /// command while one is pending, otherwise the mode the next key belongs
    /// to.
    fn mode_badge(&self, app: &AppContext, vim: &Rc<RefCell<VimState>>) -> Box<dyn Element> {
        let vim = vim.borrow();
        let showcmd = vim.showcmd();
        let mode = vim.mode();
        let text = if showcmd.is_empty() {
            mode.label().to_string()
        } else {
            showcmd
        };
        // Insert mode is the ordinary state, so it reads as a quiet hint; the
        // command modes carry the accent.
        let color = if mode == VimMode::Insert {
            ColorToken::Muted
        } else {
            ColorToken::Accent
        };
        let tooltip = if mode == VimMode::Insert {
            "Vim: insert mode (Esc for normal mode)"
        } else {
            "Vim mode"
        };
        Tooltip::new(
            Text::new(text)
                .with_theme_color(color, app)
                .with_font_size(CONTROL_TEXT_SIZE)
                .finish(),
            tooltip,
        )
        .with_position(TooltipPosition::Above)
        .finish()
    }

    /// One context pill: an icon and a label, content-sized. When `items` is
    /// non-empty it is wrapped in the shared [`PopupMenu`] so the app-owned
    /// `open` flag survives the per-frame rebuild, otherwise it renders as a
    /// plain pill. The agent's pills are the shared [`ContextPill`] (a terminal
    /// pane shows the same pill in its topbar), so the two surfaces stay
    /// identical.
    fn context_pill(
        &self,
        app: &AppContext,
        icon: &'static str,
        label: &str,
        items: &[PopupMenuItem],
        open: Rc<RefCell<bool>>,
        highlight: Option<Rc<RefCell<Option<usize>>>>,
        on_select: &Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
        tooltip: &str,
        menu_max_width: Option<f32>,
    ) -> Box<dyn Element> {
        let mut pill = ContextPill::new(icon, label.to_string())
            .with_tooltip(tooltip)
            .with_tray_side(PillTraySide::Above);
        if let Some(width) = menu_max_width {
            pill = pill.with_menu_max_width(width);
        }
        // The callback, not the rows, is what says the host wired a menu: a
        // pill whose rows are read when it opens has none while it is closed.
        if let Some(cb) = on_select.clone() {
            pill = pill.with_menu(items.to_vec(), open, move |idx| (cb.borrow_mut())(idx));
            if let Some(highlight) = highlight {
                pill = pill.with_highlight_index(highlight);
            }
        }
        pill.finish(app)
    }

    /// The session chip: the host a bound SSH session put this pane's shell on,
    /// as a flat row of an icon and the session's own words — no card, no
    /// outline, no tray. It is chrome beside the input rather than a control,
    /// so it draws the same muted control-size text the working-directory
    /// pill's label uses, capped the same way.
    fn session_chip(&self, app: &AppContext, label: &str) -> Box<dyn Element> {
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(
                Icon::new("conversation-remote")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                ConstrainedBox::new(
                    Clipped::new(
                        Text::new(label.to_string())
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(CONTROL_TEXT_SIZE)
                            .with_max_lines(1)
                            .finish(),
                    )
                    .finish(),
                )
                .with_max_width(SESSION_CHIP_MAX_WIDTH)
                .finish(),
            )
            .finish()
    }

    /// Rebuild the rich-input tree.
    ///
    /// One card, two segments, whichever surface this is: the **context band**
    /// on top (the schema/SSH chip, directory, branch, and the environment
    /// control at the right end) and the **input segment** under it
    /// (the editor, the footer row with the attach/model/approval controls and
    /// the circular send, and the host's instruction strip when it has one).
    /// The terminal bar, the agent input, the thread input and the custom
    /// harness's composer all draw this same card — only the set of controls
    /// differs — so switching a pane between its shell and its agent view moves
    /// nothing but the text in the editor.
    fn rebuild(&mut self, app: &AppContext) {
        // The submit closure the editor's Enter path calls: the submit carries
        // the Enter key's modifiers so the host can route a plain Enter (a
        // terminal command) vs Cmd/Ctrl+Enter (a NEW agent conversation). The
        // rich input draws no send control — the keyboard is the whole send
        // gesture — so this closure is reached from the editor alone.
        let value_for_send = self.value.clone();
        let on_send = self.on_send.clone();
        let on_cmd_enter = self.on_cmd_enter.clone();
        let send: Rc<RefCell<dyn FnMut(ModifiersState)>> =
            Rc::new(RefCell::new(move |modifiers: ModifiersState| {
                let text = value_for_send.borrow().clone();
                let agent_submit = (modifiers.command || modifiers.ctrl) && !modifiers.shift;
                if agent_submit {
                    // Cmd/Ctrl+Enter is the way into the agent, not only a
                    // submit: the host opens the pane's agent view even with an
                    // empty draft (warp-new's gesture), so the callback fires
                    // either way. An empty draft is the host's own no-op.
                    if let Some(cb) = on_cmd_enter.as_ref() {
                        (cb.borrow_mut())(text.clone());
                    }
                    *value_for_send.borrow_mut() = String::new();
                } else {
                    // An empty submit is the host's own gesture: while a prompt
                    // is queued it is the second Enter that sends it now, and
                    // every other host treats it as a no-op.
                    if let Some(cb) = on_send.as_ref() {
                        (cb.borrow_mut())(text.clone());
                    }
                    *value_for_send.borrow_mut() = String::new();
                }
            }));

        // The draft's command band owns the input while it is up: the card's
        // top segment — the context band with the session, directory, branch and
        // environment controls — steps out of the way, so the surface is the
        // slash menu over the input segment (DSH's slash state), and it comes
        // back the moment the query is cleared or the band is dismissed. A card
        // whose host set no context control is `None` either way.
        let slash_open = self.slash_band_open();
        let context = if slash_open {
            None
        } else {
            self.context_band(app)
        };
        let has_context = context.is_some();

        // The input segment's own column: the draft's attachments, the editor,
        // the footer row, and the host's instruction strip.
        let mut input = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(COMPOSER_FOOTER_GAP);
        if !self.attachments.is_empty() {
            let mut attachment_row = Wrap::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.0)
                .with_run_spacing(6.0);
            for attachment in &self.attachments {
                // A chip is one more rounded control: 24pt tall, its own 1px
                // outline, the raised fill, so it reads as part of the field it
                // belongs to rather than as a loose label.
                let chip = Container::new(
                    Text::new(attachment.clone())
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(CONTROL_TEXT_SIZE)
                        .with_max_lines(1)
                        .finish(),
                )
                .with_background(Fill::Solid(app.theme.color(ColorToken::Hover)))
                .with_border(
                    Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)),
                )
                .with_corner_radius(COMPOSER_CONTROL_RADIUS)
                .with_padding(EdgeInsets::new(8.0, 3.0, 8.0, 3.0))
                .finish();
                attachment_row = attachment_row.with_child(chip);
            }
            input = input.with_child(attachment_row.finish());
        }

        // Neither host band is drawn here: the slash-command list and the model
        // list are the host's, drawn over the whole input block above it. What
        // the composer keeps of them is the key routing — see `handle_slash_key`
        // and `handle_model_key`.
        let value = self.value.clone();
        let on_change = self.on_change.clone();
        let on_focus_change = self.on_focus_change.clone();
        let send_for_submit = send.clone();
        // A disabled bar draws no draft: the preserved text is the app's,
        // handed back once the decision lands, but showing it here would read
        // as an editor that still takes keys. The bar says what it waits for
        // instead. A bar that only *waits* keeps its draft and its keys: the
        // follow-up is exactly what the input is for.
        let textarea = TextArea::new()
            .with_value(if self.disabled {
                String::new()
            } else {
                self.value.borrow().clone()
            })
            .with_placeholder(if self.disabled {
                "Waiting for your decision…".to_string()
            } else if self.waiting {
                // The decision still owns Enter; typing is a follow-up that
                // waits its turn, so the bar says what the input is for.
                "Message the agent while it waits…".to_string()
            } else {
                self.placeholder.clone()
            })
            .with_min_height(COMPOSER_EDITOR_MIN_HEIGHT)
            // The composer's draft is the rich input's body text: 15pt at 1.4
            // line height, the reference's editor size.
            .with_font_size(15.0)
            .with_line_height(1.4)
            .with_focused(self.focused && !self.disabled)
            .with_blur_on_outside_click(self.blur_on_outside_click)
            // The editor owns its whole line: a press to the right of the text
            // is a press in the editor and puts the beam at the end.
            .with_full_width(true)
            // A long draft folds inside the field: the editor stays one text
            // field, and its caret follows the row the index lands in instead
            // of running past the field's edge.
            .with_wrapping(true)
            .with_caret(self.caret.clone())
            .with_vim_opt(self.vim.clone())
            .with_clipboard_opt(self.clipboard.clone())
            .with_anchor_opt(self.anchor.clone())
            .with_on_change(move |text| {
                *value.borrow_mut() = text.clone();
                // The draft is the menu's query: the host re-filters the
                // commands from it, and the menu opens and closes with the
                // leading `/` itself (see `slash_menu_open`).
                if let Some(cb) = on_change.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_focus_change(move |focused| {
                if let Some(cb) = on_focus_change.as_ref() {
                    (cb.borrow_mut())(focused);
                }
            })
            .with_on_submit(move |mods| (send_for_submit.borrow_mut())(mods))
            .finish();
        input = input.with_child(
            ConstrainedBox::new(textarea)
                .with_min_height(COMPOSER_EDITOR_MIN_HEIGHT)
                .with_max_height(COMPOSER_EDITOR_MAX_HEIGHT)
                .finish(),
        );

        // The footer row: the borderless controls on the left and the circular
        // send (or stop) on the right, both inside the input segment.
        if let Some(footer) = self.footer_row(app) {
            input = input.with_child(footer);
        }

        // The instructions the host keeps inside the input close it, under the
        // footer: the shell bar's `⌘↵ new conversation` is the entry the pane
        // shows here rather than over the separator, where the rest of a pane's
        // strip goes.
        if !self.hints.is_empty() {
            input = input.with_child(ShortcutHints::new(self.hints.clone()).finish(app));
        }

        // The segment is borderless (owner feedback: the rich input and its
        // outer ring read cleaner without one). It is a flat `SurfaceRaised`
        // card over the `Surface` card, separated by tone and its radius alone.
        let input_segment = Container::new(input.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_corner_radius(COMPOSER_INPUT_RADIUS)
            // 16 on the text side, 8 on the trailing side: the footer's
            // controls keep the reference's inset from the segment's edge.
            .with_padding(EdgeInsets::new(16.0, 14.0, 8.0, 8.0))
            .finish();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(0.0);
        if let Some(context) = context {
            // The card's own inset is 6; the band's controls sit 10 further in,
            // which is the reference's 16 horizontal padding. 12 above them and
            // 6 below close the band at [`CONTEXT_SEGMENT_HEIGHT`].
            column = column.with_child(
                Padding::new(context, EdgeInsets::new(10.0, 8.0, 10.0, 8.0)).finish(),
            );
        }
        column = column.with_child(input_segment);

        let card = self.into_card(column.finish(), app, has_context);
        // The two bands share one slot over the input, and both are the host's
        // to draw (the command list and the model list, in that order): while
        // the draft is a command that list holds the slot, so the model card is
        // put away rather than drawn beside it — and does not come back when the
        // draft stops being one, because the user never opened it again.
        if slash_open {
            *self.model_menu_open.borrow_mut() = false;
            if let Some(tray) = self.model_tray.as_ref() {
                *tray.open.borrow_mut() = false;
            }
        }
        // A waiting decision puts both bands away: the card above the input is
        // the surface the next key belongs to.
        if self.disabled {
            *self.model_menu_open.borrow_mut() = false;
            *self.approval_menu_open.borrow_mut() = false;
            if let Some(tray) = self.model_tray.as_ref() {
                *tray.open.borrow_mut() = false;
            }
        }
        self.root = Some(card);
    }

    /// Wrap the segments in the composer's card: radius 26, the `Surface` fill,
    /// no outline (owner feedback: the rich input's outer ring reads cleaner
    /// without a border), and a 6pt inset that holds the input segment and the
    /// card's bottom edge. The card and its segment are flat surfaces separated
    /// by tone alone. The context band is drawn at the card's own top edge (its
    /// padding opens it 12pt down); a card with no context band insets the input
    /// segment from the top too, so the shape stays a rounded box.
    ///
    /// The card sits in the surface's own gutter ([`COMPOSER_CARD_GUTTER`]) on
    /// every host, so the shell bar, the agent input and the thread input are
    /// the same object in the same place.
    fn into_card(
        &self,
        content: Box<dyn Element>,
        app: &AppContext,
        has_context: bool,
    ) -> Box<dyn Element> {
        let top = if has_context { 0.0 } else { COMPOSER_CARD_INSET };
        let card = Container::new(content)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_corner_radius(COMPOSER_CARD_RADIUS)
            .with_padding(EdgeInsets::new(
                COMPOSER_CARD_INSET,
                top,
                COMPOSER_CARD_INSET,
                COMPOSER_CARD_INSET,
            ))
            .finish();
        Padding::new(
            card,
            EdgeInsets::new(
                COMPOSER_CARD_GUTTER,
                0.0,
                COMPOSER_CARD_GUTTER,
                COMPOSER_CARD_GUTTER,
            ),
        )
        .finish()
    }

}

impl Default for ChatComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for ChatComposer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
        // The card draws no focus ring: the owner asked for no blue border or
        // margin around the rich input, so the caret is the only focus signal.
        // The card is borderless too (owner: cleaner flat look), so it paints
        // nothing around itself whether it is focused or not;
        // `the_card_draws_no_focus_ring_when_focused` pins the removal.
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        // A disabled input is not a typing surface: the approval card that is
        // waiting owns both the keys and the presses, so nothing here is
        // allowed to take focus, open a menu or submit a draft.
        if self.disabled {
            return false;
        }
        if self.handle_send_to_cloud_key(event) {
            return true;
        }
        if self.handle_approval_mode_key(event) {
            return true;
        }
        if self.handle_proposal_key(event) {
            return true;
        }
        // The model band is over the input and holds the keys that take a row
        // from it while it is up.
        if self.handle_model_tray_key(event) {
            return true;
        }
        if self.handle_model_key(event) {
            return true;
        }
        // While the menu is up it owns the keys that pick from it: the editor
        // keeps every other key, because what it types is the menu's query.
        if self.handle_slash_key(event) {
            return true;
        }
        let handled = self
            .root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false);
        if handled {
            return true;
        }
        // A press anywhere in the rich input that no child consumed (the card's
        // gutters, the pill rows, the empty tail of the column) is still a press
        // on the input: focus the editor so the surface becomes writable and the
        // beam shows. Children that do consume the press — the pills, the menus,
        // the editor itself — keep their own behaviour.
        if let DispatchedEvent::MouseDown { position, .. } = event {
            if !self.focused {
                if let Some(bounds) = self.bounds() {
                    if bounds.contains(PointF::new(position.x, position.y)) {
                        if let Some(cb) = self.on_focus_change.as_ref() {
                            (cb.borrow_mut())(true);
                        }
                        return true;
                    }
                }
            }
        }
        false
    }
}
