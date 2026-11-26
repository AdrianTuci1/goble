//! The Appearance page's two anchored menus: the theme-preset dropdown and the
//! per-channel colour palette.
//!
//! Both menus are drawn by the page-level [`OverlayLayer`], not by the row that
//! triggers them. The settings tree is rebuilt every frame and a row's control
//! is painted before the rows below it, so a panel drawn inside a row would be
//! painted over by the next row. The trigger records its page-space rectangle in
//! [`AppearanceMenuAnchors`], and the overlay paints the one open panel after
//! every section and routes pointer/key events to it first.
//!
//! The menus are ordinary elements, not fake state: they register with the
//! window's [`OpenMenus`](goble_ui::elements::OpenMenus) registry, so a press
//! outside or Escape closes them through one path, and the palette's grid and
//! hex field dispatch the same `SetText`/`Reset`/`SetSelect` actions as every
//! other scalar row. Nothing here writes a file directly.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::settings::{SettingsButton, SettingsTextField, SETTINGS_CONTROL_RADIUS};
use goble_ui::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, Element, EventContext, Fill,
    Flex, Icon, IconButton, LayoutContext, MainAxisAlignment, MainAxisSize, MenuClose, MenuId,
    MenuPointer, PaintContext, Point, SizeConstraint, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorScheme, ColorToken, Theme, ThemePreset};

use super::super::fields;
use super::super::state::{
    AppearanceMenuAnchors, SettingsAction, SettingsActions, SettingsUiState,
};

/// The theme menu's stable registry id.
const THEME_MENU_ID: MenuId = "settings.appearance.theme";
/// The colour palette's stable registry id.
const PALETTE_MENU_ID: MenuId = "settings.appearance.palette";

/// The trigger and panel geometry. The panel is wide enough for six swatches a
/// row and a 160pt hex field.
const TRIGGER_HEIGHT: f32 = 32.0;
const THEME_TRIGGER_WIDTH: f32 = 240.0;
const PALETTE_TRIGGER_WIDTH: f32 = 196.0;
const SWATCH_TRIGGER: f32 = 24.0;
const PANEL_WIDTH: f32 = 288.0;
const PANEL_PADDING: f32 = 10.0;
const PANEL_GAP: f32 = 6.0;
const PANEL_EDGE_MARGIN: f32 = 8.0;
const MENU_ROW_HEIGHT: f32 = 34.0;
const GRID_COLUMNS: usize = 6;
const GRID_CELL_HEIGHT: f32 = 32.0;
const GRID_SWATCH: f32 = 26.0;

/// Place a popover against its measured trigger, inside the page's visible
/// rectangle. It prefers the space under the trigger, flips above when that
/// would run past the bottom, and clamps both axes so the panel stays inside
/// the settings scroll viewport (a panel painted outside it is clipped away).
fn place_popover(anchor: RectF, panel: Vector2F, viewport: RectF) -> Vector2F {
    let min_x = viewport.min_x() + PANEL_EDGE_MARGIN;
    let max_x = (viewport.max_x() - panel.x - PANEL_EDGE_MARGIN).max(min_x);
    let x = (anchor.max_x() - panel.x).clamp(min_x, max_x);

    let min_y = viewport.min_y() + PANEL_EDGE_MARGIN;
    let max_y = (viewport.max_y() - panel.y - PANEL_EDGE_MARGIN).max(min_y);
    let below = anchor.max_y() + PANEL_GAP;
    let above = anchor.min_y() - PANEL_GAP - panel.y;
    let y = if below <= max_y {
        below
    } else if above >= min_y {
        above
    } else if (anchor.min_y() - viewport.min_y()) >= (viewport.max_y() - anchor.max_y()) {
        // Neither side fits: use the larger half of the viewport and let the
        // clamp keep the top edge visible.
        above
    } else {
        below
    };
    vec2f(x, y.clamp(min_y, max_y))
}

/// The draft id marking which channel's hex field holds the caret. It is a UI
/// marker, not a setting: the real value is written to `theme.<channel>`.
fn hex_focus_draft(channel: usize) -> String {
    format!("appearance.palette.hex.{channel}")
}

/// The draft id the palette's keyboard highlight is parked in. Like the open
/// flags, it lives outside the element because the tree is rebuilt each frame.
fn palette_highlight_draft() -> &'static str {
    "appearance.palette.highlight"
}

/// The draft id the theme menu's keyboard highlight is parked in.
fn theme_highlight_draft() -> &'static str {
    "appearance.theme.highlight"
}

/// One selectable colour in the palette grid.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteSwatch {
    pub label: String,
    pub hex: String,
    pub color: ColorU,
}

/// The base scheme the page is showing: the stored preset, or the built-in
/// dark/light base when the file has no preset.
fn active_scheme(state: &SettingsUiState) -> ColorScheme {
    preset_of(state)
        .map(ThemePreset::colors)
        .unwrap_or_else(|| {
            if fields::theme_bool(state, "dark", true) {
                Theme::dark().colors
            } else {
                Theme::light().colors
            }
        })
}

fn preset_of(state: &SettingsUiState) -> Option<ThemePreset> {
    fields::theme_text(state, "preset")
        .as_deref()
        .and_then(ThemePreset::from_key)
}

/// The palette's accent: the preset's own accent, or the built-in base's.
fn active_accent(state: &SettingsUiState) -> ColorU {
    preset_of(state)
        .map(ThemePreset::accent)
        .unwrap_or_else(|| {
            if fields::theme_bool(state, "dark", true) {
                Theme::dark().accent_color()
            } else {
                Theme::light().accent_color()
            }
        })
}

/// The effective colour of `theme.<key>`: the file's override when it parses,
/// otherwise the active palette's own value for that channel.
pub fn channel_color(state: &SettingsUiState, key: &str) -> ColorU {
    if let Some(hex) = fields::theme_text(state, key) {
        if let Some(color) = ColorU::from_hex(&hex) {
            return color;
        }
    }
    let scheme = active_scheme(state);
    match key {
        "primary" => scheme.text,
        "secondary" => scheme.muted,
        "accent" => active_accent(state),
        "bg" => scheme.bg,
        "surface" => scheme.surface,
        _ => scheme.text,
    }
}

/// The channel index and stored key of a `theme.<channel>` path.
pub fn channel_for_path(path: &str) -> Option<(usize, &'static str)> {
    match path.rsplit('.').next()? {
        "accent" => Some((0, "accent")),
        "primary" => Some((1, "primary")),
        "secondary" => Some((2, "secondary")),
        "bg" => Some((3, "bg")),
        "surface" => Some((4, "surface")),
        _ => None,
    }
}

/// The palette grid: the five preset accents first, then the active palette's
/// semantic tokens. Duplicate colours are folded together so the grid has one
/// cell per real choice, not one per token name that happens to share a value.
pub fn palette_swatches(state: &SettingsUiState) -> Vec<PaletteSwatch> {
    let scheme = active_scheme(state);
    let accent = active_accent(state);
    let mut out: Vec<PaletteSwatch> = Vec::new();
    let mut push = |label: &str, color: ColorU| {
        let hex = color.to_hex_string();
        if out.iter().any(|entry| entry.hex == hex) {
            return;
        }
        out.push(PaletteSwatch {
            label: label.to_string(),
            hex,
            color,
        });
    };

    for preset in ThemePreset::ALL {
        push(preset.display_name(), preset.accent());
    }
    push("Accent", accent);
    push("Text", scheme.text);
    push("Muted", scheme.muted);
    push("Background", scheme.bg);
    push("Surface", scheme.surface);
    push("Raised", scheme.surface_raised);
    push("Border", scheme.border);
    push("Focus", scheme.focus);
    push("Success", scheme.success);
    push("Warning", scheme.warning);
    push("Path", scheme.path);
    push("Command", scheme.command);
    push("Error", scheme.error);
    push("Badge", scheme.badge);
    out
}

/// The palette's open channel, from the element-owned cell.
pub fn palette_open_channel(state: &SettingsUiState) -> Option<usize> {
    *state.color_palette_menu.borrow()
}

/// Whether the theme menu is up. The element-owned cell is the source of truth;
/// the state's plain flag is honoured too, so a direct state change (a test or
/// the older page) still draws the menu.
pub fn theme_menu_open(state: &SettingsUiState) -> bool {
    *state.theme_select_menu.borrow() || state.theme_select_open
}

fn dispatch(actions: &SettingsActions, action: SettingsAction) {
    actions.dispatch(action);
}

fn set_draft(actions: &SettingsActions, id: &str, value: impl Into<String>) {
    dispatch(
        actions,
        SettingsAction::SetDraft {
            id: id.to_string(),
            value: value.into(),
        },
    );
}

fn close_theme_menu(state: &SettingsUiState, actions: &SettingsActions) {
    *state.theme_select_menu.borrow_mut() = false;
    dispatch(actions, SettingsAction::ToggleThemeSelect(false));
}

fn open_theme_menu(state: &SettingsUiState, actions: &SettingsActions) {
    *state.color_palette_menu.borrow_mut() = None;
    set_draft(actions, &hex_focus_draft(0), "");
    *state.theme_select_menu.borrow_mut() = true;
    dispatch(actions, SettingsAction::ToggleThemeSelect(true));
    let selected = fields::palette_index(state);
    set_draft(actions, theme_highlight_draft(), selected.to_string());
}

fn close_palette_menu(state: &SettingsUiState, actions: &SettingsActions) {
    let channel = state.color_palette_menu.borrow_mut().take();
    if let Some(channel) = channel {
        set_draft(actions, &hex_focus_draft(channel), "");
    }
    dispatch(
        actions,
        SettingsAction::SetDraft {
            id: "appearance.palette.open".to_string(),
            value: String::new(),
        },
    );
}

fn open_palette_menu(
    state: &SettingsUiState,
    actions: &SettingsActions,
    channel: usize,
    selected: usize,
) {
    *state.theme_select_menu.borrow_mut() = false;
    dispatch(actions, SettingsAction::ToggleThemeSelect(false));
    *state.color_palette_menu.borrow_mut() = Some(channel);
    dispatch(
        actions,
        SettingsAction::SetDraft {
            id: "appearance.palette.open".to_string(),
            value: channel.to_string(),
        },
    );
    set_draft(actions, palette_highlight_draft(), selected.to_string());
}

/// The theme-preset control: a 32pt trigger showing the current palette, its
/// swatch and a chevron. The panel itself is drawn by [`AppearanceMenuLayer`].
pub struct ThemeSelectControl {
    options: Vec<String>,
    selected: usize,
    open: bool,
    focused: bool,
    disabled: bool,
    swatch: Option<ColorU>,
    anchors: Rc<RefCell<AppearanceMenuAnchors>>,
    state: SettingsUiState,
    actions: SettingsActions,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ThemeSelectControl {
    #[allow(clippy::too_many_arguments)]
    fn new(
        state: &SettingsUiState,
        actions: &SettingsActions,
        options: Vec<String>,
        selected: usize,
        swatch: Option<ColorU>,
        disabled: bool,
        focused: bool,
    ) -> Self {
        Self {
            options,
            selected,
            open: theme_menu_open(state),
            focused,
            disabled,
            swatch,
            anchors: Rc::clone(&state.appearance_anchors),
            state: state.clone(),
            actions: actions.clone(),
            size: None,
            origin: None,
        }
    }

    fn label(&self) -> String {
        self.options
            .get(self.selected)
            .cloned()
            .unwrap_or_else(|| "Default base".to_string())
    }

    fn toggle(&mut self) {
        if self.open {
            close_theme_menu(&self.state, &self.actions);
            self.open = false;
        } else {
            open_theme_menu(&self.state, &self.actions);
            self.open = true;
        }
    }
}

impl Element for ThemeSelectControl {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(THEME_TRIGGER_WIDTH, TRIGGER_HEIGHT);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self
            .size
            .unwrap_or(vec2f(THEME_TRIGGER_WIDTH, TRIGGER_HEIGHT));
        self.anchors.borrow_mut().theme = Some(rectf(origin.x, origin.y, size.x, size.y));
        let fill = if self.disabled {
            app.theme
                .color(ColorToken::SurfaceRaised)
                .mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            app.theme
                .color(ColorToken::SurfaceRaised)
                .mix(&app.theme.color(ColorToken::Bg), 0.45)
        };
        let border = if self.open || (self.focused && !self.disabled) {
            app.theme.color(ColorToken::Focus)
        } else if self.disabled {
            app.theme
                .color(ColorToken::Border)
                .mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            app.theme
                .color(ColorToken::Border)
                .mix(&app.theme.color(ColorToken::Text), 0.08)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            let rect = rectf(origin.x, origin.y, size.x, size.y);
            renderer.fill_rounded_rect(rect, fill, SETTINGS_CONTROL_RADIUS);
            renderer.stroke_rect(rect, border, 1.0, SETTINGS_CONTROL_RADIUS);
        }
        let text_color = if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else {
            app.theme.color(ColorToken::Text)
        };
        let mut x = origin.x + 10.0;
        if let Some(color) = self.swatch {
            let swatch = rectf(x, origin.y + (size.y - 14.0) / 2.0, 14.0, 14.0);
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rounded_rect(swatch, color, 4.0);
                renderer.stroke_rect(
                    swatch,
                    app.theme
                        .color(ColorToken::Border)
                        .mix(&app.theme.color(ColorToken::Text), 0.2),
                    1.0,
                    4.0,
                );
            }
            x += 20.0;
        }
        let mut label = Text::new(self.label())
            .with_font_size(13.0)
            .with_line_height(1.4)
            .with_color(text_color)
            .finish();
        let inner = SizeConstraint::loose(vec2f((size.x - (x - origin.x) - 26.0).max(0.0), size.y));
        let label_size = label.layout(inner, &mut LayoutContext::default(), app);
        let y = origin.y + (size.y - label_size.y).max(0.0) / 2.0;
        label.paint(vec2f(x, y), ctx, app);

        let mut chevron = Icon::new(if self.open {
            "chevron-up"
        } else {
            "chevron-down"
        })
        .with_size(12.0)
        .with_color(app.theme.color(ColorToken::Muted))
        .finish();
        let _ = chevron.layout(
            SizeConstraint::tight(vec2f(12.0, 12.0)),
            &mut LayoutContext::default(),
            app,
        );
        chevron.paint(
            vec2f(
                origin.x + size.x - 10.0 - 12.0,
                origin.y + (size.y - 12.0) / 2.0,
            ),
            ctx,
            app,
        );
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
        if self.disabled {
            return false;
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };
        match event {
            DispatchedEvent::MouseDown { position, button } if *button == 0 => {
                if bounds.contains(goble_ui::geometry::PointF::new(position.x, position.y)) {
                    self.toggle();
                    true
                } else {
                    false
                }
            }
            DispatchedEvent::KeyDown { key, modifiers } => {
                if modifiers.command || modifiers.ctrl || modifiers.alt {
                    return false;
                }
                if !self.open
                    && matches!(
                        key.as_str(),
                        "ArrowDown" | "ArrowUp" | "Enter" | " " | "Space"
                    )
                {
                    self.toggle();
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}

/// Build the theme trigger from the page's `Select` model.
pub fn theme_select_control(
    state: &SettingsUiState,
    actions: &SettingsActions,
    options: &[String],
    selected: usize,
    swatch: Option<ColorU>,
    disabled: bool,
    focused: bool,
) -> Box<dyn Element> {
    Box::new(ThemeSelectControl::new(
        state,
        actions,
        options.to_vec(),
        selected,
        swatch,
        disabled,
        focused,
    ))
}

/// A channel's row control: the editable hex field plus the colour square that
/// opens the palette. The panel is drawn by [`AppearanceMenuLayer`].
pub struct ColorPaletteControl {
    path: String,
    channel: usize,
    value: String,
    color: ColorU,
    open: bool,
    focused: bool,
    disabled: bool,
    hex_focused: bool,
    anchors: Rc<RefCell<AppearanceMenuAnchors>>,
    state: SettingsUiState,
    actions: SettingsActions,
    field: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ColorPaletteControl {
    #[allow(clippy::too_many_arguments)]
    fn new(
        state: &SettingsUiState,
        actions: &SettingsActions,
        path: &str,
        channel: usize,
        value: String,
        color: ColorU,
        disabled: bool,
        focused: bool,
    ) -> Self {
        let hex_focused = state.draft(&hex_focus_draft(channel), "") == "1";
        Self {
            path: path.to_string(),
            channel,
            value,
            color,
            open: *state.color_palette_menu.borrow() == Some(channel),
            focused,
            disabled,
            hex_focused,
            anchors: Rc::clone(&state.appearance_anchors),
            state: state.clone(),
            actions: actions.clone(),
            field: None,
            size: None,
            origin: None,
        }
    }

    /// The swatch sits INSIDE the hex input, at its leading edge: the field
    /// draws a leading box and the swatch fills it, so the value starts after
    /// the square instead of the square sitting beside the row title.
    fn swatch_rect(&self, origin: Vector2F) -> RectF {
        const FIELD_INSET: f32 = 10.0;
        rectf(
            origin.x + FIELD_INSET,
            origin.y + (TRIGGER_HEIGHT - SWATCH_TRIGGER) / 2.0,
            SWATCH_TRIGGER,
            SWATCH_TRIGGER,
        )
    }

    fn field_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.field.is_none() {
            let id_change = self.path.clone();
            let id_submit = self.path.clone();
            let focus_id = hex_focus_draft(self.channel);
            let change_dispatch = self.actions.dispatch.clone();
            let submit_dispatch = self.actions.dispatch.clone();
            let focus_dispatch = self.actions.dispatch.clone();
            let placeholder = if self.value.trim().is_empty() {
                self.color.to_hex_string()
            } else {
                "#rrggbb".to_string()
            };
            let field = SettingsTextField::new()
                .with_value(self.value.clone())
                .with_placeholder(placeholder)
                .with_width(PALETTE_TRIGGER_WIDTH)
                // Reserve the leading box the swatch paints into; the text
                // starts after it, inside the same input casing.
                .with_leading(
                    goble_ui::elements::Empty::new().finish(),
                    SWATCH_TRIGGER + 8.0,
                )
                .with_focused(self.hex_focused && !self.disabled)
                .with_disabled(self.disabled)
                .with_on_change(move |value| {
                    (change_dispatch.borrow_mut())(SettingsAction::SetDraft {
                        id: id_change.clone(),
                        value,
                    });
                })
                .with_on_submit(move || {
                    (submit_dispatch.borrow_mut())(SettingsAction::CommitField {
                        id: id_submit.clone(),
                    });
                })
                .with_on_focus_change(move |focused| {
                    (focus_dispatch.borrow_mut())(SettingsAction::SetDraft {
                        id: focus_id.clone(),
                        value: if focused {
                            "1".to_string()
                        } else {
                            String::new()
                        },
                    });
                });
            let _ = app;
            self.field = Some(field.finish());
        }
        self.field.as_mut().unwrap()
    }

    fn toggle(&mut self) {
        if self.open {
            close_palette_menu(&self.state, &self.actions);
            self.open = false;
        } else {
            let selected = palette_swatches(&self.state)
                .iter()
                .position(|entry| entry.hex == self.color.to_hex_string())
                .unwrap_or(0);
            open_palette_menu(&self.state, &self.actions, self.channel, selected);
            self.open = true;
        }
    }
}

impl Element for ColorPaletteControl {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let field = self.field_mut(app);
        let field_size = field.layout(constraint, ctx, app);
        let size = vec2f(PALETTE_TRIGGER_WIDTH, TRIGGER_HEIGHT.max(field_size.y));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self
            .size
            .unwrap_or(vec2f(PALETTE_TRIGGER_WIDTH, TRIGGER_HEIGHT));
        // The palette is anchored to the in-field swatch, so the flip/clamp
        // maths places it under the square rather than under the whole row.
        self.anchors
            .borrow_mut()
            .palette
            .insert(self.channel, self.swatch_rect(origin));

        let field = self.field_mut(app);
        field.paint(origin, ctx, app);

        let swatch = self.swatch_rect(origin);
        let fill = if self.disabled {
            app.theme
                .color(ColorToken::SurfaceRaised)
                .mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            self.color
        };
        let border = if self.open || (self.focused && !self.disabled) {
            app.theme.color(ColorToken::Focus)
        } else {
            app.theme
                .color(ColorToken::Border)
                .mix(&app.theme.color(ColorToken::Text), 0.2)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(swatch, fill, 4.0);
            renderer.stroke_rect(swatch, border, if self.open { 2.0 } else { 1.0 }, 4.0);
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };
        if let DispatchedEvent::MouseDown { position, button } = event {
            let point = goble_ui::geometry::PointF::new(position.x, position.y);
            let origin = vec2f(bounds.min_x(), bounds.min_y());
            if *button == 0 && self.swatch_rect(origin).contains(point) {
                self.toggle();
                return true;
            }
        }
        // The hex field is the row's real text control; a click or a key it
        // takes must not be re-read as the swatch toggle.
        if self.field_mut(app).dispatch_event(event, ctx, app) {
            return true;
        }
        if let DispatchedEvent::KeyDown { key, modifiers } = event {
            if modifiers.command || modifiers.ctrl || modifiers.alt {
                return false;
            }
            if !self.open && self.focused && matches!(key.as_str(), "ArrowDown" | "ArrowUp") {
                self.toggle();
                return true;
            }
        }
        false
    }
}

/// Build a channel row's hex field + swatch trigger.
#[allow(clippy::too_many_arguments)]
pub fn color_palette_control(
    state: &SettingsUiState,
    actions: &SettingsActions,
    path: &str,
    channel: usize,
    value: String,
    color: ColorU,
    disabled: bool,
    focused: bool,
) -> Box<dyn Element> {
    Box::new(ColorPaletteControl::new(
        state, actions, path, channel, value, color, disabled, focused,
    ))
}

// ---------------------------------------------------------------------------
// The page-level overlay
// ---------------------------------------------------------------------------

/// Draws `content` first and the one open Appearance menu after it, and routes
/// events to the menu before the content. This is the paint order a floating
/// menu needs: the row that owns the trigger is usually *above* the rows the
/// panel covers, so a panel painted in row order would be covered.
pub struct OverlayLayer {
    content: Box<dyn Element>,
    overlay: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl OverlayLayer {
    pub fn new(content: Box<dyn Element>, overlay: Option<Box<dyn Element>>) -> Self {
        Self {
            content,
            overlay,
            size: None,
            origin: None,
        }
    }
}

impl Element for OverlayLayer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.content.layout(constraint, ctx, app);
        if let Some(overlay) = self.overlay.as_mut() {
            // The overlay paints at an absolute anchor; its own flow box is
            // zero-sized so it cannot push the page around. Hand it the page's
            // constraint so it has a fallback viewport before the scroll region
            // has measured one.
            overlay.layout(
                SizeConstraint::loose(vec2f(constraint.max.x, constraint.max.y)),
                ctx,
                app,
            );
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.content.paint(origin, ctx, app);
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.paint(origin, ctx, app);
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if let Some(overlay) = self.overlay.as_mut() {
            if overlay.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        self.content.dispatch_event(event, ctx, app)
    }
}

enum MenuKind {
    Theme {
        options: Vec<String>,
        selected: usize,
        highlight: usize,
    },
    Palette {
        path: String,
        channel: usize,
        label: String,
        value: String,
        entries: Vec<PaletteSwatch>,
        selected: Option<usize>,
        highlight: usize,
        hex_focused: bool,
    },
}

/// The one open Appearance menu, painted at its trigger's anchor.
struct AppearanceMenuLayer {
    kind: MenuKind,
    anchors: Rc<RefCell<AppearanceMenuAnchors>>,
    theme_open: Rc<RefCell<bool>>,
    palette_open: Rc<RefCell<Option<usize>>>,
    state: SettingsUiState,
    actions: SettingsActions,
    panel: Option<Box<dyn Element>>,
    panel_size: Option<Vector2F>,
    panel_position: Option<Vector2F>,
    panel_bounds: Option<RectF>,
    origin: Option<Point>,
}

impl AppearanceMenuLayer {
    fn anchor(&self) -> Option<RectF> {
        match self.kind {
            MenuKind::Theme { .. } => self.anchors.borrow().theme,
            MenuKind::Palette { channel, .. } => {
                self.anchors.borrow().palette.get(&channel).copied()
            }
        }
    }

    fn menu_id(&self) -> MenuId {
        match self.kind {
            MenuKind::Theme { .. } => THEME_MENU_ID,
            MenuKind::Palette { .. } => PALETTE_MENU_ID,
        }
    }

    fn close_handle(&self) -> MenuClose {
        let actions = self.actions.clone();
        let theme_open = Rc::clone(&self.theme_open);
        let palette_open = Rc::clone(&self.palette_open);
        let is_theme = matches!(self.kind, MenuKind::Theme { .. });
        Rc::new(RefCell::new(move || {
            if is_theme {
                *theme_open.borrow_mut() = false;
                dispatch(&actions, SettingsAction::ToggleThemeSelect(false));
            } else {
                let channel = palette_open.borrow_mut().take();
                if let Some(channel) = channel {
                    set_draft(&actions, &hex_focus_draft(channel), "");
                }
            }
        }))
    }

    fn build_panel(&self, app: &AppContext) -> Box<dyn Element> {
        match &self.kind {
            MenuKind::Theme {
                options,
                selected,
                highlight,
            } => theme_panel(
                app,
                options,
                *selected,
                *highlight,
                self.state.clone(),
                self.actions.clone(),
            ),
            MenuKind::Palette {
                path,
                channel,
                label,
                value,
                entries,
                selected,
                highlight,
                hex_focused,
            } => palette_panel(
                app,
                path,
                *channel,
                label,
                value,
                entries,
                *selected,
                *highlight,
                *hex_focused,
                self.state.clone(),
                self.actions.clone(),
            ),
        }
    }

    fn move_highlight(&mut self, delta: i32, columns: bool) {
        let (len, current, step) = match &mut self.kind {
            MenuKind::Theme {
                options, highlight, ..
            } => (options.len(), highlight, 1),
            MenuKind::Palette {
                entries, highlight, ..
            } => (
                entries.len(),
                highlight,
                if columns { GRID_COLUMNS } else { 1 },
            ),
        };
        if len == 0 {
            return;
        }
        let next = (*current as i64 + delta as i64 * step as i64).rem_euclid(len as i64) as usize;
        *current = next;
        let id = match self.kind {
            MenuKind::Theme { .. } => theme_highlight_draft(),
            MenuKind::Palette { .. } => palette_highlight_draft(),
        };
        set_draft(&self.actions, id, next.to_string());
    }

    fn commit_highlight(&mut self) {
        match &self.kind {
            MenuKind::Theme { highlight, .. } => {
                let index = *highlight;
                *self.theme_open.borrow_mut() = false;
                dispatch(
                    &self.actions,
                    SettingsAction::SetSelect {
                        path: "theme.preset".to_string(),
                        index,
                    },
                );
            }
            MenuKind::Palette {
                path,
                channel,
                entries,
                highlight,
                ..
            } => {
                if let Some(entry) = entries.get(*highlight) {
                    let hex = entry.hex.clone();
                    dispatch(
                        &self.actions,
                        SettingsAction::SetText {
                            path: path.clone(),
                            value: hex,
                        },
                    );
                    dispatch(
                        &self.actions,
                        SettingsAction::ClearField { id: path.clone() },
                    );
                }
                let channel = *channel;
                *self.palette_open.borrow_mut() = None;
                set_draft(&self.actions, &hex_focus_draft(channel), "");
            }
        }
    }

    fn handle_key(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        if modifiers.command || modifiers.ctrl || modifiers.alt {
            return false;
        }
        // The text field gets first refusal while it holds the caret: printable
        // keys build the hex string, Enter commits it, and Backspace deletes.
        let hex_focused = matches!(
            &self.kind,
            MenuKind::Palette {
                hex_focused: true,
                ..
            }
        );
        if hex_focused {
            if let Some(panel) = self.panel.as_mut() {
                if panel.dispatch_event(event, ctx, app) {
                    return true;
                }
            }
            // Arrow keys move the grid's highlight even while the field is
            // focused, but they are still the menu's keys (not the page's).
            if matches!(
                key.as_str(),
                "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown"
            ) {
                let columns = matches!(key.as_str(), "ArrowUp" | "ArrowDown");
                let delta = match key.as_str() {
                    "ArrowLeft" | "ArrowUp" => -1,
                    _ => 1,
                };
                self.move_highlight(delta, columns);
                return true;
            }
        }
        match key.as_str() {
            "ArrowDown" => {
                self.move_highlight(1, true);
                true
            }
            "ArrowUp" => {
                self.move_highlight(-1, true);
                true
            }
            "ArrowRight" => {
                self.move_highlight(1, false);
                true
            }
            "ArrowLeft" => {
                self.move_highlight(-1, false);
                true
            }
            "Enter" | " " | "Space" => {
                self.commit_highlight();
                true
            }
            "Escape" => {
                match &self.kind {
                    MenuKind::Theme { .. } => {
                        *self.theme_open.borrow_mut() = false;
                        dispatch(&self.actions, SettingsAction::ToggleThemeSelect(false));
                    }
                    MenuKind::Palette { channel, .. } => {
                        let channel = *channel;
                        *self.palette_open.borrow_mut() = None;
                        set_draft(&self.actions, &hex_focus_draft(channel), "");
                    }
                }
                true
            }
            _ => false,
        }
    }
}

impl Element for AppearanceMenuLayer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.panel = None;
        self.panel_size = None;
        self.panel_position = None;
        self.panel_bounds = None;
        let Some(anchor) = self.anchor() else {
            return Vector2F::zero();
        };
        let mut panel = self.build_panel(app);
        let size = panel.layout(SizeConstraint::loose(vec2f(PANEL_WIDTH, 600.0)), ctx, app);
        // The scroll region records the viewport it painted through; that is
        // the rectangle the panel must stay inside or it is clipped. On the
        // first frame (or in a direct element test) fall back to the layout
        // constraint, widened to the anchor when an axis is unbounded.
        // The scroll region's painted viewport is the true clip rectangle. A
        // stale viewport from before the sheet was measured (or a page whose
        // overlay now sits outside the scroll) can have the anchor outside it;
        // fall back to a rectangle that right-aligns the panel to the trigger
        // and opens it below, which keeps it on the same sheet.
        let anchor_point = goble_ui::geometry::PointF::new(anchor.min_x(), anchor.min_y());
        let viewport = self
            .state
            .scroll
            .borrow()
            .painted_viewport()
            .filter(|viewport| {
                viewport.width() > 0.0 && viewport.height() > 0.0 && viewport.contains(anchor_point)
            })
            .unwrap_or_else(|| {
                let width = if constraint.max.x.is_finite() {
                    constraint.max.x.max(anchor.max_x() + PANEL_EDGE_MARGIN)
                } else {
                    anchor.max_x() + PANEL_EDGE_MARGIN
                };
                let height = if constraint.max.y.is_finite() {
                    constraint
                        .max
                        .y
                        .max(anchor.max_y() + PANEL_GAP + size.y + PANEL_EDGE_MARGIN)
                } else {
                    anchor.max_y() + PANEL_GAP + size.y + PANEL_EDGE_MARGIN
                };
                rectf(0.0, 0.0, width, height)
            });
        let position = place_popover(anchor, size, viewport);
        self.panel_position = Some(position);
        self.panel_size = Some(size);
        self.panel = Some(panel);
        Vector2F::zero()
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        // A close fired during the event pass may have cleared the shared flag
        // after this element was built. Do not repaint (and re-register) a menu
        // the user just dismissed.
        let still_open = match &self.kind {
            MenuKind::Theme { .. } => *self.theme_open.borrow() || self.state.theme_select_open,
            MenuKind::Palette { .. } => self.palette_open.borrow().is_some(),
        };
        if !still_open {
            return;
        }
        let (Some(position), Some(size), Some(panel)) =
            (self.panel_position, self.panel_size, self.panel.as_mut())
        else {
            return;
        };
        // `position` is already in window coordinates (the anchor and the
        // viewport are), so the panel is painted there directly. Subtracting
        // the layer's own origin would shift it left/up by that origin when the
        // layer is mounted outside the scroll content (the settings overlay).
        let _ = origin;
        panel.paint(position, ctx, app);
        let bounds = rectf(position.x, position.y, size.x, size.y);
        self.panel_bounds = Some(bounds);

        let id = self.menu_id();
        let close = self.close_handle();
        if app.open_menus.id() != Some(id) {
            app.open_menus.open(id, close.clone());
        }
        app.open_menus
            .report(id, close, vec![bounds], MenuPointer::Paint);
        if let Some(anchor) = self.anchor() {
            app.open_menus.report_anchor(id, anchor);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        Some(Vector2F::zero())
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
        if self.panel.is_none() {
            return false;
        }
        if let DispatchedEvent::KeyDown { .. } = event {
            return self.handle_key(event, ctx, app);
        }
        if let Some(bounds) = self.panel_bounds {
            let position = match event {
                DispatchedEvent::MouseDown { position, .. }
                | DispatchedEvent::MouseUp { position, .. }
                | DispatchedEvent::MouseMove { position } => Some(*position),
                _ => None,
            };
            if let Some(position) = position {
                if bounds.contains(goble_ui::geometry::PointF::new(position.x, position.y)) {
                    if let Some(panel) = self.panel.as_mut() {
                        let _ = panel.dispatch_event(event, ctx, app);
                    }
                    return true;
                }
            }
        }
        false
    }
}

/// Build the page's overlay for whichever Appearance menu is open.
pub fn appearance_menu_layer(
    app: &AppContext,
    state: &SettingsUiState,
    actions: &SettingsActions,
) -> Option<Box<dyn Element>> {
    if let Some(channel) = palette_open_channel(state) {
        let (path, key) = channel_pair(channel);
        let stored = fields::theme_text(state, key).unwrap_or_default();
        let value = state.draft(&format!("theme.{key}"), &stored).to_string();
        let entries = palette_swatches(state);
        let color = channel_color(state, key);
        let selected = entries
            .iter()
            .position(|entry| entry.hex == color.to_hex_string());
        let highlight = state
            .draft(palette_highlight_draft(), "0")
            .parse::<usize>()
            .unwrap_or(0)
            .min(entries.len().saturating_sub(1));
        let hex_focused = state.draft(&hex_focus_draft(channel), "") == "1";
        return Some(Box::new(AppearanceMenuLayer {
            kind: MenuKind::Palette {
                path,
                channel,
                label: channel_label(channel).to_string(),
                value,
                entries,
                selected,
                highlight,
                hex_focused,
            },
            anchors: Rc::clone(&state.appearance_anchors),
            theme_open: Rc::clone(&state.theme_select_menu),
            palette_open: Rc::clone(&state.color_palette_menu),
            state: state.clone(),
            actions: actions.clone(),
            panel: None,
            panel_size: None,
            panel_position: None,
            panel_bounds: None,
            origin: None,
        }));
    }
    if theme_menu_open(state) {
        let options = fields::palette_options()
            .into_iter()
            .map(|(label, _)| label)
            .collect::<Vec<_>>();
        let selected = fields::palette_index(state);
        let highlight = state
            .draft(theme_highlight_draft(), &selected.to_string())
            .parse::<usize>()
            .unwrap_or(selected)
            .min(options.len().saturating_sub(1));
        let _ = app;
        return Some(Box::new(AppearanceMenuLayer {
            kind: MenuKind::Theme {
                options,
                selected,
                highlight,
            },
            anchors: Rc::clone(&state.appearance_anchors),
            theme_open: Rc::clone(&state.theme_select_menu),
            palette_open: Rc::clone(&state.color_palette_menu),
            state: state.clone(),
            actions: actions.clone(),
            panel: None,
            panel_size: None,
            panel_position: None,
            panel_bounds: None,
            origin: None,
        }));
    }
    None
}

fn channel_pair(channel: usize) -> (String, &'static str) {
    let key = channel_label(channel);
    (format!("theme.{key}"), key)
}

fn channel_label(channel: usize) -> &'static str {
    match channel {
        0 => "accent",
        1 => "primary",
        2 => "secondary",
        3 => "bg",
        4 => "surface",
        _ => "accent",
    }
}

// ---------------------------------------------------------------------------
// Panel contents
// ---------------------------------------------------------------------------

/// A theme option row: the label, the current check, and the keyboard/pointer
/// highlight. The panel's key handling lives in `AppearanceMenuLayer`; this row
/// only commits a pointer press.
struct MenuRow {
    label: String,
    selected: bool,
    highlighted: bool,
    disabled: bool,
    on_click: Option<Rc<RefCell<dyn FnMut()>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl MenuRow {
    fn new(
        label: impl Into<String>,
        selected: bool,
        highlighted: bool,
        disabled: bool,
        on_click: impl FnMut() + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            selected,
            highlighted,
            disabled,
            on_click: Some(Rc::new(RefCell::new(on_click))),
            size: None,
            origin: None,
        }
    }
}

impl Element for MenuRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if constraint.max.x.is_finite() {
            constraint.max.x
        } else {
            PANEL_WIDTH
        };
        let size = vec2f(width, MENU_ROW_HEIGHT);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(vec2f(PANEL_WIDTH, MENU_ROW_HEIGHT));
        let bounds = rectf(origin.x, origin.y, size.x, size.y);
        let hovered = !self.disabled && ctx.hovered(bounds);
        if self.selected || self.highlighted || hovered {
            let fill = if self.selected {
                app.theme.color(ColorToken::Selected)
            } else {
                app.theme.color(ColorToken::Hover)
            };
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rounded_rect(bounds, fill, 6.0);
            }
        }
        let text_color = if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else if self.selected {
            app.theme.color(ColorToken::Accent)
        } else {
            app.theme.color(ColorToken::Text)
        };
        let mut label = Text::new(self.label.clone())
            .with_font_size(13.0)
            .with_line_height(1.4)
            .with_color(text_color)
            .finish();
        let label_size = label.layout(
            SizeConstraint::loose(vec2f((size.x - 40.0).max(0.0), size.y)),
            &mut LayoutContext::default(),
            app,
        );
        label.paint(
            vec2f(
                origin.x + 10.0,
                origin.y + (size.y - label_size.y).max(0.0) / 2.0,
            ),
            ctx,
            app,
        );
        if self.selected {
            let mut check = Icon::new("check")
                .with_size(13.0)
                .with_color(app.theme.color(ColorToken::Accent))
                .finish();
            let _ = check.layout(
                SizeConstraint::tight(vec2f(13.0, 13.0)),
                &mut LayoutContext::default(),
                app,
            );
            check.paint(
                vec2f(
                    origin.x + size.x - 10.0 - 13.0,
                    origin.y + (size.y - 13.0) / 2.0,
                ),
                ctx,
                app,
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
        if self.disabled {
            return false;
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };
        if let DispatchedEvent::MouseDown { position, button } = event {
            if *button == 0
                && bounds.contains(goble_ui::geometry::PointF::new(position.x, position.y))
            {
                if let Some(callback) = self.on_click.as_ref() {
                    (callback.borrow_mut())();
                }
                return true;
            }
        }
        false
    }
}

fn theme_panel(
    app: &AppContext,
    options: &[String],
    selected: usize,
    highlight: usize,
    state: SettingsUiState,
    actions: SettingsActions,
) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);
    for (index, label) in options.iter().enumerate() {
        let theme_open = Rc::clone(&state.theme_select_menu);
        let close_dispatch = actions.dispatch.clone();
        let select_dispatch = actions.dispatch.clone();
        let row = MenuRow::new(
            label.clone(),
            index == selected,
            index == highlight,
            false,
            move || {
                *theme_open.borrow_mut() = false;
                (select_dispatch.borrow_mut())(SettingsAction::SetSelect {
                    path: "theme.preset".to_string(),
                    index,
                });
                (close_dispatch.borrow_mut())(SettingsAction::ToggleThemeSelect(false));
            },
        )
        .finish();
        column = column.with_child(row);
    }
    let body = column.finish();
    ConstrainedBox::new(
        Container::new(body)
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
            .with_corner_radius(12.0)
            .with_padding(goble_ui::elements::EdgeInsets::uniform(8.0))
            .finish(),
    )
    .with_width(PANEL_WIDTH)
    .finish()
}

#[allow(clippy::too_many_arguments)]
fn palette_panel(
    app: &AppContext,
    path: &str,
    channel: usize,
    label: &str,
    value: &str,
    entries: &[PaletteSwatch],
    selected: Option<usize>,
    highlight: usize,
    hex_focused: bool,
    state: SettingsUiState,
    actions: SettingsActions,
) -> Box<dyn Element> {
    let path = path.to_string();
    let close_dispatch = actions.dispatch.clone();
    let close_focus = actions.dispatch.clone();
    let palette_open = Rc::clone(&state.color_palette_menu);
    let close_channel = channel;
    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new(format!("Palette · {label}"))
                .with_font_size(12.0)
                .with_weight(FontWeight::SemiBold)
                .with_color(app.theme.color(ColorToken::Text))
                .finish(),
        )
        .with_child(
            IconButton::new(
                Icon::new("x")
                    .with_size(11.0)
                    .with_color(app.theme.color(ColorToken::Muted))
                    .finish(),
            )
            .with_size(vec2f(20.0, 20.0))
            .with_on_click(move || {
                *palette_open.borrow_mut() = None;
                (close_focus.borrow_mut())(SettingsAction::SetDraft {
                    id: hex_focus_draft(close_channel),
                    value: String::new(),
                });
                (close_dispatch.borrow_mut())(SettingsAction::SetDraft {
                    id: "appearance.palette.open".to_string(),
                    value: String::new(),
                });
            })
            .finish(),
        )
        .finish();

    let hex_cell: Rc<RefCell<String>> = Rc::new(RefCell::new(value.to_string()));
    let change_cell = Rc::clone(&hex_cell);
    let change_path = path.clone();
    let change_dispatch = actions.dispatch.clone();
    let submit_cell = Rc::clone(&hex_cell);
    let submit_path = path.clone();
    let submit_dispatch = actions.dispatch.clone();
    let submit_palette = Rc::clone(&state.color_palette_menu);
    let submit_actions = actions.clone();
    let focus_id = hex_focus_draft(channel);
    let focus_dispatch = actions.dispatch.clone();
    let field_value = state
        .color_preview
        .borrow()
        .and_then(|(target, color)| (target == channel).then(|| color.to_hex_string()))
        .unwrap_or_else(|| value.to_string());
    let field = SettingsTextField::new()
        .with_value(field_value)
        .with_placeholder(&channel_color(&state, channel_key(channel)).to_hex_string())
        .with_width(160.0)
        .with_focused(hex_focused)
        .with_on_change(move |value| {
            *change_cell.borrow_mut() = value.clone();
            (change_dispatch.borrow_mut())(SettingsAction::SetDraft {
                id: change_path.clone(),
                value,
            });
        })
        .with_on_submit(move || {
            if let Some(color) = ColorU::from_hex(&submit_cell.borrow()) {
                (submit_dispatch.borrow_mut())(SettingsAction::SetText {
                    path: submit_path.clone(),
                    value: color.to_hex_string(),
                });
                (submit_dispatch.borrow_mut())(SettingsAction::ClearField {
                    id: submit_path.clone(),
                });
                *submit_palette.borrow_mut() = None;
                set_draft(&submit_actions, &hex_focus_draft(channel), "");
            }
        })
        .with_on_focus_change(move |focused| {
            (focus_dispatch.borrow_mut())(SettingsAction::SetDraft {
                id: focus_id.clone(),
                value: if focused {
                    "1".to_string()
                } else {
                    String::new()
                },
            });
        })
        .finish();

    let reset_open = Rc::clone(&state.color_palette_menu);
    let reset_dispatch = actions.dispatch.clone();
    let reset_focus = actions.dispatch.clone();
    let reset_path = path.clone();
    let reset = SettingsButton::new("Reset")
        .with_icon("undo")
        .with_height(28.0)
        .with_on_click(move || {
            *reset_open.borrow_mut() = None;
            (reset_focus.borrow_mut())(SettingsAction::SetDraft {
                id: hex_focus_draft(channel),
                value: String::new(),
            });
            (reset_dispatch.borrow_mut())(SettingsAction::Reset {
                path: reset_path.clone(),
            });
            (reset_dispatch.borrow_mut())(SettingsAction::ClearField {
                id: reset_path.clone(),
            });
        })
        .finish();
    let hex_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Text::new("Hex")
                .with_font_size(11.0)
                .with_color(app.theme.color(ColorToken::Muted))
                .finish(),
        )
        .with_child(field)
        .with_child(reset)
        .finish();

    // 362: the DSH control is the saturation/value square and the full-width
    // hue strip, not a preset grid. The hex field still writes exact values.
    let picker_path = path.clone();
    let preview_color = state
        .color_preview
        .borrow()
        .and_then(|(target, color)| (target == channel).then_some(color))
        .unwrap_or_else(|| channel_color(&state, channel_key(channel)));
    let preview_dispatch = actions.dispatch.clone();
    let preview_channel = channel;
    let commit_dispatch = actions.dispatch.clone();
    let commit_clear = actions.dispatch.clone();
    let picker = crate::ui::color_picker::HsvBarPicker::new(preview_color)
        .with_drag(Rc::clone(&state.color_picker_drag))
        .with_on_preview(move |color| {
            (preview_dispatch.borrow_mut())(SettingsAction::PreviewColor {
                channel: preview_channel,
                color,
            });
        })
        .with_on_commit(move |color| {
            (commit_dispatch.borrow_mut())(SettingsAction::SetText {
                path: picker_path.clone(),
                value: color.to_hex_string(),
            });
            (commit_clear.borrow_mut())(SettingsAction::ClearColorPreview);
        })
        .finish();

    let hint = Text::new("Drag the square for saturation/value, the strip for hue · type #rrggbb for an exact value.")
        .with_font_size(11.0)
        .with_line_height(1.35)
        .with_color(app.theme.color(ColorToken::Muted))
        .finish();

    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(8.0)
        .with_child(header)
        .with_child(hex_row)
        .with_child(picker)
        .with_child(hint)
        .finish();
    ConstrainedBox::new(
        Container::new(body)
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
            .with_corner_radius(12.0)
            .with_padding(goble_ui::elements::EdgeInsets::uniform(PANEL_PADDING))
            .finish(),
    )
    .with_width(PANEL_WIDTH)
    .finish()
}

fn channel_key(channel: usize) -> &'static str {
    channel_label(channel)
}

/// A six-column grid of palette swatches. Pointer presses select a colour; the
/// menu's key handling moves the highlight and commits it.
struct ColorGrid {
    entries: Vec<PaletteSwatch>,
    selected: Option<usize>,
    highlight: usize,
    on_select: Option<Rc<RefCell<dyn FnMut(usize)>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ColorGrid {
    fn new(
        entries: Vec<PaletteSwatch>,
        selected: Option<usize>,
        highlight: usize,
        dispatch: Rc<RefCell<dyn FnMut(SettingsAction)>>,
        path: String,
        palette_open: Rc<RefCell<Option<usize>>>,
        channel: usize,
    ) -> Self {
        let selectable = entries.clone();
        let on_select: Rc<RefCell<dyn FnMut(usize)>> =
            Rc::new(RefCell::new(move |index: usize| {
                if let Some(entry) = selectable.get(index) {
                    (dispatch.borrow_mut())(SettingsAction::SetText {
                        path: path.clone(),
                        value: entry.hex.clone(),
                    });
                    (dispatch.borrow_mut())(SettingsAction::ClearField { id: path.clone() });
                }
                *palette_open.borrow_mut() = None;
                (dispatch.borrow_mut())(SettingsAction::SetDraft {
                    id: hex_focus_draft(channel),
                    value: String::new(),
                });
            }));
        Self {
            entries,
            selected,
            highlight,
            on_select: Some(on_select),
            size: None,
            origin: None,
        }
    }

    fn cell_geometry(&self, width: f32) -> (f32, f32) {
        let gap = 6.0;
        let cell_w = ((width - gap * (GRID_COLUMNS as f32 - 1.0)) / GRID_COLUMNS as f32).max(0.0);
        (cell_w, GRID_CELL_HEIGHT)
    }

    fn index_at(&self, position: Vector2F) -> Option<usize> {
        let origin = self.origin?;
        let size = self.size?;
        let (cell_w, cell_h) = self.cell_geometry(size.x);
        if position.x < origin.x() || position.y < origin.y() {
            return None;
        }
        let col = ((position.x - origin.x()) / (cell_w + 6.0)).floor() as usize;
        let row = ((position.y - origin.y()) / cell_h).floor() as usize;
        if col >= GRID_COLUMNS {
            return None;
        }
        let index = row * GRID_COLUMNS + col;
        (index < self.entries.len()).then_some(index)
    }
}

impl Element for ColorGrid {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if constraint.max.x.is_finite() {
            constraint.max.x
        } else {
            PANEL_WIDTH - PANEL_PADDING * 2.0
        };
        let rows = self.entries.len().div_ceil(GRID_COLUMNS).max(1);
        let size = vec2f(width, rows as f32 * GRID_CELL_HEIGHT);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(vec2f(PANEL_WIDTH, GRID_CELL_HEIGHT));
        let (cell_w, cell_h) = self.cell_geometry(size.x);
        for (index, entry) in self.entries.iter().enumerate() {
            let col = index % GRID_COLUMNS;
            let row = index / GRID_COLUMNS;
            let x = origin.x + col as f32 * (cell_w + 6.0) + (cell_w - GRID_SWATCH) / 2.0;
            let y = origin.y + row as f32 * cell_h + (cell_h - GRID_SWATCH) / 2.0;
            let rect = rectf(x, y, GRID_SWATCH, GRID_SWATCH);
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rounded_rect(rect, entry.color, 6.0);
            }
            let selected = self.selected == Some(index);
            let highlighted = self.highlight == index;
            let border = if selected {
                app.theme.color(ColorToken::Focus)
            } else if highlighted {
                app.theme.color(ColorToken::Text)
            } else {
                app.theme
                    .color(ColorToken::Border)
                    .mix(&app.theme.color(ColorToken::Text), 0.2)
            };
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.stroke_rect(
                    rect,
                    border,
                    if selected || highlighted { 2.0 } else { 1.0 },
                    6.0,
                );
            }
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
        if let DispatchedEvent::MouseDown { position, button } = event {
            if *button == 0 {
                if let Some(index) = self.index_at(*position) {
                    if let Some(callback) = self.on_select.as_ref() {
                        (callback.borrow_mut())(index);
                    }
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_ui::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::event::DispatchedEvent;

    fn record_actions() -> (SettingsActions, Rc<RefCell<Vec<SettingsAction>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&log);
        let actions = SettingsActions::new(move |action| {
            sink.borrow_mut().push(action);
        });
        (actions, log)
    }

    fn key(key: &str) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: key.to_string(),
            modifiers: Default::default(),
        }
    }

    fn paint_menu(menu: &mut Box<dyn Element>, app: &AppContext) {
        menu.layout(
            SizeConstraint::loose(vec2f(900.0, 600.0)),
            &mut LayoutContext::default(),
            app,
        );
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), app);
    }

    #[test]
    fn the_popover_flips_above_a_trigger_near_the_bottom() {
        let viewport = rectf(20.0, 40.0, 900.0, 400.0);
        let anchor = rectf(500.0, 400.0, PALETTE_TRIGGER_WIDTH, TRIGGER_HEIGHT);
        let panel = vec2f(PANEL_WIDTH, 160.0);
        let placed = place_popover(anchor, panel, viewport);
        assert!(
            placed.y + panel.y <= anchor.min_y() - PANEL_GAP + 0.01,
            "the panel sits above its trigger: {placed:?}"
        );
        assert!(placed.y >= viewport.min_y() + PANEL_EDGE_MARGIN);
    }

    #[test]
    fn the_popover_clamps_to_both_sides_of_the_viewport() {
        let viewport = rectf(20.0, 40.0, 400.0, 300.0);
        let panel = vec2f(PANEL_WIDTH, 160.0);
        // A wide trigger whose right edge is past the viewport: the panel is
        // pulled back inside instead of hanging off the edge and clipping.
        let right = place_popover(
            rectf(360.0, 100.0, PALETTE_TRIGGER_WIDTH, TRIGGER_HEIGHT),
            panel,
            viewport,
        );
        assert!(right.x >= viewport.min_x() + PANEL_EDGE_MARGIN);
        assert!(
            right.x + panel.x <= viewport.max_x() - PANEL_EDGE_MARGIN + 0.01,
            "the panel's right edge stays inside: {right:?}"
        );

        // A trigger above the visible viewport (the page is scrolled away)
        // still produces a panel inside the viewport.
        let top = place_popover(rectf(100.0, -80.0, 196.0, TRIGGER_HEIGHT), panel, viewport);
        assert!(top.y >= viewport.min_y() + PANEL_EDGE_MARGIN);
        assert!(top.y + panel.y <= viewport.max_y() - PANEL_EDGE_MARGIN + 0.01);
    }

    #[test]
    fn the_theme_menu_commits_the_highlighted_preset_with_setselect() {
        let state = SettingsUiState::default();
        *state.theme_select_menu.borrow_mut() = true;
        state.appearance_anchors.borrow_mut().theme = Some(rectf(20.0, 20.0, 240.0, 32.0));
        let (actions, log) = record_actions();
        let app = AppContext::default();
        let mut menu = appearance_menu_layer(&app, &state, &actions).expect("theme menu");
        paint_menu(&mut menu, &app);
        let mut ctx = EventContext::default();
        assert!(menu.dispatch_event(&key("ArrowDown"), &mut ctx, &app));
        assert!(menu.dispatch_event(&key("Enter"), &mut ctx, &app));
        assert!(log.borrow().iter().any(|action| matches!(
            action,
            SettingsAction::SetSelect { path, index }
                if path == "theme.preset" && *index == 1
        )));
        assert!(!*state.theme_select_menu.borrow());
    }

    #[test]
    fn the_palette_menu_commits_a_swatch_with_settext() {
        let mut state = SettingsUiState::default();
        *state.color_palette_menu.borrow_mut() = Some(0);
        state.set_draft(palette_highlight_draft(), "1".to_string());
        state
            .appearance_anchors
            .borrow_mut()
            .palette
            .insert(0, rectf(20.0, 20.0, PALETTE_TRIGGER_WIDTH, TRIGGER_HEIGHT));
        let (actions, log) = record_actions();
        let app = AppContext::default();
        let mut menu = appearance_menu_layer(&app, &state, &actions).expect("palette menu");
        paint_menu(&mut menu, &app);
        let mut ctx = EventContext::default();
        assert!(menu.dispatch_event(&key("Enter"), &mut ctx, &app));
        let entries = palette_swatches(&state);
        assert!(log.borrow().iter().any(|action| matches!(
            action,
            SettingsAction::SetText { path, value }
                if path == "theme.accent" && *value == entries[1].hex
        )));
        assert_eq!(*state.color_palette_menu.borrow(), None);
    }

    #[test]
    fn the_grid_folds_duplicate_palette_colours_and_keeps_the_presets() {
        let state = SettingsUiState::default();
        let entries = palette_swatches(&state);
        assert!(entries.len() >= ThemePreset::ALL.len());
        let mut hexes = entries
            .iter()
            .map(|entry| entry.hex.clone())
            .collect::<Vec<_>>();
        let before = hexes.len();
        hexes.sort();
        hexes.dedup();
        assert_eq!(hexes.len(), before, "every grid cell is a distinct colour");
    }
}

#[cfg(test)]
mod click_tests {
    use super::*;
    use goble_ui::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::event::DispatchedEvent;

    #[test]
    fn the_channel_swatch_opens_the_palette_and_records_its_anchor() {
        let state = SettingsUiState::default();
        let actions = SettingsActions::new(|_| {});
        let app = AppContext::default();
        let mut control = ColorPaletteControl::new(
            &state,
            &actions,
            "theme.accent",
            0,
            String::new(),
            channel_color(&state, "accent"),
            false,
            false,
        );
        control.layout(
            SizeConstraint::loose(vec2f(400.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        control.paint(vec2f(40.0, 10.0), &mut PaintContext::default(), &app);
        assert!(state.appearance_anchors.borrow().palette.contains_key(&0));
        let mut ctx = EventContext::default();
        // The swatch is at the hex field's leading edge now.
        let at = vec2f(40.0 + 10.0 + SWATCH_TRIGGER / 2.0, 10.0 + 16.0);
        assert!(control.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: at,
                button: 0
            },
            &mut ctx,
            &app
        ));
        assert_eq!(*state.color_palette_menu.borrow(), Some(0));
    }

    #[test]
    fn the_theme_trigger_opens_the_theme_menu() {
        let state = SettingsUiState::default();
        let actions = SettingsActions::new(|_| {});
        let app = AppContext::default();
        let mut control = ThemeSelectControl::new(
            &state,
            &actions,
            vec!["Default base".to_string(), "Goble Night".to_string()],
            0,
            None,
            false,
            false,
        );
        control.layout(
            SizeConstraint::loose(vec2f(400.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        control.paint(vec2f(40.0, 10.0), &mut PaintContext::default(), &app);
        assert!(state.appearance_anchors.borrow().theme.is_some());
        let mut ctx = EventContext::default();
        assert!(control.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(60.0, 24.0),
                button: 0
            },
            &mut ctx,
            &app
        ));
        assert!(*state.theme_select_menu.borrow());
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::ui::SettingsCategory;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    fn draw_texts(
        state: &SettingsUiState,
        actions: &SettingsActions,
        app: &AppContext,
    ) -> Vec<String> {
        let model = super::super::super::pages::page_model(SettingsCategory::Appearance, state);
        let mut page = super::super::render_page(app, &model, state, actions);
        render_element(&mut page, vec2f(1000.0, 800.0), app)
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_open_theme_menu_paints_its_presets_over_the_page() {
        let mut state = SettingsUiState::default();
        state.theme_select_open = true;
        state.appearance_anchors.borrow_mut().theme = Some(rectf(40.0, 40.0, 240.0, 32.0));
        let actions = SettingsActions::new(|_| {});
        let app = AppContext::default();
        let texts = draw_texts(&state, &actions, &app);
        assert!(texts.iter().any(|text| text == "Goble Night"), "{texts:?}");
        assert!(texts.iter().any(|text| text == "Tokyo Night"), "{texts:?}");
    }

    #[test]
    fn the_open_palette_paints_the_grid_and_hex_entry() {
        let state = SettingsUiState::default();
        *state.color_palette_menu.borrow_mut() = Some(0);
        state
            .appearance_anchors
            .borrow_mut()
            .palette
            .insert(0, rectf(40.0, 40.0, PALETTE_TRIGGER_WIDTH, TRIGGER_HEIGHT));
        let actions = SettingsActions::new(|_| {});
        let app = AppContext::default();
        let texts = draw_texts(&state, &actions, &app);
        assert!(
            texts.iter().any(|text| text == "Palette · accent"),
            "{texts:?}"
        );
        assert!(texts.iter().any(|text| text == "Hex"), "{texts:?}");
        assert!(texts.iter().any(|text| text == "Reset"), "{texts:?}");
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use crate::actions::settings_actions::make_settings_actions;
    use crate::media::MediaState;
    use crate::state::UiState;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use std::sync::Arc;

    fn fixture(
        config: &str,
    ) -> (
        tempfile::TempDir,
        Arc<DesktopState>,
        Rc<RefCell<UiState>>,
        SettingsActions,
    ) {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path();
        std::fs::create_dir_all(home.join(".goble")).expect("home");
        let path = home.join("config.toml");
        std::fs::write(&path, config).expect("seed config");
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("store"),
            ThreadStore::new(home).expect("thread store"),
        );
        desktop.set_config_path(path);
        let state = Rc::new(RefCell::new(UiState::mock()));
        state.borrow_mut().refresh_settings(&desktop);
        let actions = make_settings_actions(
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
            Rc::new(RefCell::new(MediaState::mock())),
        );
        (dir, desktop, state, actions)
    }

    #[test]
    fn palette_writes_real_theme_keys_and_the_next_write_keeps_the_revision() {
        let (_dir, desktop, state, actions) =
            fixture("[theme]\npreset = \"goble-night\"\naccent = \"#ff8800\"\n");
        actions.dispatch(SettingsAction::SetText {
            path: "theme.accent".to_string(),
            value: "#12ab34".to_string(),
        });
        let text = std::fs::read_to_string(
            desktop
                .settings_path(goble_core::config::ConfigFile::Config)
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains("accent = \"#12ab34\""), "{text}");
        assert!(text.contains("preset = \"goble-night\""), "{text}");

        // A second write without a reload proves the page adopted the revision
        // the first write returned (a stale fence would surface a conflict).
        actions.dispatch(SettingsAction::SetText {
            path: "theme.primary".to_string(),
            value: "#abcdef".to_string(),
        });
        let text = std::fs::read_to_string(
            desktop
                .settings_path(goble_core::config::ConfigFile::Config)
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains("accent = \"#12ab34\""), "{text}");
        assert!(text.contains("primary = \"#abcdef\""), "{text}");

        // Selecting a preset writes `theme.preset` and drops the overrides.
        actions.dispatch(SettingsAction::SetSelect {
            path: "theme.preset".to_string(),
            index: 3,
        });
        let text = std::fs::read_to_string(
            desktop
                .settings_path(goble_core::config::ConfigFile::Config)
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains("preset = \"tokyonight\""), "{text}");
        assert!(!text.contains("accent ="), "{text}");
        assert!(!text.contains("primary ="), "{text}");
        let _ = state;
    }
}
