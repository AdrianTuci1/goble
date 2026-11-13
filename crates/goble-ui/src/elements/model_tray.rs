//! The composer's model tray: the models a pane can run, listed in the column
//! layout the reference client uses — the models in one list, grouped under the
//! harness each one belongs to, and the thinking levels the highlighted model
//! accepts beside them.
//!
//! The harness column is recognition, not a switch. A group header carries the
//! harness's mark and its name, so a row says which harness owns a model without
//! asking the user to choose a provider first; a pick is always a model. Which
//! endpoint that model talks to is the configuration's business, not the tray's.
//!
//! It is the same band the command list uses ([`super::slash_menu`]): drawn
//! directly above the input rather than floating over it, the input's width, in
//! the same slot. What differs is the layout: two lists side by side instead of
//! one, and the card's own rounding.
//!
//! Like the command list it does not take the keyboard: the editor keeps the
//! caret and the composer routes the keys ([`handle_tray_key`]), because the
//! tray itself never holds focus. The element draws what it was given; what it
//! owns is the geometry, the hit testing and the paint.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::icon::icon_atlas_name;
use crate::elements::interactive::contains;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, RectF, Vector2F};
use crate::theme::ColorToken;

/// How many model rows a column draws before it windows around the selection.
pub const MODEL_TRAY_ROWS: usize = 8;

/// The width the panel falls back to when its constraint is unbounded.
const FALLBACK_WIDTH: f32 = 420.0;

/// The thinking column's width: the levels are short words, so the model list
/// takes what is left — model names are the long ones.
const LEVEL_WIDTH: f32 = 132.0;
/// The narrowest the model column is allowed to get before the thinking column
/// gives room back.
const MODEL_MIN_WIDTH: f32 = 180.0;

/// The column headers' row, and the room under the lists so the last row does
/// not sit on the card's edge.
const HEADER_HEIGHT: f32 = 24.0;
const PADDING_BOTTOM: f32 = 6.0;
/// A model row, a thinking row, and the smaller header that names a harness.
const ROW_HEIGHT: f32 = 26.0;
const GROUP_HEIGHT: f32 = 20.0;
/// A row's text starts this far from its column's left edge.
const ROW_INSET: f32 = 14.0;
/// The gap between a harness's mark and its name.
const MARK_GAP: f32 = 8.0;
const GROUP_MARK_SIZE: f32 = 12.0;
const LABEL_FONT_SIZE: f32 = 12.0;
const GROUP_FONT_SIZE: f32 = 11.0;
const HEADER_FONT_SIZE: f32 = 10.0;
const LINE_HEIGHT: f32 = 1.2;

/// One harness a pane can run, as the tray recognises it: the mark an id draws,
/// the name its group header shows, and the models that harness runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelTrayHarness {
    /// The harness id the mark comes from (`goble`, `claude`, `grok`), or a
    /// provider name this build has no mark for.
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

/// One model in a harness's group: the id a turn runs on, the label the row
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

    /// The level the tray falls back to: the first one the model offers, so a
    /// pick is always one the harness runs.
    pub fn default_level(&self) -> String {
        self.thinking.first().cloned().unwrap_or_default()
    }
}

/// What a thinking level is called in the tray. The harness reports the ids it
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

/// The two lists, in the order the tray draws them. The harness is a group
/// header inside the first one; it is not a list of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelTrayColumn {
    Model,
    Level,
}

impl ModelTrayColumn {
    fn index(self) -> usize {
        match self {
            ModelTrayColumn::Model => 0,
            ModelTrayColumn::Level => 1,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => ModelTrayColumn::Level,
            _ => ModelTrayColumn::Model,
        }
    }
}

/// Every model the tray lists, in the order it draws them, each with the harness
/// that owns it. The keyboard and the hit test both work on this one list, so
/// what the arrows move and what a press takes cannot disagree.
fn flat_models(harnesses: &[ModelTrayHarness]) -> Vec<(usize, &ModelTrayModel)> {
    harnesses
        .iter()
        .enumerate()
        .flat_map(|(group, harness)| harness.models.iter().map(move |model| (group, model)))
        .collect()
}

/// The tray's selection, host-owned: the element is rebuilt every frame, so the
/// row the arrows moved to has to live outside it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelTrayState {
    pub column: usize,
    /// The row of the model list the selection is on, counted across every
    /// harness: the group headers are not rows a keyboard can land on.
    pub row: usize,
    pub level: usize,
    /// Whether the tray has been placed on the pane's own model yet. The tray
    /// seeds itself from the model in force the first time it is drawn, and the
    /// control clears this when it opens so the next open starts there again.
    pub seeded: bool,
}

impl ModelTrayState {
    pub fn column(&self) -> ModelTrayColumn {
        ModelTrayColumn::from_index(self.column)
    }

    /// Place the selection on the pane's model, and on the level it runs at.
    /// Called once per open, from the model the composer shows.
    pub fn seed(&mut self, harnesses: &[ModelTrayHarness], model: &str, level: &str) {
        let list = flat_models(harnesses);
        self.column = ModelTrayColumn::Model.index();
        self.row = list
            .iter()
            .position(|(_, candidate)| candidate.id == model)
            .unwrap_or(0);
        self.level = list
            .get(self.row)
            .and_then(|(_, candidate)| candidate.thinking.iter().position(|one| one == level))
            .unwrap_or(0);
        self.seeded = true;
        self.clamp(harnesses);
    }

    /// Keep every index inside the lists the tray was given: the groups are
    /// rebuilt from configuration that can change between frames.
    pub fn clamp(&mut self, harnesses: &[ModelTrayHarness]) {
        let list = flat_models(harnesses);
        self.row = self.row.min(list.len().saturating_sub(1));
        let levels = self.level_count(harnesses);
        self.level = self.level.min(levels.saturating_sub(1));
        // The thinking column holds nothing for a model with no levels, so the
        // keyboard never sits in an empty column.
        self.column = self.column.min(if levels == 0 { 0 } else { 1 });
    }

    /// How many thinking levels the highlighted model offers.
    pub fn level_count(&self, harnesses: &[ModelTrayHarness]) -> usize {
        self.model_at(harnesses)
            .map(|model| model.thinking.len())
            .unwrap_or(0)
    }

    /// The model the selection is on, when the lists still hold it.
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

/// What a key did to the tray. The composer routes the keys and fires the host
/// callback for the outcome, so what the arrows move and what a pick applies can
/// never disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelTrayKey {
    /// The key moved the selection inside the tray.
    Moved,
    /// The key belongs to the editor.
    Ignored,
    /// Enter on a model row: apply the model at that flat index.
    AcceptModel(usize),
    /// Enter on a thinking row: apply the level at that index, named by the id
    /// its harness reports (`off`, `on`).
    AcceptLevel(String),
    /// Escape: put the tray away.
    Close,
}

/// Route one key to the tray: Left/Right move between the two lists, Up/Down
/// move the row inside the list, Enter takes the row and Escape puts the tray
/// away.
pub fn handle_tray_key(
    event: &DispatchedEvent,
    harnesses: &[ModelTrayHarness],
    state: &Rc<RefCell<ModelTrayState>>,
) -> ModelTrayKey {
    let DispatchedEvent::KeyDown { key, modifiers } = event else {
        return ModelTrayKey::Ignored;
    };
    // Cmd/Ctrl+Enter is the host's own gesture (a new conversation), so it falls
    // through to the editor even with the tray up.
    if modifiers.command || modifiers.ctrl || modifiers.shift {
        return ModelTrayKey::Ignored;
    }
    let mut state = state.borrow_mut();
    state.clamp(harnesses);
    let total = flat_models(harnesses).len();
    let levels = state.level_count(harnesses);
    match key.as_str() {
        "ArrowLeft" => {
            state.column = ModelTrayColumn::Model.index();
            ModelTrayKey::Moved
        }
        "ArrowRight" => {
            if levels > 0 {
                state.column = ModelTrayColumn::Level.index();
            }
            ModelTrayKey::Moved
        }
        "ArrowUp" | "ArrowDown" => {
            let down = key == "ArrowDown";
            match state.column() {
                ModelTrayColumn::Model => {
                    if total > 0 {
                        state.row = if down {
                            (state.row + 1).min(total - 1)
                        } else {
                            state.row.saturating_sub(1)
                        };
                        // The levels belong to the model listed above them.
                        state.level = 0;
                    }
                }
                ModelTrayColumn::Level => {
                    if levels > 0 {
                        state.level = if down {
                            (state.level + 1).min(levels - 1)
                        } else {
                            state.level.saturating_sub(1)
                        };
                    }
                }
            }
            ModelTrayKey::Moved
        }
        "Enter" | "Tab" => match state.column() {
            ModelTrayColumn::Model => ModelTrayKey::AcceptModel(state.row),
            ModelTrayColumn::Level => ModelTrayKey::AcceptLevel(state.level_at(harnesses)),
        },
        "Escape" => ModelTrayKey::Close,
        _ => ModelTrayKey::Ignored,
    }
}

/// What a drawn row stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    /// A harness header inside the model list: recognition only, never taken.
    Group(usize),
    /// A model row, by its flat index.
    Model(usize),
    /// A thinking row, by its index in the highlighted model's list.
    Level(usize),
}

/// One drawn row's bounds, and what it stands for.
#[derive(Clone, Debug)]
struct TrayRow {
    kind: RowKind,
    rect: RectF,
}

/// Where one list sits in the tray.
#[derive(Clone, Copy, Debug)]
struct ColumnBox {
    column: ModelTrayColumn,
    x: f32,
    width: f32,
}

/// What a pane hands a composer and its band: the tray's data, the selection the
/// two share, and the flag the model control toggles.
///
/// The band the [`ModelTray`] draws and the control that opens it are two
/// elements rebuilt every frame, so everything they have to agree on — which
/// rows are listed, which one the arrows are on, whether the band is up — lives
/// in this handle and is cloned into both.
#[derive(Clone)]
pub struct ComposerModelTray {
    pub harnesses: Rc<Vec<ModelTrayHarness>>,
    pub state: Rc<RefCell<ModelTrayState>>,
    /// Whether the tray is open. The model control toggles it, the band reads it.
    pub open: Rc<RefCell<bool>>,
    /// The model the pane runs and the thinking level in force, so the tray
    /// opens on them and marks them in the lists.
    pub model: String,
    pub level: String,
    /// A taken model: its flat index in the list the tray drew.
    pub on_select_model: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    /// A taken thinking level, named by the id its harness reports.
    pub on_select_level: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
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
            on_select_model: None,
            on_select_level: None,
        }
    }

    pub fn with_current(mut self, model: impl Into<String>, level: impl Into<String>) -> Self {
        self.model = model.into();
        self.level = level.into();
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

    /// The band's element, when the control has it open: the same data, the same
    /// selection, and the callbacks that put it away once a row is taken.
    pub fn band(&self) -> Option<ModelTray> {
        if !*self.open.borrow() {
            return None;
        }
        let mut tray = ModelTray::new(
            self.harnesses.as_ref().clone(),
            self.state.clone(),
            self.model.clone(),
            self.level.clone(),
        );
        if let Some(cb) = self.on_select_model.clone() {
            let open = self.open.clone();
            tray = tray.with_on_select_model(move |row| {
                (cb.borrow_mut())(row);
                *open.borrow_mut() = false;
            });
        }
        if let Some(cb) = self.on_select_level.clone() {
            let open = self.open.clone();
            tray = tray.with_on_select_level(move |level| {
                (cb.borrow_mut())(level);
                *open.borrow_mut() = false;
            });
        }
        Some(tray)
    }
}

/// The tray the composer's model control opens: the models a pane can run,
/// grouped by the harness that runs them, and the thinking levels of the
/// highlighted one.
pub struct ModelTray {
    harnesses: Vec<ModelTrayHarness>,
    state: Rc<RefCell<ModelTrayState>>,
    /// The pane's model and level in force, so the tray seeds itself on them and
    /// marks them in the lists.
    current_model: String,
    current_level: String,
    on_select_model: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_select_level: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    columns: [ColumnBox; 2],
    rows: Vec<TrayRow>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ModelTray {
    pub fn new(
        harnesses: Vec<ModelTrayHarness>,
        state: Rc<RefCell<ModelTrayState>>,
        current_model: impl Into<String>,
        current_level: impl Into<String>,
    ) -> Self {
        Self {
            harnesses,
            state,
            current_model: current_model.into(),
            current_level: current_level.into(),
            on_select_model: None,
            on_select_level: None,
            columns: [
                ColumnBox {
                    column: ModelTrayColumn::Model,
                    x: 0.0,
                    width: FALLBACK_WIDTH - LEVEL_WIDTH,
                },
                ColumnBox {
                    column: ModelTrayColumn::Level,
                    x: FALLBACK_WIDTH - LEVEL_WIDTH,
                    width: LEVEL_WIDTH,
                },
            ],
            rows: Vec::new(),
            size: None,
            origin: None,
        }
    }

    pub fn with_on_select_model<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_level<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_select_level = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Place the selection on the pane's own model, once per open.
    fn seed(&mut self) {
        let mut state = self.state.borrow_mut();
        if state.seeded {
            return;
        }
        state.seed(&self.harnesses, &self.current_model, &self.current_level);
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

    /// Where the text of a row sits vertically inside it.
    fn line_offset(font_size: f32) -> f32 {
        ((font_size * LINE_HEIGHT).max(0.0) + font_size * 0.2) * 0.5
    }

    /// A row's rect in the band's own coordinates, moved to where the band was
    /// painted.
    fn absolute(&self, rect: RectF) -> RectF {
        match self.origin {
            Some(origin) => rectf(
                rect.min_x() + origin.x(),
                rect.min_y() + origin.y(),
                rect.width(),
                rect.height(),
            ),
            None => rect,
        }
    }

    /// The drawn row under a pointer position, when it is a row that can be
    /// taken.
    fn row_at(&self, position: Vector2F) -> Option<RowKind> {
        self.rows
            .iter()
            .find(|row| contains(self.absolute(row.rect), position))
            .map(|row| row.kind)
    }

    fn panel_bounds(&self) -> Option<RectF> {
        let size = self.size?;
        let origin = self.origin?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }

    /// Take a pointer event: a move places the highlight, a press takes the row
    /// the highlight is on. A group header is recognition only, so a press on
    /// one moves nothing.
    fn pointer(&mut self, event: &DispatchedEvent) {
        let (position, pressed) = match event {
            DispatchedEvent::MouseMove { position } => (*position, false),
            DispatchedEvent::MouseDown { position, .. } => (*position, true),
            _ => return,
        };
        let Some(kind) = self.row_at(position) else {
            return;
        };
        match kind {
            RowKind::Group(_) => {}
            RowKind::Model(row) => {
                let mut state = self.state.borrow_mut();
                state.column = ModelTrayColumn::Model.index();
                if state.row != row {
                    state.row = row;
                    state.level = 0;
                }
                drop(state);
                if pressed {
                    if let Some(cb) = self.on_select_model.clone() {
                        (cb.borrow_mut())(row);
                    }
                }
            }
            RowKind::Level(level) => {
                let level_id = {
                    let mut state = self.state.borrow_mut();
                    state.column = ModelTrayColumn::Level.index();
                    let id = state
                        .model_at(&self.harnesses)
                        .and_then(|model| model.thinking.get(level))
                        .cloned()
                        .unwrap_or_default();
                    state.level = level;
                    id
                };
                if pressed {
                    if let Some(cb) = self.on_select_level.clone() {
                        (cb.borrow_mut())(level_id);
                    }
                }
            }
        }
    }
}

impl Element for ModelTray {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.seed();
        let width = if constraint.max.x.is_finite() && constraint.max.x > 0.0 {
            constraint.max.x
        } else {
            FALLBACK_WIDTH
        };
        // The thinking column gives room back when the pane is narrow, but never
        // so much that the model names lose their list.
        let mut level_width = (width * 0.32).clamp(0.0, LEVEL_WIDTH);
        if width - level_width < MODEL_MIN_WIDTH {
            level_width = (width - MODEL_MIN_WIDTH).max(0.0);
        }
        let model_width = (width - level_width).max(0.0);
        self.columns = [
            ColumnBox {
                column: ModelTrayColumn::Model,
                x: 0.0,
                width: model_width,
            },
            ColumnBox {
                column: ModelTrayColumn::Level,
                x: model_width,
                width: level_width,
            },
        ];

        let state = self.state.borrow().clone();
        let flat = flat_models(&self.harnesses);
        let total = flat.len();
        let start = Self::window_start(state.row, total);
        let end = (start + MODEL_TRAY_ROWS).min(total);

        let mut rows: Vec<TrayRow> = Vec::new();
        let mut y = HEADER_HEIGHT;
        let mut group: Option<usize> = None;
        for index in start..end {
            let Some((group_index, _)) = flat.get(index).copied() else {
                continue;
            };
            if group != Some(group_index) {
                group = Some(group_index);
                rows.push(TrayRow {
                    kind: RowKind::Group(group_index),
                    rect: rectf(0.0, y, model_width, GROUP_HEIGHT),
                });
                y += GROUP_HEIGHT;
            }
            rows.push(TrayRow {
                kind: RowKind::Model(index),
                rect: rectf(0.0, y, model_width, ROW_HEIGHT),
            });
            y += ROW_HEIGHT;
        }
        let model_height = y;

        // The thinking list belongs to the highlighted model, and windows the
        // same way when a harness reports more levels than fit.
        let levels = state.level_count(&self.harnesses);
        let level_start = Self::window_start(state.level, levels);
        let level_end = (level_start + MODEL_TRAY_ROWS).min(levels);
        let mut level_y = HEADER_HEIGHT;
        for level in level_start..level_end {
            rows.push(TrayRow {
                kind: RowKind::Level(level),
                rect: rectf(model_width, level_y, level_width, ROW_HEIGHT),
            });
            level_y += ROW_HEIGHT;
        }

        self.rows = rows;
        let list_height = model_height.max(level_y).max(HEADER_HEIGHT + ROW_HEIGHT);
        let size = Vector2F::new(width, list_height + PADDING_BOTTOM);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let panel = rectf(origin.x, origin.y, size.x, size.y);
        let state = self.state.borrow().clone();
        let columns = self.columns;
        let rows = self.rows.clone();
        let harnesses = &self.harnesses;
        let flat = flat_models(harnesses);
        let levels = state
            .model_at(harnesses)
            .map(|model| model.thinking.clone())
            .unwrap_or_default();
        let current_model = self.current_model.clone();
        let current_level = self.current_level.clone();
        let renderer = match ctx.renderer.as_mut() {
            Some(renderer) => renderer,
            None => return,
        };

        // The input's own card, in the slot above it: the band's width, the
        // theme's rounding, and the surface every menu here uses.
        let radius = app.theme.radius_px();
        renderer.fill_rounded_rect(panel, app.theme.color(ColorToken::SurfaceRaised), radius);
        renderer.stroke_rect(panel, app.theme.color(ColorToken::Border), 1.0, radius);

        // The column headers, and the hairline that sets them off the lists.
        for (index, label) in ["MODEL", "THINKING"].into_iter().enumerate() {
            let column = columns[index];
            renderer.draw_text(
                Vector2F::new(
                    origin.x + column.x + ROW_INSET,
                    origin.y + Self::line_offset(HEADER_FONT_SIZE),
                ),
                label.to_string(),
                HEADER_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                (column.width - ROW_INSET * 2.0).max(0.0),
                LINE_HEIGHT,
            );
        }
        renderer.fill_rect(
            rectf(
                origin.x + ROW_INSET,
                origin.y + HEADER_HEIGHT - 1.0,
                (size.x - ROW_INSET * 2.0).max(0.0),
                1.0,
            ),
            app.theme.color(ColorToken::Border),
        );
        // One rule between the lists, inset so it reads as a divider rather than
        // as the card's own edge.
        renderer.fill_rect(
            rectf(
                origin.x + columns[1].x,
                origin.y + HEADER_HEIGHT,
                1.0,
                (size.y - HEADER_HEIGHT - PADDING_BOTTOM).max(0.0),
            ),
            app.theme.color(ColorToken::Border),
        );

        for row in &rows {
            let bounds = self.absolute(row.rect);
            let on_level_list = matches!(row.kind, RowKind::Level(_));
            let focused = (state.column() == columns[1].column && on_level_list)
                || (state.column() == columns[0].column && !on_level_list);
            let selected = match row.kind {
                RowKind::Group(_) => false,
                RowKind::Model(index) => state.row == index,
                RowKind::Level(level) => state.level == level,
            };
            if focused && selected {
                // Inset from the card's edge and rounded with it, so the
                // highlight reads as a row of this panel.
                renderer.fill_rounded_rect(
                    rectf(
                        bounds.min_x() + 6.0,
                        bounds.min_y() + 1.0,
                        (bounds.width() - 12.0).max(0.0),
                        (bounds.height() - 2.0).max(0.0),
                    ),
                    app.theme.color(ColorToken::Selected),
                    app.theme.radius_px(),
                );
            }
            match row.kind {
                RowKind::Group(group) => {
                    let Some(harness) = harnesses.get(group) else {
                        continue;
                    };
                    let spec = goble_core::harness::harness_for(&harness.harness);
                    let mark = spec.map(|spec| spec.icon).unwrap_or("cpu");
                    let mark_color = if spec.is_some() {
                        app.theme.color(ColorToken::Text)
                    } else {
                        app.theme.color(ColorToken::Muted)
                    };
                    renderer.draw_icon(
                        Vector2F::new(
                            bounds.min_x() + ROW_INSET,
                            bounds.min_y() + (GROUP_HEIGHT - GROUP_MARK_SIZE) / 2.0,
                        ),
                        icon_atlas_name(mark),
                        GROUP_MARK_SIZE,
                        mark_color,
                    );
                    renderer.draw_text(
                        Vector2F::new(
                            bounds.min_x() + ROW_INSET + GROUP_MARK_SIZE + MARK_GAP,
                            bounds.min_y() + Self::line_offset(GROUP_FONT_SIZE),
                        ),
                        harness.label.clone(),
                        GROUP_FONT_SIZE,
                        app.theme.color(ColorToken::Muted),
                        (bounds.width() - ROW_INSET * 2.0 - GROUP_MARK_SIZE - MARK_GAP).max(0.0),
                        LINE_HEIGHT,
                    );
                }
                RowKind::Model(index) => {
                    let Some((_, model)) = flat.get(index).copied() else {
                        continue;
                    };
                    let current = model.id == current_model;
                    let color = if current {
                        app.theme.color(ColorToken::Accent)
                    } else {
                        app.theme.color(ColorToken::Text)
                    };
                    renderer.draw_text(
                        Vector2F::new(
                            bounds.min_x() + ROW_INSET,
                            bounds.min_y() + Self::line_offset(LABEL_FONT_SIZE),
                        ),
                        model.name.clone(),
                        LABEL_FONT_SIZE,
                        color,
                        (bounds.width() - ROW_INSET * 2.0).max(0.0),
                        LINE_HEIGHT,
                    );
                }
                RowKind::Level(level) => {
                    let Some(level_id) = levels.get(level) else {
                        continue;
                    };
                    let current = level_id == &current_level;
                    let color = if current {
                        app.theme.color(ColorToken::Accent)
                    } else {
                        app.theme.color(ColorToken::Text)
                    };
                    renderer.draw_text(
                        Vector2F::new(
                            bounds.min_x() + ROW_INSET,
                            bounds.min_y() + Self::line_offset(LABEL_FONT_SIZE),
                        ),
                        level_label(level_id),
                        LABEL_FONT_SIZE,
                        color,
                        (bounds.width() - ROW_INSET * 2.0).max(0.0),
                        LINE_HEIGHT,
                    );
                }
            }
        }

        // A list with nothing in it says why instead of reading as broken.
        if flat.is_empty() {
            renderer.draw_text(
                Vector2F::new(
                    origin.x + ROW_INSET,
                    origin.y + HEADER_HEIGHT + Self::line_offset(LABEL_FONT_SIZE),
                ),
                "No models configured",
                LABEL_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                (columns[0].width - ROW_INSET * 2.0).max(0.0),
                LINE_HEIGHT,
            );
        }
        if state
            .model_at(harnesses)
            .is_some_and(|model| model.thinking.is_empty())
        {
            renderer.draw_text(
                Vector2F::new(
                    origin.x + columns[1].x + ROW_INSET,
                    origin.y + HEADER_HEIGHT + Self::line_offset(LABEL_FONT_SIZE),
                ),
                "Nothing to choose",
                LABEL_FONT_SIZE,
                app.theme.color(ColorToken::Muted),
                (columns[1].width - ROW_INSET * 2.0).max(0.0),
                LINE_HEIGHT,
            );
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
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        match event {
            DispatchedEvent::MouseMove { .. } => {
                self.pointer(event);
                false
            }
            DispatchedEvent::MouseDown { position, .. } => {
                // A press anywhere on the panel belongs to the tray: it must not
                // reach the editor behind it and blur the caret the tray is
                // being picked from.
                let inside = self
                    .panel_bounds()
                    .map(|bounds| contains(bounds, *position))
                    .unwrap_or(false);
                if inside {
                    self.pointer(event);
                }
                inside
            }
            DispatchedEvent::MouseUp { position, .. } => self
                .panel_bounds()
                .map(|bounds| contains(bounds, *position))
                .unwrap_or(false),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ModifiersState;

    fn key(name: &str) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers: ModifiersState::none(),
        }
    }

    fn harnesses() -> Vec<ModelTrayHarness> {
        vec![
            ModelTrayHarness::new("goble", "Goble").with_models(vec![
                ModelTrayModel::new("deepseek-flash", "DeepSeek Flash")
                    .with_thinking(vec!["off".to_string(), "on".to_string()]),
                ModelTrayModel::new("deepseek-pro", "DeepSeek Pro")
                    .with_thinking(vec!["off".to_string(), "on".to_string()]),
            ]),
            ModelTrayHarness::new("claude", "Anthropic").with_models(vec![
                ModelTrayModel::new("claude-sonnet", "Claude Sonnet"),
            ]),
        ]
    }

    #[test]
    fn the_tray_seeds_on_the_pane_model_and_its_level() {
        let mut state = ModelTrayState::default();
        state.seed(&harnesses(), "claude-sonnet", "off");
        assert_eq!(state.column(), ModelTrayColumn::Model);
        assert_eq!(state.row, 2, "the row is counted across the harnesses");
        assert_eq!(state.level, 0);
        assert!(state.seeded);
    }

    #[test]
    fn the_arrows_move_through_the_list_and_never_into_an_empty_one() {
        let harnesses = harnesses();
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        state.borrow_mut().seed(&harnesses, "claude-sonnet", "off");
        // The last model offers no thinking levels, so the thinking column has
        // nothing to enter.
        assert_eq!(
            handle_tray_key(&key("ArrowRight"), &harnesses, &state),
            ModelTrayKey::Moved
        );
        assert_eq!(state.borrow().column(), ModelTrayColumn::Model);
        assert_eq!(
            handle_tray_key(&key("ArrowUp"), &harnesses, &state),
            ModelTrayKey::Moved
        );
        assert_eq!(state.borrow().row, 1);
        assert_eq!(
            handle_tray_key(&key("ArrowRight"), &harnesses, &state),
            ModelTrayKey::Moved
        );
        assert_eq!(state.borrow().column(), ModelTrayColumn::Level);
        assert_eq!(
            handle_tray_key(&key("ArrowDown"), &harnesses, &state),
            ModelTrayKey::Moved
        );
        assert_eq!(state.borrow().level, 1);
    }

    #[test]
    fn enter_takes_a_model_then_a_level() {
        let harnesses = harnesses();
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        state
            .borrow_mut()
            .seed(&harnesses, "deepseek-flash", "off");
        assert_eq!(
            handle_tray_key(&key("Enter"), &harnesses, &state),
            ModelTrayKey::AcceptModel(0)
        );
        assert_eq!(
            handle_tray_key(&key("ArrowRight"), &harnesses, &state),
            ModelTrayKey::Moved
        );
        assert_eq!(state.borrow().column(), ModelTrayColumn::Level);
        assert_eq!(
            handle_tray_key(&key("Enter"), &harnesses, &state),
            ModelTrayKey::AcceptLevel("off".to_string())
        );
        assert_eq!(
            handle_tray_key(&key("Escape"), &harnesses, &state),
            ModelTrayKey::Close
        );
    }

    #[test]
    fn a_command_key_never_comes_to_the_tray() {
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        let event = DispatchedEvent::KeyDown {
            key: "Enter".to_string(),
            modifiers: ModifiersState {
                command: true,
                ..ModifiersState::none()
            },
        };
        assert_eq!(
            handle_tray_key(&event, &harnesses(), &state),
            ModelTrayKey::Ignored,
            "Cmd+Enter is the host's new-conversation gesture"
        );
    }

    #[test]
    fn the_window_keeps_the_selected_model_and_its_harness_header_visible() {
        let app = AppContext::default();
        let models: Vec<ModelTrayModel> = (0..20)
            .map(|index| ModelTrayModel::new(format!("m{index}"), format!("Model {index}")))
            .collect();
        let harnesses = vec![
            ModelTrayHarness::new("goble", "Goble").with_models(models),
            ModelTrayHarness::new("claude", "Anthropic").with_models(vec![
                ModelTrayModel::new("claude-sonnet", "Claude Sonnet")
                    .with_thinking(vec!["off".to_string()]),
            ]),
        ];
        let state = Rc::new(RefCell::new(ModelTrayState::default()));
        state.borrow_mut().seed(&harnesses, "m17", "off");
        let mut tray = ModelTray::new(harnesses, state.clone(), "m17", "off");
        tray.layout(
            SizeConstraint::loose(vec2f(520.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let drawn = tray.rows.clone();
        assert!(
            drawn
                .iter()
                .any(|row| row.kind == RowKind::Model(17)),
            "the selected row is drawn"
        );
        let first = drawn.first().map(|row| row.kind);
        assert!(
            matches!(first, Some(RowKind::Group(0))),
            "the window opens with the group header that owns the rows under it"
        );
    }

    #[test]
    fn a_level_is_spelled_the_way_a_person_reads_it() {
        assert_eq!(level_label("off"), "Off");
        assert_eq!(level_label("on"), "On");
        assert_eq!(level_label("medium"), "Medium");
        assert_eq!(level_label(""), "");
    }
}
