//! The composer's model selector: a DeepSeek Harness-style two-level menu
//! anchored to the model chip in the input's footer.
//!
//! DSH's `ModelSelect` (`@deepseek-ai/dsh-client-ui-model-selection`) is a
//! dropdown, not a band: the trigger is the model chip, and the card above it
//! opens on a root pane with one **Model** row and one **Effort** row (label,
//! current value, a right chevron); each drills into its own list — the models
//! grouped under the provider that owns them, and the thinking levels — with the
//! value in force marked by a trailing check. This module is that card, wired to
//! the pane's real catalogue ([`ModelTrayHarness`] groups of [`ModelTrayModel`]s)
//! and the app's own selection callbacks.
//!
//! The card is drawn by the element that wraps the chip, so it is anchored to
//! the chip the way DSH anchors its portaled card (above it, right-aligned),
//! and it paints outside the composer card without being in the host's layout.
//! Like every menu here it never holds the keyboard: the composer routes the
//! keys ([`handle_tray_key`]), the element draws what the shared
//! [`ModelTrayState`] says and reports what the pointer chose.
//!
//! The tree is rebuilt every frame, so the state the card and the key routing
//! must agree on — pane, highlighted row, highlighted level — lives in the
//! app-owned [`ModelTrayState`]. The host's flat model list
//! ([`super::slash_menu::ModelMenu`]) stays as the slot the view draws for hosts
//! that do not wire this menu.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::elements::conversation_list_item::truncate_with_ellipsis;
use crate::elements::icon::icon_atlas_name;
use crate::elements::interactive::contains;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, MenuClose, MenuId, MenuPointer, PaintContext,
    Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

/// How many model rows the card draws at once before it windows around the
/// selection.
pub const MODEL_TRAY_ROWS: usize = 8;

/// The label the composer's model card registers under in the window's shared
/// open-menu state. The card is one of the window's menus even though it is not
/// a [`PopupMenu`](crate::elements::PopupMenu): it owns the pointer over its
/// surface, closes on an outside press and Escape, and opening it closes any
/// other menu.
pub const MODEL_TRAY_MENU_ID: MenuId = "composer.model";

/// The width the card's root pane keeps: DSH's 240pt settings-menu casing
/// (`ModelSelect.module.css` `.settingsMenu`). A window narrower than the card
/// shrinks it instead of letting it run off the edge, exactly as DSH does with
/// `min(240px, 100vw - 24px)`.
const ROOT_WIDTH: f32 = 240.0;
const MIN_WIDTH: f32 = 420.0;
const MAX_WIDTH: f32 = 760.0;
/// The design cap on the card's height (DSH clamps it to the window:
/// `max-height: min(360px, calc(100dvh - 96px))`).
const MAX_HEIGHT: f32 = 560.0;
/// The air between the card and the chip it hangs from (DSH:
/// `bottom: calc(anchor(top) + 8px)`).
const CARD_GAP: f32 = 8.0;
/// DSH clamps its portaled card 12pt inside the layout viewport (`.menu`
/// `left: 12px` / `right: 12px`); the composer card is this surface's
/// viewport, so the card keeps the same inset inside it. The composer publishes
/// its card width minus twice this inset, so the card also max-sizes against
/// DSH's `min(240px/420px, 100vw - 24px)` bounds.
pub const MODEL_MENU_VIEWPORT_INSET: f32 = 12.0;
/// DSH's menu surface (`MenuSurface.module.css`: `--dsw-radius-lg` = 16pt) and
/// its 4pt casing padding (`ModelSelect.module.css` `.menu { padding: 4px }`).
const CARD_RADIUS: f32 = 16.0;
const CARD_PADDING: f32 = 4.0;
/// A row/cell inside the surface: DSH `--dsw-radius-md` = 12pt
/// (`.option`/`.cell { border-radius: var(--dsw-radius-md) }`).
const ROW_RADIUS: f32 = 12.0;

/// A cell/row: the shared menu row's 32pt rhythm, its content inset, the gap
/// between its boxes and the glyph gaps. DSH's model menu rows are 34pt; the
/// composer's other menus use 32 + a 2pt gap, so the model card follows the
/// menu material it shares rather than the figma page's row height.
const ROW_HEIGHT: f32 = 32.0;
const ROW_SPACING: f32 = 2.0;
/// DSH `.option` padding-x (5px 7px rows); the root pane's `.cell` uses
/// [`CELL_PAD_X`] (0 8px).
const ROW_PAD_X: f32 = 7.0;
const CELL_PAD_X: f32 = 8.0;
const ROW_GAP: f32 = 6.0;
const CHECK_SIZE: f32 = 14.0;
const CHEVRON_SIZE: f32 = 12.0;

/// A provider/group heading over its rows: 4 above, the 16pt line, 2 below;
/// groups are 3 apart.
const GROUP_HEIGHT: f32 = 22.0;
const GROUP_PAD_X: f32 = 7.0;
const GROUP_PAD_TOP: f32 = 4.0;
const GROUP_GAP: f32 = 3.0;

/// A status row inside the card.
const STATUS_HEIGHT: f32 = 28.0;

const LABEL_FONT_SIZE: f32 = 13.0;
const CELL_LINE_HEIGHT: f32 = 20.0;
const MODEL_LINE_HEIGHT: f32 = 18.0;
const GROUP_FONT_SIZE: f32 = 11.0;
const GROUP_LINE_HEIGHT: f32 = 16.0;

/// The root pane's two dictionary labels.
pub const MODEL_CELL_LABEL: &str = "Model";
pub const EFFORT_CELL_LABEL: &str = "Effort";
/// The effort row's value when the pane runs on the model's default level.
pub const EFFORT_DEFAULT_LABEL: &str = "Default";
/// The two empty states the card can draw.
pub const NO_MODELS: &str = "No models configured";
pub const NO_EFFORTS: &str = "This model provides no reasoning effort levels.";

/// One provider a pane can run, as the card recognises it: the name its group
/// heading shows and the models that provider runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelTrayHarness {
    /// The provider/harness id the models belong to (`deepseek`, `anthropic`,
    /// `goble`, …).
    pub harness: String,
    pub label: String,
    pub models: Vec<ModelTrayModel>,
}

impl ModelTrayHarness {
    pub fn new(harness: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            harness: harness.into(),
            label: label.into(),
            models: Vec::new(),
        }
    }

    pub fn with_models(mut self, models: Vec<ModelTrayModel>) -> Self {
        self.models = models;
        self
    }
}

/// One model in a provider's group: the id a turn runs on, the label the row
/// shows, and the thinking levels the model accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelTrayModel {
    pub id: String,
    pub name: String,
    pub thinking: Vec<String>,
}

impl ModelTrayModel {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            thinking: Vec::new(),
        }
    }

    pub fn with_thinking(mut self, levels: Vec<String>) -> Self {
        self.thinking = levels;
        self
    }

    /// The level the card falls back to: the first one the model offers, so a
    /// pick is always one the harness runs.
    pub fn default_level(&self) -> String {
        self.thinking.first().cloned().unwrap_or_default()
    }
}

/// What a thinking level is called in the card. The harness reports the ids it
/// runs; the row spells them the way a person reads them.
pub fn level_label(level: &str) -> String {
    match level {
        "off" => "Off".to_string(),
        "on" => "On".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

/// Which pane of the two-level menu the card shows: DSH's root menu, or one of
/// the two lists it drills into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelTrayPane {
    #[default]
    Root,
    Model,
    Effort,
}

/// Every model the card lists, in the order it draws them, each with the
/// provider that owns it. The keyboard and the hit test both work on this one
/// list, so what the arrows move and what a press takes cannot disagree.
fn flat_models(harnesses: &[ModelTrayHarness]) -> Vec<(usize, &ModelTrayModel)> {
    harnesses
        .iter()
        .enumerate()
        .flat_map(|(group, harness)| harness.models.iter().map(move |model| (group, model)))
        .collect()
}

/// The model a pane runs, when the catalogue still holds it.
fn model_by_id<'a>(harnesses: &'a [ModelTrayHarness], id: &str) -> Option<&'a ModelTrayModel> {
    flat_models(harnesses)
        .into_iter()
        .find(|(_, model)| model.id == id)
        .map(|(_, model)| model)
}

/// The card's selection, app-owned: the element is rebuilt every frame, so the
/// pane the menu is on and the rows the arrows moved to have to live outside it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelTrayState {
    pub pane: ModelTrayPane,
    /// The root pane's cell the keyboard is on: 0 = Model, 1 = Effort.
    pub cell: usize,
    /// Whether the keyboard is actually on the root pane's cell. DSH opens the
    /// root pane with focus on the trigger, so no cell is highlighted until the
    /// first arrow moves onto one (`.cell:focus-visible` only); this is that
    /// distinction, since the composer routes the keys and keeps no focus.
    pub cell_active: bool,
    /// The row of the model list the selection is on, counted across every
    /// provider: the group headings are not rows a keyboard can land on.
    pub row: usize,
    /// The provider/integration column's selected row.
    pub provider: usize,
    /// Filter text typed into the model column's search field.
    pub search: String,
    /// The row of the current model's thinking levels.
    pub level: usize,
    /// Whether the card has been placed on the pane's own model yet. The card
    /// seeds itself from the model in force the first time it is drawn, and the
    /// chip clears this when it opens so the next open starts there again.
    pub seeded: bool,
}

impl ModelTrayState {
    pub fn pane(&self) -> ModelTrayPane {
        self.pane
    }

    /// Place the card on the root pane, on the pane's model and on the level it
    /// runs at. Called once per open, from the model the composer shows.
    pub fn seed(&mut self, harnesses: &[ModelTrayHarness], model: &str, level: &str) {
        let list = flat_models(harnesses);
        self.pane = ModelTrayPane::Root;
        self.cell = 0;
        self.cell_active = false;
        self.row = list
            .iter()
            .position(|(_, candidate)| candidate.id == model)
            .unwrap_or(0);
        self.provider = list.get(self.row).map(|(group, _)| *group).unwrap_or(0);
        self.level = list
            .get(self.row)
            .and_then(|(_, candidate)| candidate.thinking.iter().position(|one| one == level))
            .unwrap_or(0);
        self.seeded = true;
        self.clamp(harnesses, model);
    }

    /// Keep every index inside the lists the card was given: the groups are
    /// rebuilt from configuration that can change between frames.
    pub fn clamp(&mut self, harnesses: &[ModelTrayHarness], model: &str) {
        let list = flat_models(harnesses);
        self.row = self.row.min(list.len().saturating_sub(1));
        self.provider = self.provider.min(harnesses.len().saturating_sub(1));
        let levels = model_by_id(harnesses, model)
            .map(|model| model.thinking.len())
            .unwrap_or(0);
        self.level = self.level.min(levels.saturating_sub(1));
        // The effort cell exists only while the model in force offers levels;
        // the keyboard never sits in a pane that is not there.
        self.cell = self.cell.min(if levels == 0 { 0 } else { 1 });
        if levels == 0 {
            self.level = 0;
        }
    }

    /// How many thinking levels the current model offers.
    pub fn level_count(&self, harnesses: &[ModelTrayHarness]) -> usize {
        self.model_at(harnesses)
            .map(|model| model.thinking.len())
            .unwrap_or(0)
    }

    /// The model the selection is on, when the list still holds it.
    pub fn model_at<'a>(&self, harnesses: &'a [ModelTrayHarness]) -> Option<&'a ModelTrayModel> {
        flat_models(harnesses)
            .get(self.row)
            .map(|(_, model)| *model)
    }

    /// The thinking level the selection is on, named by the id the harness
    /// reports. Empty when the model offers none — a row that takes nothing.
    pub fn level_at(&self, harnesses: &[ModelTrayHarness]) -> String {
        self.model_at(harnesses)
            .and_then(|model| model.thinking.get(self.level))
            .cloned()
            .unwrap_or_default()
    }
}

/// What a key did to the card. The composer routes the keys and fires the host
/// callback for the outcome, so what the arrows move and what a pick applies can
/// never disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelTrayKey {
    /// The key belongs to the editor.
    Ignored,
    /// The key moved the selection (a pane switch or a row move).
    Moved,
    /// Enter/Tab on the root pane's Model cell: show the model list.
    DrillModel,
    /// Enter/Tab on the root pane's Effort cell: show the effort list.
    DrillEffort,
    /// Escape/Shift+Tab: leave the drilled pane for the root menu.
    Back,
    /// Enter/Tab on a model row: apply the model at that flat index.
    AcceptModel(usize),
    /// Enter/Tab on a thinking row: apply the level at that index, named by the
    /// id its harness reports (`off`, `on`).
    AcceptLevel(String),
    /// Escape from the root pane: put the menu away.
    Close,
}

fn wrap(index: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    ((index as isize + delta).rem_euclid(len as isize)) as usize
}

/// Route one key to the card the way DSH's `ModelSelect` does: Up/Down move the
/// highlight (root cells, model rows, effort rows, wrapping), Enter or Tab
/// drills in the root and takes a row in a list, Escape and Shift+Tab leave a
/// drilled pane first and otherwise close.
pub fn handle_tray_key(
    event: &DispatchedEvent,
    harnesses: &[ModelTrayHarness],
    current_model: &str,
    current_level: &str,
    state: &Rc<RefCell<ModelTrayState>>,
) -> ModelTrayKey {
    let DispatchedEvent::KeyDown { key, modifiers } = event else {
        return ModelTrayKey::Ignored;
    };
    // A chorded key is the host's own gesture (Cmd/Ctrl+Enter, Shift+Tab rings
    // the composer): it falls through to the composer even with the card up.
    if modifiers.command || modifiers.ctrl || modifiers.alt {
        return ModelTrayKey::Ignored;
    }
    let mut state = state.borrow_mut();
    if !state.seeded {
        state.seed(harnesses, current_model, current_level);
    }
    state.clamp(harnesses, current_model);
    let total = flat_models(harnesses).len();
    let levels = model_by_id(harnesses, current_model)
        .map(|model| model.thinking.len())
        .unwrap_or(0);
    let back = |state: &mut ModelTrayState| {
        if state.pane == ModelTrayPane::Root {
            ModelTrayKey::Close
        } else {
            // DSH hands the keyboard back to the cell that opened the pane
            // (`back`): the root opens with that cell highlighted.
            state.cell = if state.pane == ModelTrayPane::Effort {
                1
            } else {
                0
            };
            state.cell_active = true;
            state.pane = ModelTrayPane::Root;
            ModelTrayKey::Back
        }
    };
    match key.as_str() {
        "Escape" => back(&mut state),
        "Tab" if modifiers.shift => back(&mut state),
        "Enter" | "Tab" => match state.pane {
            ModelTrayPane::Root => {
                // 477: the first column is the integration list.
                state.pane = ModelTrayPane::Model;
                ModelTrayKey::DrillModel
            }
            ModelTrayPane::Model => ModelTrayKey::AcceptModel(state.row),
            ModelTrayPane::Effort => ModelTrayKey::AcceptLevel(state.level_at(harnesses)),
        },
        "ArrowUp" | "ArrowDown" => {
            let delta = if key == "ArrowDown" { 1 } else { -1 };
            match state.pane {
                ModelTrayPane::Root => {
                    let providers = harnesses.len();
                    if providers > 0 {
                        state.provider = wrap(state.provider.min(providers - 1), delta, providers);
                        let first = flat_models(harnesses)
                            .iter()
                            .position(|(group, _)| *group == state.provider)
                            .unwrap_or(0);
                        state.row = first;
                        state.level = 0;
                    }
                    state.cell_active = true;
                }
                ModelTrayPane::Model => {
                    if total > 0 {
                        state.row = wrap(state.row, delta, total);
                        // The levels belong to the model row above them.
                        state.level = 0;
                    }
                }
                ModelTrayPane::Effort => {
                    if levels > 0 {
                        state.level = wrap(state.level, delta, levels);
                    }
                }
            }
            ModelTrayKey::Moved
        }
        _ => ModelTrayKey::Ignored,
    }
}

/// What a drawn row of the panel stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelRowKind {
    /// The root pane's cell, by index (0 = Model, 1 = Effort).
    Cell(usize),
    /// A provider heading.
    Group(usize),
    /// The three-column selector's integration row.
    Provider(usize),
    /// The model column's search field.
    Search,
    /// A model row, by its flat index.
    Model(usize),
    /// A thinking row, by its index in the current model's list.
    Level(usize),
}

/// One drawn row's bounds, relative to the menu element's origin, and what it
/// stands for.
#[derive(Clone, Debug)]
struct PanelRow {
    kind: PanelRowKind,
    rect: RectF,
}

/// The drawn panel: where it sits relative to the chip and its rows.
#[derive(Clone, Debug)]
struct Panel {
    origin: Vector2F,
    size: Vector2F,
    rows: Vec<PanelRow>,
}

/// The bounds the model card is placed inside: the width of the surface it
/// opens over while the tree lays out, and that surface's rect while it paints.
///
/// DSH portals the card to the document body and clamps it to the layout
/// viewport (`left: 12px`, `max-width: min(420px, calc(100vw - 24px))`). The
/// native composer paints one frame with no portal, so the composer card is the
/// local viewport: its 12pt gutter is the inset DSH clamps to, and keeping the
/// card over the composer is what the portaled card does visually. The element
/// tree is rebuilt every frame, so the composer publishes its bounds through
/// this handle: its width during layout (before the card is measured) and its
/// rect during paint (before the card is drawn).
#[derive(Clone, Debug, Default)]
pub struct ModelTrayBounds {
    width: Rc<Cell<f32>>,
    rect: Rc<RefCell<Option<RectF>>>,
}

impl ModelTrayBounds {
    /// Bounds with nothing published yet: the card sizes to its design caps and
    /// hangs right-aligned off the chip.
    pub fn new() -> Self {
        Self {
            width: Rc::new(Cell::new(f32::INFINITY)),
            rect: Rc::new(RefCell::new(None)),
        }
    }

    /// The width of the surface the card opens over (the composer card).
    pub fn set_width(&self, width: f32) {
        self.width.set(width);
    }

    /// The painted rect of the surface the card opens over, in frame
    /// coordinates. Published before the card paints, so the card's placement
    /// sees the rect of the frame it is being drawn in.
    pub fn set_rect(&self, rect: RectF) {
        *self.rect.borrow_mut() = Some(rect);
    }

    /// The published width as a card cap, when one is finite.
    fn available_width(&self) -> Option<f32> {
        let width = self.width.get();
        width.is_finite().then(|| width.max(0.0))
    }

    /// The published painted rect, when the host paints inside a known surface.
    fn painted_rect(&self) -> Option<RectF> {
        *self.rect.borrow()
    }
}

/// The card the composer's model chip opens. It wraps the trigger the composer
/// built (the chip is the host's control: its press toggles [`Self::open`]) and
/// draws DSH's two-level menu above it.
pub struct ModelTray {
    trigger: Box<dyn Element>,
    harnesses: Rc<Vec<ModelTrayHarness>>,
    state: Rc<RefCell<ModelTrayState>>,
    open: Rc<RefCell<bool>>,
    /// The model and level in force, so the card marks them and seeds itself on
    /// them.
    current_model: String,
    current_level: String,
    credential_source: Option<String>,
    on_select_model: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_select_level: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    panel: Option<Panel>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    /// The surface the card is placed inside, when the host publishes one; a
    /// bare tray (tests, another host) sizes to its caps and right-aligns.
    bounds: Option<ModelTrayBounds>,
}

impl ModelTray {
    #[allow(clippy::too_many_arguments)]
    fn new(
        trigger: Box<dyn Element>,
        harnesses: Rc<Vec<ModelTrayHarness>>,
        state: Rc<RefCell<ModelTrayState>>,
        open: Rc<RefCell<bool>>,
        current_model: impl Into<String>,
        current_level: impl Into<String>,
        credential_source: Option<String>,
        on_select_model: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
        on_select_level: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
        bounds: Option<ModelTrayBounds>,
    ) -> Self {
        Self {
            trigger,
            harnesses,
            state,
            open,
            current_model: current_model.into(),
            current_level: current_level.into(),
            credential_source,
            on_select_model,
            on_select_level,
            panel: None,
            size: None,
            origin: None,
            bounds,
        }
    }

    /// Place the shared state on the pane's own model, once per open.
    fn seed(&self) {
        let mut state = self.state.borrow_mut();
        if state.seeded {
            return;
        }
        state.seed(&self.harnesses, &self.current_model, &self.current_level);
    }

    /// How the card is put away from outside its element: the shared registry
    /// keeps it while it is the one open, so opening another menu closes the
    /// card and an outside press clears the flag the composer reads.
    fn close_handle(&self) -> MenuClose {
        let open = Rc::clone(&self.open);
        Rc::new(RefCell::new(move || *open.borrow_mut() = false))
    }

    /// Register the card as the window's open menu, dismissing whatever menu
    /// was up before it.
    fn claim(&self, app: &AppContext) {
        if app.open_menus.id() != Some(MODEL_TRAY_MENU_ID) {
            app.open_menus.open(MODEL_TRAY_MENU_ID, self.close_handle());
        }
    }

    /// Hand the card's surface to the window and end the menu paint, so the
    /// pointer over the card belongs to it and nothing behind it hovers there.
    /// Called on every path out of `paint` once the card started painting.
    fn publish(&self, ctx: &mut PaintContext, app: &AppContext, card: RectF) {
        ctx.leave_menu_surface();
        app.open_menus.report(
            MODEL_TRAY_MENU_ID,
            self.close_handle(),
            vec![card],
            MenuPointer::Events,
        );
        if let Some(anchor) = self.bounds() {
            app.open_menus.report_anchor(MODEL_TRAY_MENU_ID, anchor);
        }
    }

    /// The first model row the list shows: the window never scrolls past the
    /// end, and the selected row always sits inside it.
    fn window_start(row: usize, total: usize) -> usize {
        let span = MODEL_TRAY_ROWS.min(total);
        if span == 0 {
            return 0;
        }
        row.saturating_sub(MODEL_TRAY_ROWS / 2)
            .min(total.saturating_sub(span))
    }

    /// The current model's thinking levels: the effort pane and the root cell's
    /// presence both read them.
    fn levels(&self) -> Vec<String> {
        model_by_id(&self.harnesses, &self.current_model)
            .map(|model| model.thinking.clone())
            .unwrap_or_default()
    }

    /// The label of the model in force, as the root cell's value reads it.
    fn current_model_label(&self) -> String {
        model_by_id(&self.harnesses, &self.current_model)
            .map(|model| model.name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| self.current_model.clone())
    }

    /// The label of the level in force, as the root cell's value reads it.
    fn current_level_label(&self) -> String {
        if self.current_level.trim().is_empty() {
            EFFORT_DEFAULT_LABEL.to_string()
        } else {
            level_label(&self.current_level)
        }
    }

    /// The widest a model row needs: its content inset, the name, the check
    /// column, and the card's own padding.
    fn model_width(model: &ModelTrayModel) -> f32 {
        ROW_PAD_X * 2.0
            + measure_text_family(
                &model.name,
                LABEL_FONT_SIZE,
                MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                f32::INFINITY,
                FontWeight::Regular,
                FontFamily::System,
                false,
            )
            .x
            + ROW_GAP
            + CHECK_SIZE
            + CARD_PADDING * 2.0
    }

    /// The width one line of `text` needs at the menu's label scale.
    fn text_width(text: &str, font_size: f32, line_height: f32) -> f32 {
        measure_text_family(
            text,
            font_size,
            line_height / font_size,
            f32::INFINITY,
            FontWeight::Regular,
            FontFamily::System,
            false,
        )
        .x
    }

    /// The card caps for the surface the card opens over: DSH's
    /// `min(240px, 100vw - 24px)` root, `min(420px, 100vw - 24px)` list.
    fn card_limits(available: Option<f32>) -> (f32, f32, f32) {
        let cap = available.unwrap_or(f32::INFINITY).max(0.0);
        let root = ROOT_WIDTH.min(cap);
        let min = MIN_WIDTH.min(cap);
        let max = MAX_WIDTH.min(cap).max(min);
        (root, min, max)
    }

    /// Build the panel the current pane draws: its size, where it sits relative
    /// to the chip, and its rows for the hit test. `available` is the width of
    /// the surface the card opens over; a card that cannot fit shrinks its root
    /// casing and its list instead of running off the surface's edge.
    fn build_panel(&self, available: Option<f32>) -> Panel {
        let state = self.state.borrow().clone();
        let width = 760.0_f32.min(available.unwrap_or(760.0)).max(420.0);
        let height = 420.0;
        let provider = state.provider.min(self.harnesses.len().saturating_sub(1));
        let flat = flat_models(&self.harnesses);
        let models: Vec<(usize, &ModelTrayModel)> = flat
            .iter()
            .copied()
            .filter(|(group, _)| *group == provider)
            .collect();
        let selected_model = models
            .iter()
            .find(|(_, model)| model.id == self.current_model)
            .or_else(|| models.first())
            .map(|(_, model)| *model);
        let levels = selected_model
            .map(|model| model.thinking.clone())
            .unwrap_or_default();
        let mut rows = Vec::new();
        // Column 1: integrations/providers.
        for index in 0..self.harnesses.len() {
            rows.push(PanelRow {
                kind: PanelRowKind::Provider(index),
                rect: rectf(4.0, 32.0 + index as f32 * 36.0, 204.0, 34.0),
            });
        }
        // Column 2: the provider's searchable model list.
        rows.push(PanelRow {
            kind: PanelRowKind::Search,
            rect: rectf(216.0, 32.0, 272.0, 30.0),
        });
        for (row, (flat_index, _)) in models.iter().enumerate() {
            rows.push(PanelRow {
                kind: PanelRowKind::Model(*flat_index),
                rect: rectf(216.0, 68.0 + row as f32 * 34.0, 272.0, 32.0),
            });
        }
        // Column 3: the selected model's detail/effort picker.
        for index in 0..levels.len() {
            rows.push(PanelRow {
                kind: PanelRowKind::Level(index),
                rect: rectf(500.0, 68.0 + index as f32 * 34.0, 248.0, 32.0),
            });
        }
        let origin = vec2f(
            self.size.map(|size| size.x - width).unwrap_or(-width),
            -(height + CARD_GAP),
        );
        Panel {
            origin,
            size: vec2f(width, height),
            rows,
        }
    }

    /// Push the visible model rows, inserting a provider heading whenever the
    /// group changes, and measure their stacked height. The rows are laid out
    /// inside the card's padding: they start at the card's content edge and are
    /// `width - 2 * CARD_PADDING` wide.
    fn push_model_rows(
        harnesses: &[ModelTrayHarness],
        start: usize,
        end: usize,
        rows: &mut Vec<PanelRow>,
        top: f32,
        width: f32,
    ) {
        let flat = flat_models(harnesses);
        let row_width = (width - CARD_PADDING * 2.0).max(0.0);
        let mut y = top;
        let mut group: Option<usize> = None;
        for index in start..end {
            let Some((group_index, _)) = flat.get(index).copied() else {
                continue;
            };
            if group != Some(group_index) {
                if group.is_some() {
                    y += GROUP_GAP;
                }
                group = Some(group_index);
                rows.push(PanelRow {
                    kind: PanelRowKind::Group(group_index),
                    rect: rectf(0.0, y, row_width, GROUP_HEIGHT),
                });
                y += GROUP_HEIGHT;
            } else {
                y += ROW_SPACING;
            }
            rows.push(PanelRow {
                kind: PanelRowKind::Model(index),
                rect: rectf(0.0, y, row_width, ROW_HEIGHT),
            });
            y += ROW_HEIGHT;
        }
    }

    /// The stacked height of a run of rows.
    fn rows_height(rows: &[PanelRow]) -> f32 {
        rows.iter()
            .map(|row| row.rect.max_y())
            .fold(0.0f32, f32::max)
    }

    fn offset_rect(rect: RectF, x: f32, y: f32) -> RectF {
        rectf(
            rect.min_x() + x,
            rect.min_y() + y,
            rect.width(),
            rect.height(),
        )
    }

    /// Where the panel actually lands in the frame: DSH anchors the card above
    /// the chip with its trailing edge on the chip's, so it opens leftwards; its
    /// portal then keeps the card inside the layout viewport. The composer card
    /// is that viewport here, so a card that would run off its leading edge
    /// slides right until the whole card is inside instead of being cropped.
    fn placed_panel(&self) -> Option<(Vector2F, Vector2F)> {
        let origin = self.origin?;
        let panel = self.panel.as_ref()?;
        let mut x = origin.x() + panel.origin.x;
        if let Some(bounds) = self.bounds.as_ref().and_then(ModelTrayBounds::painted_rect) {
            // DSH insets the card 12pt inside the layout viewport
            // (`left: 12px; right: 12px`). The composer card is the native
            // frame's local viewport here, so the card clamps 12pt inside it;
            // when the card cannot fit between both insets it falls back to the
            // card's own edges rather than leaving the surface.
            let inset_left = bounds.min_x() + MODEL_MENU_VIEWPORT_INSET;
            let inset_right = bounds.max_x() - MODEL_MENU_VIEWPORT_INSET;
            let (left, right) = if inset_right - inset_left >= panel.size.x {
                (inset_left, inset_right - panel.size.x)
            } else {
                (
                    bounds.min_x(),
                    (bounds.max_x() - panel.size.x).max(bounds.min_x()),
                )
            };
            x = x.clamp(left, right);
        }
        Some((vec2f(x, origin.y() + panel.origin.y), panel.size))
    }

    /// The drawn panel's bounds in the frame's coordinates.
    fn panel_bounds(&self) -> Option<RectF> {
        let (origin, size) = self.placed_panel()?;
        Some(rectf(origin.x, origin.y, size.x, size.y))
    }

    /// One line of `font_size` vertically centred in a row.
    fn line_y(bounds: RectF, line_height: f32) -> f32 {
        bounds.min_y() + (ROW_HEIGHT - line_height).max(0.0) / 2.0
    }

    /// The row the pointer is in, when it is over the open panel.
    fn row_at(&self, position: Vector2F) -> Option<PanelRowKind> {
        let panel = self.panel.as_ref()?;
        let (origin, _) = self.placed_panel()?;
        panel
            .rows
            .iter()
            .find(|row| {
                contains(
                    rectf(
                        origin.x + row.rect.min_x(),
                        origin.y + row.rect.min_y(),
                        row.rect.width(),
                        row.rect.height(),
                    ),
                    position,
                )
            })
            .map(|row| row.kind)
    }

    /// Track the pointer over the open panel: the highlight follows the row
    /// under it, the way DSH's rows take focus on hover.
    fn pointer_move(&self, position: Vector2F) {
        let Some(kind) = self.row_at(position) else {
            return;
        };
        let mut state = self.state.borrow_mut();
        match kind {
            PanelRowKind::Cell(cell) => {
                state.cell = cell.min(1);
                state.cell_active = true;
            }
            PanelRowKind::Group(_) | PanelRowKind::Search => {}
            PanelRowKind::Provider(index) => {
                state.provider = index;
                state.pane = ModelTrayPane::Root;
            }
            PanelRowKind::Model(row) => {
                state.row = row;
                state.level = 0;
            }
            PanelRowKind::Level(level) => state.level = level,
        }
    }

    /// Take a press on the open panel: the root cells drill, a row takes its
    /// value and closes the menu (DSH's `choose`/`chooseEffort`).
    fn press(&self, position: Vector2F) -> bool {
        let Some(kind) = self.row_at(position) else {
            return false;
        };
        match kind {
            PanelRowKind::Cell(cell) => {
                let mut state = self.state.borrow_mut();
                state.cell = cell.min(1);
                state.cell_active = true;
                state.pane = if cell == 1 && !self.levels().is_empty() {
                    ModelTrayPane::Effort
                } else {
                    ModelTrayPane::Model
                };
                true
            }
            PanelRowKind::Group(_) | PanelRowKind::Search => true,
            PanelRowKind::Provider(index) => {
                let mut state = self.state.borrow_mut();
                state.provider = index;
                state.pane = ModelTrayPane::Model;
                let flat = flat_models(&self.harnesses);
                state.row = flat
                    .iter()
                    .position(|(group, _)| *group == index)
                    .unwrap_or(0);
                true
            }
            PanelRowKind::Model(row) => {
                *self.open.borrow_mut() = false;
                {
                    let mut state = self.state.borrow_mut();
                    state.row = row;
                }
                if let Some(cb) = self.on_select_model.clone() {
                    (cb.borrow_mut())(row);
                }
                true
            }
            PanelRowKind::Level(level) => {
                {
                    let mut state = self.state.borrow_mut();
                    state.level = level;
                }
                let level_id = self.levels().get(level).cloned().unwrap_or_default();
                *self.open.borrow_mut() = false;
                if !level_id.is_empty() {
                    if let Some(cb) = self.on_select_level.clone() {
                        (cb.borrow_mut())(level_id);
                    }
                }
                true
            }
        }
    }
}

impl Element for ModelTray {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let trigger_size = self.trigger.layout(constraint, ctx, app);
        self.size = Some(trigger_size);
        if *self.open.borrow() {
            self.seed();
            // The composer publishes the width of the card this menu opens
            // over before its children are measured, so the card is built at
            // the size it can actually paint in (DSH clamps its portaled card to
            // the layout viewport). A bare tray has no bounds and keeps its
            // design caps.
            let available = self
                .bounds
                .as_ref()
                .and_then(ModelTrayBounds::available_width);
            self.panel = Some(self.build_panel(available));
        } else {
            self.panel = None;
        }
        trigger_size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.trigger.paint(origin, ctx, app);
        let Some(panel) = self.panel.clone() else {
            return;
        };
        let Some((panel_origin, _panel_size)) = self.placed_panel() else {
            return;
        };
        let card = rectf(panel_origin.x, panel_origin.y, panel.size.x, panel.size.y);
        // The card is the window's open menu while it paints: it claims the
        // shared state (closing any other menu) and owns the pointer over its
        // own surface.
        self.claim(app);
        ctx.enter_menu_surface();
        let state = self.state.borrow().clone();
        let levels = self.levels();
        let flat = flat_models(&self.harnesses);
        let radius = CARD_RADIUS;
        let renderer = match ctx.renderer.as_mut() {
            Some(renderer) => renderer,
            None => {
                self.publish(ctx, app, card);
                return;
            }
        };
        // DSH's menu surface: one rounded, padded card with a hairline, floated
        // above the chip. The card is the whole menu; there is no band under it.
        renderer.fill_rounded_rect(card, app.theme.color(ColorToken::SurfaceRaised), radius);
        renderer.stroke_rect(card, app.theme.color(ColorToken::Border), 1.0, radius);

        if panel.rows.is_empty() {
            // An empty list keeps its status row: the card does not collapse to
            // a sliver (and never paints the message outside its casing).
            let message = if state.pane() == ModelTrayPane::Effort {
                NO_EFFORTS
            } else {
                NO_MODELS
            };
            let message_max = (panel.size.x - (CARD_PADDING + ROW_PAD_X) * 2.0).max(0.0);
            let message = truncate_with_ellipsis(
                message,
                LABEL_FONT_SIZE,
                MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                message_max,
            );
            renderer.draw_text(
                Vector2F::new(
                    card.min_x() + CARD_PADDING + ROW_PAD_X,
                    card.min_y()
                        + CARD_PADDING
                        + (STATUS_HEIGHT - MODEL_LINE_HEIGHT).max(0.0) / 2.0,
                ),
                message,
                LABEL_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                message_max,
                MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
            );
            self.publish(ctx, app, card);
            return;
        }

        // 477: the card is three columns — integrations, models, detail/effort.
        for (x, label) in [
            (card.min_x() + 10.0, "Integrations"),
            (card.min_x() + 224.0, "Models"),
            (card.min_x() + 506.0, "Effort"),
        ] {
            renderer.draw_text(
                Vector2F::new(x, card.min_y() + 8.0),
                label,
                GROUP_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                200.0,
                GROUP_LINE_HEIGHT / GROUP_FONT_SIZE,
            );
        }
        let detail_name = truncate_with_ellipsis(
            &self.current_model_label(),
            LABEL_FONT_SIZE,
            MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
            230.0,
        );
        renderer.draw_text(
            Vector2F::new(card.min_x() + 506.0, card.min_y() + 30.0),
            detail_name,
            LABEL_FONT_SIZE,
            app.theme.color(ColorToken::Text),
            230.0,
            MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
        );
        if let Some(source) = self.credential_source.as_deref() {
            renderer.draw_text(
                Vector2F::new(card.min_x() + 506.0, card.min_y() + 50.0),
                &format!("Key · {source}"),
                GROUP_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                230.0,
                GROUP_LINE_HEIGHT / GROUP_FONT_SIZE,
            );
        }
        for x in [card.min_x() + 214.0, card.min_x() + 496.0] {
            renderer.fill_rounded_rect(
                rectf(x, card.min_y() + 6.0, 1.0, card.height() - 12.0),
                app.theme.color(ColorToken::Border),
                0.0,
            );
        }
        if self.harnesses.is_empty()
            || self
                .harnesses
                .get(state.provider.min(self.harnesses.len().saturating_sub(1)))
                .is_some_and(|harness| harness.models.is_empty())
        {
            renderer.draw_text(
                Vector2F::new(card.min_x() + 224.0, card.min_y() + 76.0),
                "No models reported",
                LABEL_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                250.0,
                MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
            );
        }
        for row in &panel.rows {
            let bounds = rectf(
                panel_origin.x + row.rect.min_x(),
                panel_origin.y + row.rect.min_y(),
                row.rect.width(),
                row.rect.height(),
            );
            match row.kind {
                PanelRowKind::Cell(cell) => {
                    // DSH opens the root pane with focus on the trigger, so no
                    // cell is highlighted until an arrow lands on one; the
                    // keyboard's cell and the pointer's cell are the same.
                    if state.pane() == ModelTrayPane::Root
                        && state.cell_active
                        && state.cell == cell
                    {
                        renderer.fill_rounded_rect(
                            bounds,
                            app.theme.color(ColorToken::Selected),
                            ROW_RADIUS,
                        );
                    }
                    let (label, value) = if cell == 0 {
                        (MODEL_CELL_LABEL.to_string(), self.current_model_label())
                    } else {
                        (EFFORT_CELL_LABEL.to_string(), self.current_level_label())
                    };
                    // DSH's cell: label primary, the value tertiary and
                    // ellipsized in the space the label and chevron leave, the
                    // drill chevron at the trailing edge.
                    let content_left = bounds.min_x() + CELL_PAD_X;
                    let chevron_x = bounds.max_x() - CELL_PAD_X - CHEVRON_SIZE;
                    let label_width = Self::text_width(&label, LABEL_FONT_SIZE, CELL_LINE_HEIGHT);
                    let value_max =
                        (chevron_x - ROW_GAP - content_left - label_width - ROW_GAP).max(0.0);
                    let value = if value_max > 0.0 {
                        truncate_with_ellipsis(
                            &value,
                            LABEL_FONT_SIZE,
                            CELL_LINE_HEIGHT / LABEL_FONT_SIZE,
                            value_max,
                        )
                    } else {
                        String::new()
                    };
                    let value_width =
                        Self::text_width(&value, LABEL_FONT_SIZE, CELL_LINE_HEIGHT).min(value_max);
                    renderer.draw_text(
                        Vector2F::new(content_left, Self::line_y(bounds, CELL_LINE_HEIGHT)),
                        label,
                        LABEL_FONT_SIZE,
                        app.theme.color(ColorToken::Text),
                        (chevron_x - ROW_GAP - content_left).max(0.0),
                        CELL_LINE_HEIGHT / LABEL_FONT_SIZE,
                    );
                    if value_width > 0.0 {
                        renderer.draw_text(
                            Vector2F::new(
                                chevron_x - ROW_GAP - value_width,
                                Self::line_y(bounds, CELL_LINE_HEIGHT),
                            ),
                            value,
                            LABEL_FONT_SIZE,
                            app.theme.color(ColorToken::Muted),
                            value_width,
                            CELL_LINE_HEIGHT / LABEL_FONT_SIZE,
                        );
                    }
                    renderer.draw_icon(
                        Vector2F::new(
                            chevron_x,
                            bounds.min_y() + (ROW_HEIGHT - CHEVRON_SIZE).max(0.0) / 2.0,
                        ),
                        icon_atlas_name("chevron-right"),
                        CHEVRON_SIZE,
                        app.theme.color(ColorToken::Muted),
                    );
                }
                PanelRowKind::Provider(index) => {
                    let Some(harness) = self.harnesses.get(index) else {
                        continue;
                    };
                    if state.provider == index {
                        renderer.fill_rounded_rect(
                            bounds,
                            app.theme.color(ColorToken::Selected),
                            ROW_RADIUS,
                        );
                    }
                    let icon: &'static str = match harness.harness.as_str() {
                        "antigravity" => "harness-antigravity",
                        "claude" => "harness-claude",
                        "codex" => "harness-codex",
                        "gemini" => "harness-gemini",
                        "deepseek" => "harness-deepseek",
                        _ => "plug",
                    };
                    renderer.draw_icon(
                        Vector2F::new(
                            bounds.min_x() + ROW_PAD_X,
                            bounds.min_y() + (ROW_HEIGHT - 14.0) / 2.0,
                        ),
                        icon_atlas_name(icon),
                        14.0,
                        app.theme.color(ColorToken::Muted),
                    );
                    let label_max = (bounds.width() - ROW_PAD_X * 2.0 - 22.0).max(0.0);
                    let label = truncate_with_ellipsis(
                        &harness.label,
                        LABEL_FONT_SIZE,
                        MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                        label_max,
                    );
                    renderer.draw_text(
                        Vector2F::new(
                            bounds.min_x() + ROW_PAD_X + 22.0,
                            Self::line_y(bounds, MODEL_LINE_HEIGHT),
                        ),
                        label,
                        LABEL_FONT_SIZE,
                        app.theme.color(ColorToken::Text),
                        label_max,
                        MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                    );
                }
                PanelRowKind::Search => {
                    renderer.draw_text(
                        Vector2F::new(
                            bounds.min_x() + ROW_PAD_X,
                            Self::line_y(bounds, MODEL_LINE_HEIGHT),
                        ),
                        if state.search.is_empty() {
                            "Search models..."
                        } else {
                            &state.search
                        },
                        LABEL_FONT_SIZE,
                        app.theme.color(ColorToken::Muted),
                        (bounds.width() - ROW_PAD_X * 2.0).max(0.0),
                        MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                    );
                }
                PanelRowKind::Group(group) => {
                    let Some(harness) = self.harnesses.get(group) else {
                        continue;
                    };
                    let label_max = (bounds.width() - GROUP_PAD_X * 2.0).max(0.0);
                    let label = truncate_with_ellipsis(
                        &harness.label,
                        GROUP_FONT_SIZE,
                        GROUP_LINE_HEIGHT / GROUP_FONT_SIZE,
                        label_max,
                    );
                    renderer.draw_text(
                        Vector2F::new(bounds.min_x() + GROUP_PAD_X, bounds.min_y() + GROUP_PAD_TOP),
                        label,
                        GROUP_FONT_SIZE,
                        app.theme.color(ColorToken::Muted),
                        label_max,
                        GROUP_LINE_HEIGHT / GROUP_FONT_SIZE,
                    );
                }
                PanelRowKind::Model(index) => {
                    let Some((_, model)) = flat.get(index).copied() else {
                        continue;
                    };
                    if state.row == index {
                        renderer.fill_rounded_rect(
                            bounds,
                            app.theme.color(ColorToken::Selected),
                            ROW_RADIUS,
                        );
                    }
                    // The name takes the row's leading space; a long one
                    // ellipsizes on one line instead of wrapping over the next
                    // row, and the trailing check keeps its column.
                    let name_max =
                        (bounds.width() - ROW_PAD_X * 2.0 - ROW_GAP - CHECK_SIZE).max(0.0);
                    let name = truncate_with_ellipsis(
                        &model.name,
                        LABEL_FONT_SIZE,
                        MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                        name_max,
                    );
                    renderer.draw_text(
                        Vector2F::new(
                            bounds.min_x() + ROW_PAD_X,
                            Self::line_y(bounds, MODEL_LINE_HEIGHT),
                        ),
                        name,
                        LABEL_FONT_SIZE,
                        app.theme.color(ColorToken::Text),
                        name_max,
                        MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                    );
                    if model.id == self.current_model {
                        renderer.draw_icon(
                            Vector2F::new(
                                bounds.max_x() - ROW_PAD_X - CHECK_SIZE,
                                bounds.min_y() + (ROW_HEIGHT - CHECK_SIZE).max(0.0) / 2.0,
                            ),
                            icon_atlas_name("check"),
                            CHECK_SIZE,
                            app.theme.color(ColorToken::Text),
                        );
                    }
                }
                PanelRowKind::Level(index) => {
                    let Some(level) = levels.get(index) else {
                        continue;
                    };
                    if state.level == index {
                        renderer.fill_rounded_rect(
                            bounds,
                            app.theme.color(ColorToken::Selected),
                            ROW_RADIUS,
                        );
                    }
                    let label_max =
                        (bounds.width() - ROW_PAD_X * 2.0 - ROW_GAP - CHECK_SIZE).max(0.0);
                    let label = truncate_with_ellipsis(
                        &level_label(level),
                        LABEL_FONT_SIZE,
                        MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                        label_max,
                    );
                    renderer.draw_text(
                        Vector2F::new(
                            bounds.min_x() + ROW_PAD_X,
                            Self::line_y(bounds, MODEL_LINE_HEIGHT),
                        ),
                        label,
                        LABEL_FONT_SIZE,
                        app.theme.color(ColorToken::Text),
                        label_max,
                        MODEL_LINE_HEIGHT / LABEL_FONT_SIZE,
                    );
                    if level == &self.current_level {
                        renderer.draw_icon(
                            Vector2F::new(
                                bounds.max_x() - ROW_PAD_X - CHECK_SIZE,
                                bounds.min_y() + (ROW_HEIGHT - CHECK_SIZE).max(0.0) / 2.0,
                            ),
                            icon_atlas_name("check"),
                            CHECK_SIZE,
                            app.theme.color(ColorToken::Text),
                        );
                    }
                }
            }
        }
        self.publish(ctx, app, card);
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        let bounds = match self.bounds() {
            Some(bounds) => bounds,
            None => return false,
        };
        let position = match event {
            DispatchedEvent::MouseMove { position } => *position,
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. } => *position,
            _ => return self.trigger.dispatch_event(event, ctx, app),
        };
        if !*self.open.borrow() {
            return self.trigger.dispatch_event(event, ctx, app);
        }
        // The chip stays the control that opens and closes the menu.
        if contains(bounds, position) {
            return self.trigger.dispatch_event(event, ctx, app);
        }
        if self
            .panel_bounds()
            .is_some_and(|panel| contains(panel, position))
        {
            match event {
                DispatchedEvent::MouseMove { .. } => {
                    self.pointer_move(position);
                    false
                }
                DispatchedEvent::MouseDown { button, .. } => {
                    if *button == crate::event::BUTTON_PRIMARY {
                        self.press(position);
                    }
                    true
                }
                DispatchedEvent::MouseUp { .. } => true,
                _ => true,
            }
        } else {
            // A primary press anywhere else puts the card away; the press then
            // belongs to whatever it landed on (the editor takes the caret).
            if matches!(
                event,
                DispatchedEvent::MouseDown {
                    button: crate::event::BUTTON_PRIMARY,
                    ..
                }
            ) {
                *self.open.borrow_mut() = false;
            }
            false
        }
    }
}

/// What a pane hands a composer: the card's data, the selection the two share,
/// and the flag the model control toggles.
///
/// The chip and the card are two elements rebuilt every frame, so everything
/// they have to agree on — which models are listed, which pane and row the
/// arrows are on, whether the card is up — lives in this handle and is cloned
/// into both.
#[derive(Clone)]
pub struct ComposerModelTray {
    pub harnesses: Rc<Vec<ModelTrayHarness>>,
    pub state: Rc<RefCell<ModelTrayState>>,
    /// Whether the card is open. The model control toggles it, the card reads it.
    pub open: Rc<RefCell<bool>>,
    /// The model the pane runs and the thinking level in force, so the card
    /// opens on them and marks them in the lists.
    pub model: String,
    pub level: String,
    pub credential_source: Option<String>,
    /// A taken model: its flat index in the list the card drew.
    pub on_select_model: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    /// A taken thinking level, named by the id its harness reports.
    pub on_select_level: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    /// The surface the card is placed inside, when the host publishes one (the
    /// composer does: its card's width and rect). A host that wraps the tray
    /// itself keeps the `None` and the card sizes to its design caps.
    pub bounds: Option<ModelTrayBounds>,
}

impl ComposerModelTray {
    pub fn new(
        harnesses: Vec<ModelTrayHarness>,
        state: Rc<RefCell<ModelTrayState>>,
        open: Rc<RefCell<bool>>,
    ) -> Self {
        Self {
            harnesses: Rc::new(harnesses),
            state,
            open,
            model: String::new(),
            level: String::new(),
            credential_source: None,
            on_select_model: None,
            on_select_level: None,
            bounds: None,
        }
    }

    pub fn with_current(mut self, model: impl Into<String>, level: impl Into<String>) -> Self {
        self.model = model.into();
        self.level = level.into();
        self
    }

    /// 492: show where the key came from in the detail column.
    pub fn with_credential_source(mut self, source: Option<String>) -> Self {
        self.credential_source = source;
        self
    }

    pub fn with_on_select_model<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_level<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_select_level = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Publish the surface the card opens over: the composer hands its own
    /// card's width and rect here so the menu clamps inside it, the way DSH's
    /// portaled card clamps to the layout viewport.
    pub fn with_bounds(mut self, bounds: ModelTrayBounds) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// The chip wrapped in the card it opens: the same data, the same selection,
    /// and the callbacks a taken row reports to.
    pub fn popup(&self, trigger: Box<dyn Element>) -> ModelTray {
        ModelTray::new(
            trigger,
            self.harnesses.clone(),
            self.state.clone(),
            self.open.clone(),
            self.model.clone(),
            self.level.clone(),
            self.credential_source.clone(),
            self.on_select_model.clone(),
            self.on_select_level.clone(),
            self.bounds.clone(),
        )
    }

    /// Kept for the view's band slot. The DSH model menu is a card anchored to
    /// the chip inside the composer, not a sibling band, so the slot draws
    /// nothing; a `None` here leaves the host's slot empty instead of stacking a
    /// second model list over the input.
    pub fn band(&self) -> Option<ModelTray> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ModifiersState;
    use crate::geometry::vec2f;

    fn key(name: &str) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers: ModifiersState::none(),
        }
    }

    fn harnesses() -> Vec<ModelTrayHarness> {
        vec![
            ModelTrayHarness::new("deepseek", "DeepSeek").with_models(vec![
                ModelTrayModel::new("deepseek-flash", "DeepSeek Flash")
                    .with_thinking(vec!["off".to_string(), "on".to_string()]),
                ModelTrayModel::new("deepseek-pro", "DeepSeek Pro")
                    .with_thinking(vec!["off".to_string(), "on".to_string()]),
            ]),
            ModelTrayHarness::new("anthropic", "Anthropic")
                .with_models(vec![ModelTrayModel::new("claude-sonnet", "Claude Sonnet")]),
        ]
    }

    /// The card opens on the root pane, placed on the pane's own model and level.
    #[test]
    fn the_card_seeds_on_the_root_pane_at_the_pane_model() {
        let mut state = ModelTrayState::default();
        state.seed(&harnesses(), "claude-sonnet", "off");
        assert_eq!(state.pane(), ModelTrayPane::Root);
        assert_eq!(state.cell, 0, "the Model cell leads the root pane");
        assert_eq!(state.row, 2, "the row is counted across the providers");
        assert_eq!(state.level, 0);
        assert!(state.seeded);
    }

    /// The root pane's arrows move between its cells and Enter drills into the
    /// pane each one names.
    #[test]
    fn the_root_pane_drills_into_models_and_efforts() {
        let harnesses = harnesses();
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        state.borrow_mut().seed(&harnesses, "deepseek-flash", "off");
        assert_eq!(
            handle_tray_key(&key("Enter"), &harnesses, "deepseek-flash", "off", &state),
            ModelTrayKey::DrillModel
        );
        assert_eq!(state.borrow().pane(), ModelTrayPane::Model);
        assert_eq!(
            handle_tray_key(&key("Escape"), &harnesses, "deepseek-flash", "off", &state),
            ModelTrayKey::Back
        );
        assert_eq!(state.borrow().pane(), ModelTrayPane::Root);

        assert_eq!(
            handle_tray_key(
                &key("ArrowDown"),
                &harnesses,
                "deepseek-flash",
                "off",
                &state
            ),
            ModelTrayKey::Moved
        );
        assert_eq!(
            state.borrow().provider,
            1,
            "Down moves the integration column"
        );
        assert_eq!(
            handle_tray_key(&key("Enter"), &harnesses, "deepseek-flash", "off", &state),
            ModelTrayKey::DrillModel
        );
        assert_eq!(state.borrow().pane(), ModelTrayPane::Model);
    }

    /// The model pane's arrows walk the flat model list (wrapping) and Enter
    /// takes the row through the host's index.
    #[test]
    fn the_model_pane_moves_and_takes_a_model() {
        let harnesses = harnesses();
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        state.borrow_mut().seed(&harnesses, "claude-sonnet", "off");
        state.borrow_mut().pane = ModelTrayPane::Model;
        assert_eq!(
            handle_tray_key(&key("ArrowUp"), &harnesses, "claude-sonnet", "off", &state),
            ModelTrayKey::Moved
        );
        assert_eq!(state.borrow().row, 1, "the list wraps from the last row up");
        assert_eq!(
            handle_tray_key(&key("Enter"), &harnesses, "claude-sonnet", "off", &state),
            ModelTrayKey::AcceptModel(1)
        );
    }

    /// The effort pane takes the current model's level by the id its harness
    /// reports, and a model without levels cannot enter it at all.
    #[test]
    fn the_effort_pane_moves_and_takes_a_level() {
        let harnesses = harnesses();
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        state.borrow_mut().seed(&harnesses, "deepseek-flash", "off");
        state.borrow_mut().pane = ModelTrayPane::Effort;
        assert_eq!(
            handle_tray_key(
                &key("ArrowDown"),
                &harnesses,
                "deepseek-flash",
                "off",
                &state
            ),
            ModelTrayKey::Moved
        );
        assert_eq!(state.borrow().level, 1);
        assert_eq!(
            handle_tray_key(&key("Enter"), &harnesses, "deepseek-flash", "off", &state),
            ModelTrayKey::AcceptLevel("on".to_string())
        );

        let mut dataless = harnesses.clone();
        dataless[0].models[0].thinking.clear();
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        state.borrow_mut().seed(&dataless, "deepseek-flash", "");
        assert_eq!(state.borrow().pane(), ModelTrayPane::Root);
        assert_eq!(
            handle_tray_key(&key("ArrowDown"), &dataless, "deepseek-flash", "", &state),
            ModelTrayKey::Moved
        );
        assert_eq!(
            state.borrow().cell,
            0,
            "a model with no levels has no Effort cell to sit in"
        );
    }

    #[test]
    fn escape_from_the_root_pane_closes_the_card() {
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        assert_eq!(
            handle_tray_key(&key("Escape"), &harnesses(), "deepseek-flash", "", &state),
            ModelTrayKey::Close
        );
    }

    /// A command key never comes to the card: it is the host's own gesture.
    #[test]
    fn a_command_key_never_comes_to_the_card() {
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        let event = DispatchedEvent::KeyDown {
            key: "Enter".to_string(),
            modifiers: ModifiersState {
                command: true,
                ..ModifiersState::none()
            },
        };
        assert_eq!(
            handle_tray_key(&event, &harnesses(), "m1", "", &state),
            ModelTrayKey::Ignored
        );
    }

    /// The window keeps the selected model row inside the card even when the
    /// catalogue is longer than the card.
    #[test]
    fn the_window_keeps_the_selected_row_visible() {
        let models: Vec<ModelTrayModel> = (0..20)
            .map(|index| ModelTrayModel::new(format!("m{index}"), format!("Model {index}")))
            .collect();
        let harnesses = vec![
            ModelTrayHarness::new("goble", "Goble").with_models(models),
            ModelTrayHarness::new("claude", "Anthropic")
                .with_models(vec![ModelTrayModel::new("claude-sonnet", "Claude Sonnet")]),
        ];
        let start = ModelTray::window_start(17, flat_models(&harnesses).len());
        assert!(start <= 17, "the selected row is inside the window");
        assert!(start + MODEL_TRAY_ROWS > 17);
    }

    #[test]
    fn a_level_is_spelled_the_way_a_person_reads_it() {
        assert_eq!(level_label("off"), "Off");
        assert_eq!(level_label("on"), "On");
        assert_eq!(level_label("medium"), "Medium");
        assert_eq!(level_label(""), "");
    }

    /// The band slot stays empty now the card lives inside the composer.
    #[test]
    fn the_host_slot_draws_no_second_model_list() {
        let tray = ComposerModelTray::new(
            harnesses(),
            Rc::new(RefCell::new(ModelTrayState::default())),
            Rc::new(RefCell::new(true)),
        );
        assert!(tray.band().is_none());
    }

    /// Paint one frame of an open card and return its draw commands.
    fn painted(
        menu: &mut ModelTray,
        app: &AppContext,
        origin: Vector2F,
    ) -> Vec<crate::render::RenderCommand> {
        menu.layout(
            SizeConstraint::loose(vec2f(800.0, 600.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(crate::render::Renderer::new());
        menu.paint(origin, &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    fn drawn_texts(commands: &[crate::render::RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                crate::render::RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// DSH anchors the card above the chip with its trailing edge on the chip's
    /// and its body portal keeps it inside the layout viewport. The composer
    /// card is that viewport here: a card that would run off its leading edge
    /// slides right instead of being cropped.
    #[test]
    fn the_card_is_right_aligned_and_clamps_inside_the_composer_card() {
        let app = AppContext::default();
        let bounds = ModelTrayBounds::new();
        bounds.set_width(500.0);
        let tray = ComposerModelTray::new(
            harnesses(),
            Rc::new(RefCell::new(ModelTrayState::default())),
            Rc::new(RefCell::new(true)),
        )
        .with_current("deepseek-flash", "off")
        .with_bounds(bounds.clone());
        let mut menu = tray.popup(Box::new(
            crate::elements::Empty::new().with_size(vec2f(100.0, 28.0)),
        ));
        // Right-aligned to the chip when the card fits leftwards of its edge.
        bounds.set_rect(rectf(12.0, 0.0, 900.0, 600.0));
        painted(&mut menu, &app, vec2f(600.0, 300.0));
        let (origin, size) = menu.placed_panel().expect("an open card has a panel");
        assert!(
            (size.x - 500.0).abs() < 0.5,
            "the three-column casing fits the available width: {size:?}"
        );
        assert!(
            (origin.x - (700.0 - 500.0)).abs() < 0.5,
            "the trailing edge is on the chip's right edge: {origin:?}"
        );
        assert!(
            (origin.y - (300.0 - size.y - 8.0)).abs() < 0.5,
            "the card hangs 8pt above the chip: {origin:?}"
        );

        // A narrower surface clamps the card inside its trailing edge.
        bounds.set_rect(rectf(12.0, 0.0, 500.0, 600.0));
        painted(&mut menu, &app, vec2f(600.0, 300.0));
        let (origin, _) = menu.placed_panel().expect("an open card has a panel");
        assert!(
            (origin.x - 12.0).abs() < 0.5,
            "the card clamps to the surface's leading edge when it cannot fit: {origin:?}"
        );

        // A chip near the surface's leading edge cannot open leftwards: the
        // whole card slides right, inside the surface.
        bounds.set_rect(rectf(12.0, 0.0, 500.0, 600.0));
        painted(&mut menu, &app, vec2f(20.0, 300.0));
        let (origin, _) = menu.placed_panel().expect("an open card has a panel");
        assert!(
            (origin.x - 12.0).abs() < 0.5,
            "the card clamps to the surface's leading edge: {origin:?}"
        );
    }

    /// An empty catalogue keeps its status row inside the card casing instead of
    /// collapsing the card to a sliver and painting the message outside it.
    #[test]
    fn an_empty_model_list_draws_its_status_row_inside_the_card() {
        let app = AppContext::default();
        let open = Rc::new(RefCell::new(true));
        let state = Rc::new(RefCell::new(ModelTrayState {
            pane: ModelTrayPane::Model,
            cell: 0,
            cell_active: false,
            row: 0,
            provider: 0,
            search: String::new(),
            level: 0,
            seeded: true,
        }));
        let tray = ComposerModelTray::new(Vec::new(), state, open).with_current("", "");
        let mut menu = tray.popup(Box::new(crate::elements::Empty::new()));
        menu.layout(
            SizeConstraint::loose(vec2f(800.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let panel = menu.panel.as_ref().expect("an open card has a panel");
        assert!(
            panel.size.y >= CARD_PADDING * 2.0 + STATUS_HEIGHT,
            "the empty card keeps its status row: {:?}",
            panel.size
        );
        let commands = painted(&mut menu, &app, vec2f(100.0, 300.0));
        assert!(
            drawn_texts(&commands)
                .iter()
                .any(|text| text == "No models reported"),
            "the empty model pane names its empty state: {:?}",
            drawn_texts(&commands)
        );
    }

    /// A name longer than the card is ellipsized on one line: a wrapped run
    /// would paint a second line over the row below, which is the cropping the
    /// card must never do.
    #[test]
    fn a_long_model_name_ellipsizes_in_the_root_cell() {
        let app = AppContext::default();
        let long = "DeepSeek-V4.1-Flash-Experimental-Preview-Candidate-2026-11";
        let harnesses = vec![
            ModelTrayHarness::new("deepseek", "DeepSeek")
                .with_models(vec![
                    ModelTrayModel::new("long", long).with_thinking(vec!["high".to_string()])
                ]),
        ];
        let open = Rc::new(RefCell::new(true));
        let tray = ComposerModelTray::new(
            harnesses,
            Rc::new(RefCell::new(ModelTrayState::default())),
            open,
        )
        .with_current("long", "high");
        let mut menu = tray.popup(Box::new(crate::elements::Empty::new()));
        let commands = painted(&mut menu, &app, vec2f(100.0, 300.0));
        let texts = drawn_texts(&commands);
        assert!(
            !texts.iter().any(|text| text == long),
            "the full name cannot be drawn in a 240pt cell: {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|text| text.starts_with("DeepSeek-V4.1") && text.ends_with('…')),
            "the cell value keeps a readable prefix and an ellipsis: {texts:?}"
        );
    }

    /// DSH opens the root pane with focus on the trigger, so no cell is
    /// highlighted until an arrow lands on one; the card mirrors that instead of
    /// opening with the Model cell already filled.
    #[test]
    fn the_root_pane_highlights_a_cell_only_once_the_keyboard_lands_on_one() {
        use crate::render::RenderCommand;

        let app = AppContext::default();
        let open = Rc::new(RefCell::new(true));
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        let tray = ComposerModelTray::new(harnesses(), state.clone(), open)
            .with_current("deepseek-flash", "off");
        let mut menu = tray.popup(Box::new(crate::elements::Empty::new()));
        let selected = app.theme.color(ColorToken::Selected);
        let highlights = |commands: &[RenderCommand]| {
            commands
                .iter()
                .filter(|command| {
                    matches!(
                        command,
                        RenderCommand::FillRect { color, corner_radius, .. }
                            if *color == selected && (*corner_radius - ROW_RADIUS).abs() < 0.01
                    )
                })
                .count()
        };

        let commands = painted(&mut menu, &app, vec2f(100.0, 300.0));
        assert!(
            highlights(&commands) >= 1,
            "the integration/model column opens on its selected row: {:?}",
            drawn_texts(&commands)
        );

        // The first arrow moves onto the Model cell and the card shows it.
        assert_eq!(
            handle_tray_key(
                &DispatchedEvent::KeyDown {
                    key: "ArrowDown".to_string(),
                    modifiers: crate::event::ModifiersState::none(),
                },
                &harnesses(),
                "deepseek-flash",
                "off",
                &state,
            ),
            ModelTrayKey::Moved
        );
        assert!(state.borrow().cell_active);
        let commands = painted(&mut menu, &app, vec2f(100.0, 300.0));
        assert_eq!(
            highlights(&commands),
            1,
            "the keyboard's cell is the highlighted one: {:?}",
            drawn_texts(&commands)
        );
    }

    /// 477: the models column lists every model of the selected integration.
    #[test]
    fn the_models_column_lists_each_model_name() {
        let app = AppContext::default();
        let harnesses = vec![
            ModelTrayHarness::new("anthropic", "Anthropic").with_models(vec![
                ModelTrayModel::new("opus", "Claude Opus 4"),
                ModelTrayModel::new("sonnet", "Claude Sonnet 4"),
            ]),
        ];
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        state.borrow_mut().seeded = true;
        state.borrow_mut().pane = ModelTrayPane::Model;
        let tray = ComposerModelTray::new(harnesses, state, Rc::new(RefCell::new(true)))
            .with_current("sonnet", "high");
        let mut menu = tray.popup(Box::new(crate::elements::Empty::new()));
        let commands = painted(&mut menu, &app, vec2f(100.0, 300.0));
        let texts = drawn_texts(&commands);
        assert!(
            texts.iter().any(|text| text == "Claude Opus 4"),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|text| text == "Claude Sonnet 4"),
            "{texts:?}"
        );
    }

    /// The card's size stays inside DSH's casing bounds.
    #[test]
    fn the_card_stays_inside_its_design_bounds() {
        let tray = ComposerModelTray::new(
            harnesses(),
            Rc::new(RefCell::new(ModelTrayState::default())),
            Rc::new(RefCell::new(true)),
        )
        .with_current("deepseek-flash", "off");
        let mut menu = tray.popup(Box::new(crate::elements::Empty::new()));
        menu.layout(
            SizeConstraint::loose(vec2f(800.0, 600.0)),
            &mut LayoutContext::default(),
            &AppContext::default(),
        );
        let panel = menu.panel.as_ref().expect("an open card has a panel");
        assert!(panel.size.x >= MIN_WIDTH && panel.size.x <= MAX_WIDTH);
        assert!(panel.size.y <= MAX_HEIGHT + CARD_PADDING * 2.0);
        assert!(
            panel.origin.y <= -(CARD_GAP),
            "the card hangs above the chip"
        );
    }

    // ---- the window's shared open-menu state ------------------------------

    /// One frame of the real loop for the card: layout, seed the open-menu gate
    /// from the registry (as the root view does), paint, reconcile.
    fn frame(menu: &mut ModelTray, app: &AppContext, cursor: Option<Vector2F>) {
        menu.layout(
            SizeConstraint::loose(vec2f(800.0, 600.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(crate::render::Renderer::new());
        if let Some(point) = cursor {
            ctx.cursor_position = point;
            ctx.cursor_inside = true;
        }
        ctx.open_menu_surfaces = app.open_menus.surfaces();
        app.open_menus.begin_paint();
        menu.paint(vec2f(0.0, 0.0), &mut ctx, app);
        app.open_menus.end_paint();
    }

    /// The card is one of the window's menus: opening it claims the shared
    /// state, the pointer over its card hovers the card and not the element
    /// behind it, and a press outside closes it.
    #[test]
    fn the_open_card_claims_the_open_menu_state_and_owns_the_pointer() {
        use crate::elements::{EventContext, HoverRow, Text};

        let app = AppContext::default();
        let open = Rc::new(RefCell::new(false));
        let tray = ComposerModelTray::new(
            harnesses(),
            Rc::new(RefCell::new(ModelTrayState::default())),
            Rc::clone(&open),
        )
        .with_current("deepseek-flash", "off");
        let mut menu = tray.popup(Box::new(crate::elements::Empty::new()));

        // Closed, the card is not the open menu.
        frame(&mut menu, &app, None);
        assert!(!app.open_menus.is_open(), "a closed card registers nothing");

        *open.borrow_mut() = true;
        frame(&mut menu, &app, None);
        assert_eq!(
            app.open_menus.id(),
            Some(MODEL_TRAY_MENU_ID),
            "the open card is the window's open menu"
        );
        let surfaces = app.open_menus.surfaces();
        assert_eq!(surfaces.len(), 1, "the card owns exactly its own face");
        let panel = surfaces[0];

        // The pointer on the card: a row painted behind it must not hover.
        let point = vec2f(panel.min_x() + 10.0, panel.min_y() + 10.0);
        frame(&mut menu, &app, Some(point));
        let mut ctx = PaintContext::new(crate::render::Renderer::new());
        ctx.cursor_position = point;
        ctx.cursor_inside = true;
        ctx.open_menu_surfaces = app.open_menus.surfaces();
        let hover_cell: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let mut behind = HoverRow::new(Text::new("behind").finish())
            .with_hover_key(Rc::clone(&hover_cell), "behind");
        behind.layout(
            SizeConstraint::loose(vec2f(300.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        behind.paint(vec2f(point.x - 20.0, point.y - 10.0), &mut ctx, &app);
        assert_eq!(
            *hover_cell.borrow(),
            None,
            "the element behind the model card must not hover under the pointer"
        );

        // A press outside the card (and its chip) closes it and clears the
        // shared state.
        let mut ectx = EventContext::default();
        menu.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(700.0, 300.0),
                button: 0,
            },
            &mut ectx,
            &app,
        );
        assert!(!*open.borrow(), "the outside press closed the card");
        frame(&mut menu, &app, None);
        assert!(
            !app.open_menus.is_open(),
            "the shared state let go of the closed card"
        );
    }

    /// Opening the card dismisses another menu that was up: only one menu is
    /// open at a time across the window.
    #[test]
    fn opening_the_card_closes_the_menu_that_was_open() {
        let app = AppContext::default();
        let previous_closed = Rc::new(RefCell::new(false));
        let previous_close: crate::elements::MenuClose = {
            let previous_closed = Rc::clone(&previous_closed);
            Rc::new(RefCell::new(move || *previous_closed.borrow_mut() = true))
        };
        app.open_menus.open("some.other.menu", previous_close);

        let open = Rc::new(RefCell::new(true));
        let tray = ComposerModelTray::new(
            harnesses(),
            Rc::new(RefCell::new(ModelTrayState::default())),
            Rc::clone(&open),
        )
        .with_current("deepseek-flash", "off");
        let mut menu = tray.popup(Box::new(crate::elements::Empty::new()));
        frame(&mut menu, &app, None);

        assert_eq!(app.open_menus.id(), Some(MODEL_TRAY_MENU_ID));
        assert!(
            *previous_closed.borrow(),
            "the menu that was open was closed by the card opening"
        );
    }
}
