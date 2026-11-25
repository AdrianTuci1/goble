use std::cell::RefCell;
use std::rc::Rc;

use super::band::BandCard;
use super::hints_row::ComposerHintsRow;
use super::proposal::CommandProposalUi;
use crate::color::ColorU;
use crate::elements::conversation_list_item::truncate_with_ellipsis;
use crate::elements::model_tray::MODEL_MENU_VIEWPORT_INSET;
use crate::elements::text::measure_text;
use crate::elements::{
    harness_bar_mark, AppContext, ApprovalMode, Border, ComposerButton, ComposerModelTray,
    ConstrainedBox, Container, ContextPill, CrossAxisAlignment, DiffSummary, EdgeInsets, Element,
    Empty, Expanded, Fill, Flex, Icon, LayoutContext, MainAxisSize, ModelTrayBounds, ModelTrayKey,
    Padding, PaintContext, PanelScroll, PillTraySide, Point, PopupMenu, PopupMenuItem,
    PopupMenuPosition, ShortcutHint, SizeConstraint, SlashMenuItem, Spacer, Text, TextArea,
    Tooltip, TooltipPosition, Wrap, COMPOSER_CONTROL_RADIUS, CONTEXT_PILL_HEIGHT,
    CONTEXT_PILL_LABEL_SIZE,
};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::{rectf, vec2f, PointF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};
use crate::vim::{Clipboard, VimMode, VimState};
use goble_core::harness::CommandDecision;
/// What the model control reads when the pane has no model configured: the
/// turn would run on nothing, so naming a model there would be a claim nobody
/// made. The host's own resolved label says the same thing (its model value is
/// empty until a model is configured), and this covers the label it passes.
pub const MODEL_NOT_CONFIGURED: &str = "not configured";

/// The placeholder an agent/chat surface shows: the input asks for a prompt.
/// A terminal surface takes [`TERMINAL_COMPOSER_PLACEHOLDER`] through the
/// builder instead (the app says which surface the shared input is on).
pub const AGENT_COMPOSER_PLACEHOLDER: &str = "Ask anything...";

/// The placeholder a plain shell's rich input shows: the line runs a command in
/// the pane's own PTY, so it asks for a command instead of a prompt. Only the
/// terminal surface gets it; agent surfaces (chat, agent view, worker,
/// sub-agent) keep [`AGENT_COMPOSER_PLACEHOLDER`].
pub const TERMINAL_COMPOSER_PLACEHOLDER: &str = "Run any command";

/// The rich input's controls share one label size and one side inset, so every
/// button in the footer — the model, the attach and stop controls and the
/// environment — reads as one compact strip. The height itself is
/// [`CONTEXT_PILL_HEIGHT`], the pill's own.
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
/// The terminal band's bottom casing: the card's own 6pt inset plus the 12pt
/// gutter the floating card kept under itself, folded into the band when the
/// card stretches to the pane's bottom edge. The folded gutter is what keeps the
/// band the same height as the floating card (and the inner input exactly where
/// it was relative to the pane's bottom); the band's top and sides use
/// [`COMPOSER_CARD_INSET`] alone, since the band itself reaches the pane's
/// edges.
pub const COMPOSER_BAND_BOTTOM_INSET: f32 = COMPOSER_CARD_INSET + COMPOSER_CARD_GUTTER;
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
/// The instruction strip's own height: the capped key's box, whose 11pt label
/// is a 13.2pt line (11 x 1.2). The row is a **fixed row under the card**, not
/// a child of it, so a hint appearing, disappearing or being re-worded never
/// moves the card, the editor or a shell pane's reserved bar height. The
/// terminal and agent composers therefore have identical card heights.
pub const COMPOSER_HINTS_ROW_HEIGHT: f32 = 13.2;
/// The air between the card's bottom edge and the right-aligned instruction row
/// under it. The row is part of the composer's own height but outside the card,
/// so the card keeps its measured height whatever the hints say.
pub const COMPOSER_HINTS_GAP: f32 = 6.0;
/// The height of the floating composer card without instructions: the 12pt
/// surface gutter under the card, the card's own 6pt casing, the input segment
/// and the other 6pt casing. There is no context band any more — the owner
/// removed the directory/branch controls, so the card is one input segment
/// (owner: "nu mai oferim composer dublu").
pub const COMPOSER_CARD_RESERVED_HEIGHT: f32 =
    COMPOSER_CARD_GUTTER + COMPOSER_CARD_INSET * 2.0 + COMPOSER_INPUT_MIN_HEIGHT;
/// The height a host reserves for the whole rich input: the card plus the fixed
/// instruction row under it. The shell pane tells its pty this, so the rows the
/// program paints are the rows the user can see. It must equal the height the
/// element lays out; the composer test
/// `the_reserved_bar_height_is_the_cards_own_height` pins the two together. The
/// band form lays out the same height (the folded gutter becomes
/// [`COMPOSER_BAND_BOTTOM_INSET`], which pays for the same row), so a terminal
/// bar and an agent input are identical in height.
pub const COMPOSER_BAR_RESERVED_HEIGHT: f32 =
    COMPOSER_CARD_RESERVED_HEIGHT + COMPOSER_HINTS_GAP + COMPOSER_HINTS_ROW_HEIGHT;
/// The environment menu is wider than the other dropdowns: its rows carry a
/// description line under the label ("Clones this repo in a cloud
/// environment…"), and the reference shows it cut with an ellipsis on one line
/// rather than wrapped inside the row.
const ENVIRONMENT_MENU_WIDTH: f32 = 308.0;

/// DSH's trigger cap (`.trigger { max-width: 360px }`): the chip never grows
/// past it. A longer model name ellipsizes, and the effort gives its space up
/// first (`flex-shrink: 1000` in `ModelSelect.module.css`).
const MODEL_CHIP_MAX_WIDTH: f32 = 360.0;
/// The chip's fixed chrome inside that cap: the 8pt leading inset, the 4pt gap
/// before the chevron, the 12pt chevron and the 4pt trailing inset.
const MODEL_CHIP_CHROME: f32 = 8.0 + 4.0 + 12.0 + 4.0;
/// The gap a shown effort takes between the name and itself.
const MODEL_CHIP_EFFORT_GAP: f32 = 4.0;
/// DSH's chip chrome rounding: `--dsw-radius-sm` = 8pt on the
/// `.trigger` (the composer's shared control radius is tighter, 6pt).
const MODEL_CHIP_RADIUS: f32 = 8.0;
/// Below this the effort has no readable word left, so it drops out of the chip
/// entirely — the way DSH's 1000× shrink factor ellipsizes it away before the
/// model name loses a pixel.
const MODEL_CHIP_EFFORT_MIN: f32 = 12.0;
/// What the chip's own row leaves outside it: the card inset (6 a side) and the
/// input segment's 16/8 padding. A chip longer than that would paint out of the
/// card, so the cap shrinks with a narrow pane instead of only stopping at
/// DSH's 360pt design cap.
const MODEL_CHIP_FOOTER_INSET: f32 = 6.0 * 2.0 + 16.0 + 8.0;

/// DSH's `label-caption` tone: one step below `Muted` (`label-secondary`). The
/// theme carries two text tiers, so the caption is the muted tone pulled toward
/// the page background, so the effort and the chevron on the chip read dimmer
/// than the model name beside them (`ModelSelect.module.css` `.triggerEffort`,
/// `.chevron`).
fn caption_color(app: &AppContext) -> ColorU {
    app.theme
        .color(ColorToken::Muted)
        .mix(&app.theme.color(ColorToken::Bg), 0.35)
}

/// Fit the chip's value into DSH's 360pt cap: the effort shrinks first and drops
/// out before the model name loses a pixel, then the name ellipsizes. The chip
/// is one 28pt line, so the fit happens in the builder rather than letting a
/// long run wrap over the footer.
fn fit_model_chip(
    name: String,
    effort: Option<String>,
    max_width: f32,
) -> (String, Option<String>) {
    let max = max_width.max(MODEL_CHIP_FOOTER_INSET + 24.0);
    let name_width = measure_text(&name, CONTROL_TEXT_SIZE, 1.2, f32::INFINITY).x;
    if let Some(effort) = effort {
        let effort_width = measure_text(&effort, CONTROL_TEXT_SIZE, 1.2, f32::INFINITY).x;
        let total = MODEL_CHIP_CHROME + MODEL_CHIP_EFFORT_GAP + name_width + effort_width;
        if total <= max {
            return (name, Some(effort));
        }
        let effort_max = (max - MODEL_CHIP_CHROME - MODEL_CHIP_EFFORT_GAP - name_width).max(0.0);
        if effort_max >= MODEL_CHIP_EFFORT_MIN {
            return (
                name,
                Some(truncate_with_ellipsis(
                    &effort,
                    CONTROL_TEXT_SIZE,
                    1.2,
                    effort_max,
                )),
            );
        }
        // The effort cannot carry a readable word any more: it goes first, and
        // the name gets the whole cap.
        return (fit_model_chip_name(&name, max), None);
    }
    (fit_model_chip_name(&name, max), None)
}

/// Ellipsize the model name into whatever the chip's cap leaves for it.
fn fit_model_chip_name(name: &str, max_width: f32) -> String {
    let name_max = (max_width - MODEL_CHIP_CHROME).max(0.0);
    truncate_with_ellipsis(name, CONTROL_TEXT_SIZE, 1.2, name_max)
}

/// One row of the default attach (`+`) menu: the host callback it fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttachAction {
    Files,
    Image,
    Link,
    Code,
    Voice,
}

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
    /// The instructions drawn in one fixed row **under** the composer card,
    /// aligned to the card's trailing edge. An empty list draws nothing but the
    /// row still reserves its height, so the composer never moves when a hint
    /// appears, disappears or is re-worded.
    hints: Vec<ShortcutHint>,
    /// Whether the host opts into the instruction row. A surface that ships
    /// hints reserves the row's height (`COMPOSER_HINTS_ROW_HEIGHT`) even while
    /// the list is empty; a surface that never asks for hints pays nothing.
    hints_enabled: bool,
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
    /// The environment the draft's turn will run on, drawn as the icon-only
    /// footer control (owner: the environment may move into the footer now that
    /// the top context band is gone). `None` draws no control.
    harness_label: Option<String>,
    /// The glyph beside the environment label: T3 Code's computer for a local
    /// environment and its cloud for a worker. The host names it, so the pill
    /// says where the work runs as well as what it is called; the default is
    /// the local environment's mark.
    harness_icon: &'static str,
    harness_menu_items: Vec<PopupMenuItem>,
    harness_menu_open: Rc<RefCell<bool>>,
    on_select_harness_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
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
    /// The attach (`+`) menu's app-owned open flag and keyboard highlight. The
    /// menu is a DSH-style card anchored above the `+`, not an immediate
    /// picker: the host owns both cells so the card survives the per-frame
    /// rebuild.
    attach_menu_open: Rc<RefCell<bool>>,
    attach_menu_highlight: Rc<RefCell<Option<usize>>>,
    /// The rows the expandable attach menu lists. The default set is derived
    /// from the callbacks the host wired (files, image, link, code, voice), so
    /// a host that wires only `with_on_attach` still gets a working card with
    /// its one row.
    attach_menu_items: Vec<PopupMenuItem>,
    /// A taken row of a host-supplied attach menu, by index. `None` on the
    /// default set, whose rows dispatch straight to the host's callbacks.
    on_select_attach_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    /// The panel radius the card's bottom band carries, when the host draws the
    /// card as the pane's own bottom band instead of a floating card (the
    /// terminal surface). `None` keeps the floating card: the surface gutter on
    /// every side and the card's own radius on all four corners.
    band_bottom_radius: Option<f32>,
    /// The card's width from the last layout pass, so a value built before the
    /// layout (the model chip) can cap itself to the row it will sit in. The
    /// element is rebuilt inside `layout`, so the current pass's width is the
    /// one the chip is built with.
    layout_width: Option<f32>,
    /// The bounds the model card is placed inside: the composer card's width
    /// (published in layout) and its painted rect (published in paint). The
    /// card is anchored above the chip and clamps into this surface the way
    /// DSH's portaled menu clamps to the layout viewport.
    model_tray_bounds: ModelTrayBounds,
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

/// 401: the approval card (and its trigger) is queued on the frame's last
/// paint layer while open, so it sits above every other composer menu.
struct DeferredApprovalCard {
    inner: Rc<RefCell<Box<dyn Element>>>,
    open: Rc<RefCell<bool>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl DeferredApprovalCard {
    fn new(inner: Box<dyn Element>, open: Rc<RefCell<bool>>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(inner)),
            open,
            size: None,
            origin: None,
        }
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for DeferredApprovalCard {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.borrow_mut().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.borrow_mut().paint(origin, ctx, app);
        if *self.open.borrow() {
            app.hover_chips
                .borrow_mut()
                .push_shared(origin, Rc::clone(&self.inner));
        }
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
        self.inner.borrow_mut().dispatch_event(event, ctx, app)
    }
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            value: Rc::new(RefCell::new(String::new())),
            caret: Rc::new(RefCell::new(0)),
            placeholder: String::from(AGENT_COMPOSER_PLACEHOLDER),
            attachments: Vec::new(),
            model_label: None,
            agent_harness: None,
            focused: false,
            disabled: false,
            waiting: false,
            queued_prompt_present: false,
            blur_on_outside_click: true,
            hints: Vec::new(),
            hints_enabled: false,
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
            attach_menu_open: Rc::new(RefCell::new(false)),
            attach_menu_highlight: Rc::new(RefCell::new(None)),
            attach_menu_items: Vec::new(),
            on_select_attach_item: None,
            band_bottom_radius: None,
            layout_width: None,
            model_tray_bounds: ModelTrayBounds::new(),
            model_index: Rc::new(RefCell::new(0)),
            vim: None,
            clipboard: None,
            anchor: None,
            harness_label: None,
            harness_icon: "environment-computer",
            harness_menu_items: Vec::new(),
            harness_menu_open: Rc::new(RefCell::new(false)),
            on_select_harness_item: None,
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

    /// Retired: the owner removed the directory control from the composer, so
    /// the shared input no longer has a folder band ("nu mai oferim control pe
    /// foldere"). The call sites still pass their labels while the surrounding
    /// lanes are reworked; they are accepted and ignored here.
    #[doc(hidden)]
    pub fn with_path_label(self, _path: impl Into<String>) -> Self {
        self
    }

    /// Retired with [`Self::with_path_label`]: the single composer names no
    /// bound SSH session over its editor. The argument is accepted and ignored.
    #[doc(hidden)]
    pub fn with_ssh_session(self, _session: Option<goble_core::ssh_command::SshSession>) -> Self {
        self
    }

    /// Label for the environment (medium) control in the composer's footer: the
    /// environment the draft's turn will run on. The host draws it only where
    /// the environment is the user's to choose — a conversation running on a
    /// worker — so a host that passes no label draws no control.
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

    /// Retired: the working-directory dropdown left the composer with the
    /// folder band. Accepted and ignored so the call sites keep compiling.
    #[doc(hidden)]
    pub fn with_dir_menu<F: FnMut(usize) + 'static>(
        self,
        _items: Vec<PopupMenuItem>,
        _open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        let _ = callback;
        self
    }

    /// Retired with [`Self::with_dir_menu`].
    #[doc(hidden)]
    pub fn with_dir_menu_scroll(self, _scroll: PanelScroll) -> Self {
        self
    }

    /// Retired with [`Self::with_dir_menu`].
    #[doc(hidden)]
    pub fn with_dir_menu_highlight(self, _highlight: Rc<RefCell<Option<usize>>>) -> Self {
        self
    }

    /// Retired: the branch control left the composer with the folder band.
    /// Accepted and ignored.
    #[doc(hidden)]
    pub fn with_branch_label(self, _label: impl Into<String>) -> Self {
        self
    }

    /// Retired with [`Self::with_branch_label`].
    #[doc(hidden)]
    pub fn with_branch_menu<F: FnMut(usize) + 'static>(
        self,
        _items: Vec<PopupMenuItem>,
        _open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        let _ = callback;
        self
    }

    /// Retired with [`Self::with_branch_label`].
    #[doc(hidden)]
    pub fn with_branch_menu_highlight(self, _highlight: Rc<RefCell<Option<usize>>>) -> Self {
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
        if !self.focused
            || !*self.model_menu_open.borrow()
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

    /// Draw the card as the pane's own bottom band instead of a floating card.
    ///
    /// The terminal surface asks for this (owner: the shell bar must not float
    /// as an inset card). The outer card then runs the full width of the surface
    /// it is mounted in — flush with the pane's sides and bottom, with no
    /// surface gutter — its top corners are square against the transcript above
    /// it, and only its bottom corners carry the panel's own radius,
    /// `panel_radius` (the pane card's radius). The inner input segment keeps
    /// its radius and its 6pt casing, so it stretches with the outer box and
    /// still reads as a rounded field inside the band.
    ///
    /// The band lays out the same height as the floating card: the gutter the
    /// floating card kept under itself becomes the band's bottom casing
    /// ([`COMPOSER_BAND_BOTTOM_INSET`]), so a terminal bar and an agent input
    /// stay identical in height (`COMPOSER_BAR_RESERVED_HEIGHT`) and the shell
    /// pane's pty reservation does not change. A host that leaves this off keeps
    /// the floating card, which is what every agent surface draws.
    pub fn with_bottom_band(mut self, panel_radius: f32) -> Self {
        self.band_bottom_radius = Some(panel_radius.max(0.0));
        self
    }

    /// The instructions to draw in the input's own bottom row, under the editor
    /// and the action row. The strip that belongs over the input is the host's;
    /// this is the one entry the host keeps inside the input.
    pub fn with_hints(mut self, hints: Vec<ShortcutHint>) -> Self {
        self.hints = hints;
        // The row is reserved from the first time a surface ships hints, even
        // if the current list is empty: its height is the card's contract.
        self.hints_enabled = true;
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

    /// Share the attach (`+`) menu's open flag with the app. The composer is
    /// rebuilt every frame, so the card stays up only while this cell says so;
    /// a host that never passes one gets a composer that opens the rows in its
    /// own session only (fine for tests and one-shot previews).
    pub fn with_attach_menu_open(mut self, open: Rc<RefCell<bool>>) -> Self {
        self.attach_menu_open = open;
        self
    }

    /// Share the attach menu's keyboard highlight with the app, so Up/Down and
    /// Enter keep working across the per-frame rebuild the way the model card's
    /// highlight does.
    pub fn with_attach_menu_highlight(mut self, highlight: Rc<RefCell<Option<usize>>>) -> Self {
        self.attach_menu_highlight = highlight;
        self
    }

    /// Replace the attach menu's rows. The default set is derived from the
    /// callbacks the host wired — files, image, link, code, voice — so a host
    /// with a custom set (a shorter, product-specific card) can name its own.
    pub fn with_attach_menu_items(mut self, items: Vec<PopupMenuItem>) -> Self {
        self.attach_menu_items = items;
        self
    }

    /// The callback a row of a host-supplied attach menu reports through. The
    /// default menu (built from `on_attach`/`on_image`/`on_link`/`on_code`/
    /// `on_voice`) does not need it: each row already routes to its own host
    /// callback.
    pub fn with_on_select_attach_item<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select_attach_item = Some(Rc::new(RefCell::new(callback)));
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

    /// The instruction row: one fixed-height line of shortcut hints, drawn
    /// **under** the composer card and aligned to its trailing edge (owner:
    /// "Enter send, Cmd+Enter send to cloud, tasks…" belong below the card, to
    /// the right). A spacer takes the leading space so the row never paints
    /// over the card and never changes the card's measured height.
    ///
    /// The element is [`ComposerHintsRow`], which is exactly
    /// [`COMPOSER_HINTS_ROW_HEIGHT`] tall whether it holds one entry, three or
    /// none; the card under it therefore has one height for every draft.
    fn instructions_row(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        if !self.hints_enabled {
            return None;
        }
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Spacer::new().finish())
            .with_child(ComposerHintsRow::new(self.hints.clone()).finish())
            .finish();
        let _ = app;
        Some(
            ConstrainedBox::new(row)
                .with_min_height(COMPOSER_HINTS_ROW_HEIGHT)
                .finish(),
        )
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
        if left.is_empty()
            && right.is_none()
            && self.footer_action.is_none()
            && !submits
            && !self.hints_enabled
        {
            return None;
        }
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        // The row reserves its height even when it holds no control at all: the
        // footer's 32pt is what keeps the card — and the height a shell pane
        // reserves for its pty bar — stable now that the send circle is gone.
        // The invisible zero-width strut is the tallest child of the row's wrap,
        // so it is the row's cross-axis height.
        let mut left = left;
        left.push(
            Empty::new()
                .with_size(vec2f(0.0, COMPOSER_FOOTER_HEIGHT))
                .finish(),
        );
        // 439: one line, never a wrapped second row. The model truncates and
        // the approval control drops to its icon before anything wraps.
        let left_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_children(left)
            .finish();
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(Expanded::new(left_row).finish());
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

    /// The environment control in the composer's footer: icon-only, naming
    /// where the draft's turn runs (a computer, a cloud). The owner allowed the
    /// old top-band pill to move down here, so the single composer keeps the
    /// environment without a second context row. The host's label is the
    /// tooltip, and the tray opens leftwards from the control's edge.
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
                // The control lives at the footer's trailing end, so the tray
                // opens above it and grows leftwards (end-aligned), keeping the
                // whole casing inside the composer card.
                .with_position(PopupMenuPosition::AboveEnd)
                .with_max_width(ENVIRONMENT_MENU_WIDTH)
                .with_on_select(move |idx| (cb.borrow_mut())(idx))
                .finish(),
        )
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
        // DSH's seat names the model with the catalogue's own `model.name`
        // (`ModelSelect.tsx` `currentChoice.model.name`), not the id the app
        // stores; a host label is the fallback when the catalogue no longer
        // holds the model, and an empty one still says nothing is configured.
        let catalogue_label = self.model_tray.as_ref().and_then(|tray| {
            tray.harnesses
                .iter()
                .flat_map(|group| group.models.iter())
                .find(|model| model.id == tray.model)
                .map(|model| model.name.clone())
                .filter(|name| !name.trim().is_empty())
        });
        let provider = self.model_tray.as_ref().and_then(|tray| {
            tray.harnesses
                .iter()
                .find(|group| group.models.iter().any(|model| model.id == tray.model))
                .map(|group| group.label.clone())
                .filter(|name| !name.trim().is_empty())
        });
        let text = match catalogue_label {
            Some(name) => match provider {
                Some(provider) if !name.starts_with(&provider) => format!("{provider} · {name}"),
                _ => name,
            },
            None if label.trim().is_empty() => MODEL_NOT_CONFIGURED.to_string(),
            None => label,
        };
        let open = *self.model_menu_open.borrow()
            || self
                .model_tray
                .as_ref()
                .is_some_and(|tray| *tray.open.borrow());
        // The effort the chip shows: the level in force, or the provider default
        // when the model offers levels but none is set — DSH's caption after the
        // model name.
        let effort = self.model_tray.as_ref().and_then(|tray| {
            // The level the model in force actually offers. A level left over
            // from another model (or an id the catalogue no longer lists) is
            // not a claim the chip may make; the pane then reads its default.
            let levels = tray
                .harnesses
                .iter()
                .flat_map(|group| group.models.iter())
                .find(|model| model.id == tray.model)
                .map(|model| model.thinking.as_slice())
                .unwrap_or(&[]);
            if levels.is_empty() {
                return None;
            }
            if tray.level.trim().is_empty() {
                Some("Default".to_string())
            } else if levels.iter().any(|level| level == &tray.level) {
                Some(crate::elements::level_label(&tray.level))
            } else {
                Some("Default".to_string())
            }
        });
        // DSH caps the seat at 360pt, and the row itself caps it further on a
        // narrow pane: a chip wider than the footer would paint out of the card.
        let chip_cap = self
            .layout_width
            .map(|width| {
                (width - MODEL_CHIP_FOOTER_INSET)
                    .min(MODEL_CHIP_MAX_WIDTH)
                    .max(MODEL_CHIP_CHROME + MODEL_CHIP_EFFORT_MIN)
            })
            .unwrap_or(MODEL_CHIP_MAX_WIDTH);
        let (text, effort) = fit_model_chip(text, effort, chip_cap);
        // DSH's chip: the model name in the label tone, the effort in the caption
        // tone, a 4pt gap and a 12pt chevron that flips up while the card is
        // open. Its label tone does not change with the card (`ModelSelect.module
        // .css` keeps `label-secondary`), so the chip states only the seat.
        let mut content = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0)
            .with_child(
                Text::new(text)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(CONTROL_TEXT_SIZE)
                    .finish(),
            );
        if let Some(effort) = effort {
            content = content.with_child(
                Text::new(effort)
                    .with_color(caption_color(app))
                    .with_font_size(CONTROL_TEXT_SIZE)
                    .finish(),
            );
        }
        // 303: in the composer the model is a plain text selector, with no
        // pill, outline, chevron or hover fill.
        let content = Padding::new(content.finish(), EdgeInsets::new(4.0, 0.0, 4.0, 0.0)).finish();
        let button = ComposerButton::new(content)
            .with_height(28.0)
            .with_h_pad(0.0)
            .with_radius(0.0)
            .with_outline(false)
            .with_hover_fill(ColorToken::ContentBg);
        // Opening the card makes this composer the keyboard's owner as well as
        // the pointer's. Without it, a chip pressed in a pane that had lost
        // focus opened a card whose Up/Down/Enter went nowhere (the composer
        // routes the card's keys only while it is focused).
        let focus_on_open = self.on_focus_change.clone();
        let button = if let Some(tray) = self.model_tray.clone() {
            // The chip toggles the host's open flag; the card it opens is the
            // one the composer wraps the chip in below.
            let dismissed = self.slash_dismissed.clone();
            let focus = focus_on_open.clone();
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
                        if let Some(cb) = focus.as_ref() {
                            (cb.borrow_mut())(true);
                        }
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
                    let focus = focus_on_open.clone();
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
                                if let Some(cb) = focus.as_ref() {
                                    (cb.borrow_mut())(true);
                                }
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
        // chip is wrapped in the menu element, which draws the card above it and
        // clamps it inside this composer's card.
        match self.model_tray.as_ref() {
            Some(tray) => Box::new(
                tray.clone()
                    .with_bounds(self.model_tray_bounds.clone())
                    .popup(trigger),
            ),
            None => trigger,
        }
    }

    /// The approval-mode control: the mode the pane runs under, next to the
    /// model it runs on. The control opens the four-mode menu; Shift+Tab cycles
    /// them without opening it. The label is the mode's own word, so the pane
    /// never has to open the menu to say what policy is in force.
    fn approval_control(&self, app: &AppContext, mode: ApprovalMode) -> Box<dyn Element> {
        // 394: one neutral control, the mode's icon and label in primary ink.
        // 439: on a narrow footer the label drops and only the icon + tooltip
        // stay, so the row never wraps.
        let compact = self.layout_width.is_some_and(|width| width < 460.0);
        let mut pill =
            ContextPill::new(mode.icon(), if compact { "" } else { mode.display_label() })
                .with_label_color(ColorToken::Text)
                .with_hover_fill(ColorToken::ContentBg)
                .with_tooltip(format!(
                    "Approval mode · {} · shift+tab to cycle",
                    mode.display_description()
                ))
                .with_tray_side(PillTraySide::Above)
                .with_menu_max_width(360.0);
        if let Some(cb) = self.on_select_approval_mode.clone() {
            let items = ApprovalMode::DISPLAY_ORDER
                .iter()
                .map(|option| {
                    let mut item = PopupMenuItem::new(option.display_label())
                        .with_icon(option.icon())
                        .with_description(option.display_description());
                    if *option == mode {
                        item = item.selected();
                    }
                    item
                })
                .collect();
            pill = pill
                .with_menu(items, self.approval_menu_open.clone(), move |index| {
                    let chosen = ApprovalMode::DISPLAY_ORDER
                        .get(index)
                        .copied()
                        .unwrap_or(ApprovalMode::Manual);
                    (cb.borrow_mut())(chosen)
                })
                .with_highlight_index(self.approval_menu_highlight.clone());
        }
        DeferredApprovalCard::new(pill.finish(app), self.approval_menu_open.clone()).finish()
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
        if key != "Tab" || !modifiers.shift || modifiers.command || modifiers.ctrl || modifiers.alt
        {
            return false;
        }
        (cb.borrow_mut())(mode.next());
        true
    }

    /// The rows and actions of the attach (`+`) card. The default set is
    /// derived from the callbacks the host wired; a host that supplied its own
    /// rows gets a `custom` flag and no built-in actions (its select callback
    /// is the only route).
    fn attach_menu_rows(&self) -> (Vec<PopupMenuItem>, Vec<AttachAction>, bool) {
        let mut actions: Vec<AttachAction> = Vec::new();
        let mut items = self.attach_menu_items.clone();
        let custom = !items.is_empty();
        if !custom {
            if self.on_attach.is_some() {
                items.push(PopupMenuItem::new("Add files").with_icon("folder"));
                actions.push(AttachAction::Files);
            }
            if self.on_image.is_some() {
                items.push(PopupMenuItem::new("Add image").with_icon("image"));
                actions.push(AttachAction::Image);
            }
            if self.on_link.is_some() {
                items.push(PopupMenuItem::new("Add link").with_icon("link"));
                actions.push(AttachAction::Link);
            }
            if self.on_code.is_some() {
                items.push(PopupMenuItem::new("Add code").with_icon("code"));
                actions.push(AttachAction::Code);
            }
            if self.on_voice.is_some() {
                items.push(PopupMenuItem::new("Voice input").with_icon("mic"));
                actions.push(AttachAction::Voice);
            }
        }
        (items, actions, custom)
    }

    /// Fire the host callback a taken attach row names. The card reports a row
    /// index; a default row maps to one of the host's gestures, a custom row to
    /// the host's own select callback.
    fn fire_attach_row(&self, actions: &[AttachAction], custom: bool, index: usize) {
        if custom {
            if let Some(cb) = self.on_select_attach_item.as_ref() {
                (cb.borrow_mut())(index);
            }
            return;
        }
        let Some(action) = actions.get(index).copied() else {
            return;
        };
        let callback = match action {
            AttachAction::Files => self.on_attach.clone(),
            AttachAction::Image => self.on_image.clone(),
            AttachAction::Link => self.on_link.clone(),
            AttachAction::Code => self.on_code.clone(),
            AttachAction::Voice => self.on_voice.clone(),
        };
        if let Some(cb) = callback {
            (cb.borrow_mut())();
        }
    }

    /// Keys the open attach card takes before the editor sees them: Up/Down move
    /// the highlight, Enter/Tab take the row, Escape puts the card away. The
    /// popup draws the highlight from the app-owned cell, so the two never
    /// disagree across the per-frame rebuild.
    fn handle_attach_menu_key(&mut self, event: &DispatchedEvent) -> bool {
        if !self.focused || !*self.attach_menu_open.borrow() {
            return false;
        }
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        if modifiers.command || modifiers.ctrl || modifiers.alt {
            return false;
        }
        let (items, actions, custom) = self.attach_menu_rows();
        if items.is_empty() {
            return false;
        }
        match key.as_str() {
            "ArrowDown" | "ArrowUp" => {
                let len = items.len();
                let current = self
                    .attach_menu_highlight
                    .borrow()
                    .unwrap_or(0)
                    .min(len - 1);
                let next = if key == "ArrowDown" {
                    (current + 1) % len
                } else {
                    (current + len - 1) % len
                };
                *self.attach_menu_highlight.borrow_mut() = Some(next);
                true
            }
            "Enter" | "Tab" if !modifiers.shift => {
                let index = self
                    .attach_menu_highlight
                    .borrow()
                    .unwrap_or(0)
                    .min(items.len() - 1);
                *self.attach_menu_open.borrow_mut() = false;
                self.fire_attach_row(&actions, custom, index);
                true
            }
            "Escape" => {
                *self.attach_menu_open.borrow_mut() = false;
                true
            }
            _ => false,
        }
    }

    /// The attach (`+`) control: the footer's leading glyph, which opens a
    /// DeepSeek-Harness-style card above itself instead of firing a picker
    /// straight away. The rows are the host's real gestures — files, an image,
    /// a link, a code block, voice input — and each one routes to the callback
    /// the host wired, so a card never offers a row that does nothing.
    ///
    /// The card is a [`PopupMenu`], the same surface the environment and the
    /// directory trays use before their retirement: anchored above the `+`,
    /// rounded, one 32pt row per action, a highlight the keyboard and the
    /// pointer share, and an outside press that puts it away.
    fn attach_control(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let (items, actions, custom) = self.attach_menu_rows();
        if items.is_empty() {
            return None;
        }
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
        .finish();
        let trigger = Tooltip::new(attach, "Add context")
            .with_position(TooltipPosition::Above)
            .finish();
        // The click path and the key path both end in the same host callback,
        // so a row taken with Enter and a row taken with the pointer agree.
        let actions_for_click = actions;
        let custom_for_click = custom;
        let on_select_attach = self.on_select_attach_item.clone();
        let on_attach = self.on_attach.clone();
        let on_image = self.on_image.clone();
        let on_link = self.on_link.clone();
        let on_code = self.on_code.clone();
        let on_voice = self.on_voice.clone();
        let open = self.attach_menu_open.clone();
        Some(
            PopupMenu::new(trigger, items)
                .with_open(self.attach_menu_open.clone())
                .with_highlight_index(self.attach_menu_highlight.clone())
                .with_position(PopupMenuPosition::Above)
                .with_max_width(240.0)
                .with_on_select(move |index| {
                    *open.borrow_mut() = false;
                    if custom_for_click {
                        if let Some(cb) = on_select_attach.as_ref() {
                            (cb.borrow_mut())(index);
                        }
                        return;
                    }
                    let callback = match actions_for_click.get(index).copied() {
                        Some(AttachAction::Files) => on_attach.clone(),
                        Some(AttachAction::Image) => on_image.clone(),
                        Some(AttachAction::Link) => on_link.clone(),
                        Some(AttachAction::Code) => on_code.clone(),
                        Some(AttachAction::Voice) => on_voice.clone(),
                        None => None,
                    };
                    if let Some(cb) = callback {
                        (cb.borrow_mut())();
                    }
                })
                .finish(),
        )
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
        if let Some(attach) = self.attach_control(app) {
            children.push(attach);
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

        let slash_open = self.slash_band_open();

        // The input segment's own column: the draft's attachments, the editor
        // and the footer row. The instruction row is **not** in here any more:
        // the owner moved it under the card, where it cannot change the card's
        // height.
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

        // The footer row: the borderless controls on the left and the stop
        // circle on the right, both inside the input segment.
        if let Some(footer) = self.footer_row(app) {
            input = input.with_child(footer);
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

        // One composer, one card, one fixed instruction row under it. The card
        // is the input segment in its casing; the row is right-aligned under
        // the card, outside its painted surface, so no draft, hint or wrap can
        // change the card's height.
        let card: Box<dyn Element> = match self.band_bottom_radius {
            Some(panel_radius) => {
                let mut inner = Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_child(input_segment);
                if let Some(hints) = self.instructions_row(app) {
                    inner = inner.with_spacing(COMPOSER_HINTS_GAP).with_child(
                        // The band's own casing is 6; six more points put the
                        // row's trailing edge on the input segment's edge.
                        Padding::new(hints, EdgeInsets::new(0.0, 0.0, 6.0, 0.0)).finish(),
                    );
                }
                self.into_band(inner.finish(), app, panel_radius)
            }
            None => {
                let card = self.into_card(input_segment, app);
                match self.instructions_row(app) {
                    Some(hints) => Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_child(card)
                        .with_child(
                            Padding::new(
                                hints,
                                EdgeInsets::new(
                                    COMPOSER_CARD_GUTTER,
                                    COMPOSER_HINTS_GAP,
                                    COMPOSER_CARD_GUTTER,
                                    COMPOSER_CARD_GUTTER,
                                ),
                            )
                            .finish(),
                        )
                        .finish(),
                    None => card,
                }
            }
        };
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

    /// Wrap the input segment in the composer's card: radius 26, the `Surface`
    /// fill, no outline (owner feedback: the rich input's outer ring reads
    /// cleaner without a border), and a 6pt casing that holds the input
    /// segment. The card and its segment are flat surfaces separated by tone
    /// alone. There is no context band any more: the card is just the input,
    /// which is the owner's "un singur composer".
    ///
    /// The card sits in the surface's own gutter ([`COMPOSER_CARD_GUTTER`]) on
    /// every host, so the shell bar, the agent input and the thread input are
    /// the same object in the same place. When the host ships the instruction
    /// row, the bottom gutter moves into that row's padding, so the whole
    /// element has one height ([`COMPOSER_BAR_RESERVED_HEIGHT`]) and the card
    /// itself never changes.
    fn into_card(&self, content: Box<dyn Element>, app: &AppContext) -> Box<dyn Element> {
        let card = Container::new(content)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_corner_radius(COMPOSER_CARD_RADIUS)
            .with_padding(EdgeInsets::new(
                COMPOSER_CARD_INSET,
                COMPOSER_CARD_INSET,
                COMPOSER_CARD_INSET,
                COMPOSER_CARD_INSET,
            ))
            .finish();
        // The row under the card owns the bottom gutter when it is present, so
        // the card sits flush over it and the row is the element's last line.
        let bottom = if self.hints_enabled {
            0.0
        } else {
            COMPOSER_CARD_GUTTER
        };
        Padding::new(
            card,
            EdgeInsets::new(COMPOSER_CARD_GUTTER, 0.0, COMPOSER_CARD_GUTTER, bottom),
        )
        .finish()
    }

    /// Wrap the input segment (and the instruction row under it) in the card's
    /// bottom-band shape instead of the floating card: the `Surface` fill runs
    /// the surface's full width, square on top, with the panel's radius
    /// ([`ChatComposer::with_bottom_band`]) on its bottom corners, and the inner
    /// input segment keeps its casing ([`COMPOSER_CARD_INSET`]) on the sides and
    /// its rounded shape. The band's bottom casing is
    /// [`COMPOSER_BAND_BOTTOM_INSET`]: the folded gutter keeps the whole element
    /// the same height as the floating card plus its instruction row, so a
    /// terminal bar and an agent input stay identical and the shell pane's
    /// reserved bar height does not move.
    fn into_band(
        &self,
        content: Box<dyn Element>,
        app: &AppContext,
        panel_radius: f32,
    ) -> Box<dyn Element> {
        BandCard::new(content)
            .with_background(app.theme.color(ColorToken::Surface))
            .with_bottom_radius(panel_radius)
            .with_padding(EdgeInsets::new(
                COMPOSER_CARD_INSET,
                COMPOSER_CARD_INSET,
                COMPOSER_CARD_INSET,
                COMPOSER_BAND_BOTTOM_INSET,
            ))
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
        // The card's width is known before the tree is built, so the controls
        // that must cap themselves to the row (the model chip's 360pt value) and
        // the card that clamps to this surface both see this pass's width. A
        // floating card sits in the surface gutter; the bottom band runs the
        // whole surface, so it claims the constraint's full width.
        let card_width = if constraint.max.x.is_finite() {
            let gutter = if self.band_bottom_radius.is_some() {
                0.0
            } else {
                COMPOSER_CARD_GUTTER
            };
            (constraint.max.x - gutter * 2.0).max(0.0)
        } else {
            f32::INFINITY
        };
        self.layout_width = Some(card_width);
        self.rebuild(app);
        // DSH sizes its card against `min(..., 100vw - 24px)`: publish the
        // composer card's width less the two 12pt viewport insets.
        self.model_tray_bounds
            .set_width((card_width - 2.0 * MODEL_MENU_VIEWPORT_INSET).max(0.0));
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        // Publish the card's painted rect before the children paint: the model
        // card is anchored to the chip and clamps into this surface, so it needs
        // the rect of the frame it is about to be drawn in.
        if let Some(size) = self.size {
            let gutter = if self.band_bottom_radius.is_some() {
                0.0
            } else {
                COMPOSER_CARD_GUTTER
            };
            self.model_tray_bounds.set_rect(rectf(
                origin.x + gutter,
                origin.y,
                (size.x - gutter * 2.0).max(0.0),
                size.y,
            ));
        }
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
        // The attach card owns the keys that take a row from it while it is
        // up, before the editor can use them for the draft.
        if self.handle_attach_menu_key(event) {
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
