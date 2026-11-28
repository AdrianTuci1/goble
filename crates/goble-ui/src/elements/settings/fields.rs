//! The row controls: text field, path field, select, segmented control, and the
//! small button.
//!
//! These are the settings-sized versions of the app's own controls — 32px tall,
//! 8px radius, a hairline `--input` border and a 13px value — so a page's rows
//! line up without each page re-inventing the geometry. Everything is drawn
//! from tokens; nothing here hardcodes a colour.
//!
//! The select is a real dropdown: its 32px trigger draws the current value and a
//! chevron, and a press opens an anchored panel below it. The panel lists every
//! option, marks the current one with a check, and follows the keyboard
//! (Up/Down highlight, Enter takes the row, Escape closes). A press outside the
//! panel dismisses it. The element's open flag is shared with the page so a
//! surface rebuilt every frame can keep the panel up ([`SettingsSelect::with_open`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, EventContext, Icon, LayoutContext, MenuId, PaintContext, Point, PopupMenu,
    PopupMenuItem, PopupMenuPosition, SizeConstraint, Text, TextInput, DEFAULT_MENU_ID,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::advance_width;
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily};

use super::{
    control_border, control_fill, mix, SETTINGS_CONTROL_HEIGHT, SETTINGS_CONTROL_MIN_WIDTH,
    SETTINGS_CONTROL_RADIUS, SETTINGS_CONTROL_TEXT_SIZE,
};

type TextCallback = Rc<RefCell<dyn FnMut(String)>>;
type SubmitCallback = Rc<RefCell<dyn FnMut()>>;
type CycleCallback = Rc<RefCell<dyn FnMut(usize)>>;
type ToggleCallback = Rc<RefCell<dyn FnMut(bool)>>;
type ClickCallback = Rc<RefCell<dyn FnMut()>>;
type FocusCallback = Rc<RefCell<dyn FnMut(bool)>>;

/// The settings select's trigger height: T3's compact `h-8` (32).
const SELECT_TRIGGER_HEIGHT: f32 = SETTINGS_CONTROL_HEIGHT;
/// The trigger's horizontal inset around its value and chevron.
const SELECT_INSET: f32 = 10.0;
/// The chevron's box at the trigger's trailing edge.
const SELECT_CHEVRON: f32 = 12.0;
/// The size of a leading colour chip on a select trigger.
const SELECT_SWATCH: f32 = 14.0;

/// A settings text field: a 32px control with the value at 13px.
pub struct SettingsTextField {
    value: String,
    placeholder: String,
    focused: bool,
    disabled: bool,
    write_only: bool,
    width: Option<f32>,
    /// A leading control drawn inside the field's casing (the colour swatch),
    /// with the text starting after it.
    leading: Option<Box<dyn Element>>,
    leading_width: f32,
    leading_size: Option<Vector2F>,
    on_change: Option<TextCallback>,
    on_submit: Option<SubmitCallback>,
    on_focus_change: Option<FocusCallback>,
    field: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsTextField {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            placeholder: String::new(),
            focused: false,
            disabled: false,
            write_only: false,
            width: None,
            leading: None,
            leading_width: 0.0,
            leading_size: None,
            on_change: None,
            on_submit: None,
            on_focus_change: None,
            field: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Draw the value write-only: one dot per character, never the text. A
    /// secret the core never hands out is still typed here and committed as an
    /// op; the mask is what keeps it off the screen.
    pub fn write_only(mut self) -> Self {
        self.write_only = true;
        self
    }

    /// The field's fixed width; `None` takes the width the row gives it up to
    /// the control minimum.
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width.max(SETTINGS_CONTROL_MIN_WIDTH));
        self
    }

    /// Put a leading control inside the field's casing: the colour swatch sits
    /// at the input's leading edge and the value starts after it.
    pub fn with_leading(mut self, leading: Box<dyn Element>, width: f32) -> Self {
        self.leading = Some(leading);
        self.leading_width = width.max(0.0);
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_submit<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_submit = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build(&mut self) {
        let on_change = self.on_change.clone();
        let mut field = TextInput::new()
            .with_value(self.value.clone())
            .with_placeholder(self.placeholder.clone())
            .with_plain(true)
            .with_full_width(true)
            .with_masked(self.write_only)
            .with_focused(self.focused && !self.disabled);
        if !self.disabled {
            if let Some(callback) = on_change {
                field = field.with_on_change(move |value| {
                    (callback.borrow_mut())(value);
                });
            }
            if let Some(callback) = self.on_submit.clone() {
                field = field.with_on_submit(move || {
                    (callback.borrow_mut())();
                });
            }
            if let Some(callback) = self.on_focus_change.clone() {
                field = field.with_on_focus_change(move |focused| {
                    (callback.borrow_mut())(focused);
                });
            }
        }
        self.field = Some(field.finish());
    }

    fn field_mut(&mut self) -> &mut Box<dyn Element> {
        if self.field.is_none() {
            self.build();
        }
        self.field.as_mut().unwrap()
    }

    /// The field's child inset: the value sits 10px inside the control.
    const INSET: f32 = 10.0;
}

impl Default for SettingsTextField {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for SettingsTextField {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let width = self
            .width
            .unwrap_or_else(|| constraint.max.x.max(SETTINGS_CONTROL_MIN_WIDTH));
        let available = (width - Self::INSET * 2.0).max(0.0);
        let leading_width = if self.leading.is_some() {
            self.leading_width.min(available * 0.5)
        } else {
            0.0
        };
        let field_width = (available - leading_width).max(0.0);
        if let Some(leading) = self.leading.as_mut() {
            let size = leading.layout(
                SizeConstraint::loose(vec2f(leading_width, SETTINGS_CONTROL_HEIGHT)),
                ctx,
                app,
            );
            self.leading_size = Some(size);
        } else {
            self.leading_size = None;
        }
        let inner = SizeConstraint::new(
            vec2f(field_width, 0.0),
            vec2f(field_width, SETTINGS_CONTROL_HEIGHT),
        );
        let child = self.field_mut().layout(inner, ctx, app);
        let size = vec2f(width, SETTINGS_CONTROL_HEIGHT.max(child.y));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let border = if self.focused && !self.disabled {
            app.theme.color(ColorToken::Focus)
        } else if self.disabled {
            control_border(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            control_border(app)
        };
        let fill = if self.disabled {
            control_fill(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            control_fill(app)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            let rect = rectf(origin.x, origin.y, size.x, size.y);
            renderer.fill_rounded_rect(rect, fill, SETTINGS_CONTROL_RADIUS);
            renderer.stroke_rect(rect, border, 1.0, SETTINGS_CONTROL_RADIUS);
        }
        // The field's text is vertically centred in the 32px control; a
        // leading control (the colour swatch) sits inside the casing at that
        // same inset, and the text starts after it.
        let child_height = self.field_mut().size().map(|size| size.y).unwrap_or(0.0);
        let y = origin.y + ((size.y - child_height).max(0.0)) / 2.0;
        let leading_width = if let Some(leading) = self.leading.as_mut() {
            let leading_height = self.leading_size.map(|size| size.y).unwrap_or(0.0);
            let leading_y = origin.y + ((size.y - leading_height).max(0.0)) / 2.0;
            leading.paint(vec2f(origin.x + Self::INSET, leading_y), ctx, app);
            self.leading_width
        } else {
            0.0
        };
        self.field_mut()
            .paint(vec2f(origin.x + Self::INSET + leading_width, y), ctx, app);
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
        self.field_mut().dispatch_event(event, ctx, app)
    }
}

/// A path field is a text field with the same geometry; the difference is the
/// page's copy ("Folder") and, later, a browse affordance this build does not
/// have. Keeping the type means a page says what the value is.
pub type SettingsPathField = SettingsTextField;

/// The select's trigger: the current value, an optional leading colour chip and
/// the chevron. It paints only; [`SettingsSelect`]'s popup owns the press.
struct SelectTrigger {
    options: Vec<String>,
    selected: Rc<Cell<usize>>,
    open: Rc<RefCell<bool>>,
    width: f32,
    focused: bool,
    disabled: bool,
    swatch: Option<ColorU>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SelectTrigger {
    fn label(&self) -> &str {
        self.options
            .get(self.selected.get())
            .map(String::as_str)
            .unwrap_or("")
    }
}

impl Element for SelectTrigger {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.width, SELECT_TRIGGER_HEIGHT);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        let hovered = !self.disabled && ctx.hovered(rect);
        let border = if self.focused && !self.disabled {
            app.theme.color(ColorToken::Focus)
        } else if self.disabled {
            control_border(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            control_border(app)
        };
        let fill = if self.disabled {
            control_fill(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else if hovered {
            mix(app, ColorToken::SurfaceRaised, ColorToken::Hover, 0.7)
        } else {
            control_fill(app)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(rect, fill, SETTINGS_CONTROL_RADIUS);
            renderer.stroke_rect(rect, border, 1.0, SETTINGS_CONTROL_RADIUS);
        }
        let text_color = if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else {
            app.theme.color(ColorToken::Text)
        };
        let mut left = origin.x + SELECT_INSET;
        if let Some(swatch) = self.swatch {
            let chip = rectf(
                left,
                origin.y + (size.y - SELECT_SWATCH) / 2.0,
                SELECT_SWATCH,
                SELECT_SWATCH,
            );
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rounded_rect(chip, swatch, 4.0);
            }
            left += SELECT_SWATCH + 8.0;
        }
        let text_width = (origin.x + size.x - SELECT_INSET - SELECT_CHEVRON - 8.0 - left).max(0.0);
        let label = Text::new(self.label().to_string())
            .with_font_size(SETTINGS_CONTROL_TEXT_SIZE)
            .with_line_height(1.4)
            .with_max_lines(1)
            .with_color(text_color)
            .finish();
        let mut label = label;
        let label_size = label.layout(
            SizeConstraint::loose(vec2f(text_width, SELECT_TRIGGER_HEIGHT)),
            &mut LayoutContext::default(),
            app,
        );
        let y = origin.y + (size.y - label_size.y).max(0.0) / 2.0;
        label.paint(vec2f(left, y), ctx, app);
        let mut chevron = Icon::new(if *self.open.borrow() {
            "chevron-up"
        } else {
            "chevron-down"
        })
        .with_size(SELECT_CHEVRON)
        .with_theme_color(ColorToken::Muted, app)
        .finish();
        let _ = chevron.layout(
            SizeConstraint::tight(vec2f(SELECT_CHEVRON, SELECT_CHEVRON)),
            &mut LayoutContext::default(),
            app,
        );
        chevron.paint(
            vec2f(
                origin.x + size.x - SELECT_INSET - SELECT_CHEVRON,
                origin.y + (size.y - SELECT_CHEVRON) / 2.0,
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
}

/// A settings select: a 32px trigger with the current option and a chevron that
/// opens an anchored menu of every option.
pub struct SettingsSelect {
    options: Vec<String>,
    selected: Rc<Cell<usize>>,
    focused: bool,
    disabled: bool,
    width: f32,
    swatch: Option<ColorU>,
    on_change: Option<CycleCallback>,
    on_open_change: Option<ToggleCallback>,
    /// The open flag, shared with the page so a per-frame rebuild keeps the
    /// menu up. A page that never passes one still gets a working menu within
    /// the frame it opened in (and in direct element use).
    open: Rc<RefCell<bool>>,
    /// The menu's keyboard highlight and pointer row, shared with the popup so
    /// they survive the per-frame rebuild.
    highlight: Rc<RefCell<Option<usize>>>,
    hover: Rc<RefCell<Option<usize>>>,
    menu_id: MenuId,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// 315: a settings menu painted on the frame's top layer, so an open select
/// covers later rows instead of being covered by them.
struct DeferredMenu {
    inner: Rc<RefCell<Box<dyn Element>>>,
    defer: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl DeferredMenu {
    fn new(inner: Box<dyn Element>, defer: bool) -> Self {
        Self {
            inner: Rc::new(RefCell::new(inner)),
            defer,
            size: None,
            origin: None,
        }
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for DeferredMenu {
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
        // The trigger always paints in its row. An open panel also queues for
        // the frame's last-painted layer, so no later settings row can cover
        // it; the inline copy is under that top copy.
        self.inner.borrow_mut().paint(origin, ctx, app);
        if self.defer {
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.inner.borrow_mut().dispatch_event(event, ctx, app)
    }
}

impl SettingsSelect {
    pub fn new(options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            selected: Rc::new(Cell::new(0)),
            focused: false,
            disabled: false,
            width: SETTINGS_CONTROL_MIN_WIDTH,
            swatch: None,
            on_change: None,
            on_open_change: None,
            open: Rc::new(RefCell::new(false)),
            highlight: Rc::new(RefCell::new(None)),
            hover: Rc::new(RefCell::new(None)),
            menu_id: DEFAULT_MENU_ID,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected(self, index: usize) -> Self {
        self.selected.set(index);
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width.max(SETTINGS_CONTROL_MIN_WIDTH);
        self
    }

    /// A leading 14px colour chip, for a select whose value is a theme colour or
    /// a palette.
    pub fn with_swatch(mut self, color: impl Into<ColorU>) -> Self {
        self.swatch = Some(color.into());
        self
    }

    /// Share the menu's open flag with the page. The settings tree is rebuilt
    /// every frame, so an element-local flag would close the panel on the next
    /// frame; a page that owns the flag keeps the menu up.
    pub fn with_open(mut self, open: Rc<RefCell<bool>>) -> Self {
        self.open = open;
        self
    }

    /// Name the menu in the window's shared open-menu state, so it takes part in
    /// the one-menu-at-a-time rule and in the root view's outside dismiss.
    pub fn with_menu_id(mut self, menu_id: MenuId) -> Self {
        self.menu_id = menu_id;
        self
    }

    pub fn with_on_change<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Called when the menu opens or closes, so a page that keeps the flag in
    /// its own state can mirror the change (and feed it back next frame through
    /// [`Self::with_open`]).
    pub fn with_on_open_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_open_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn selected(&self) -> usize {
        self.selected.get()
    }

    pub fn is_open(&self) -> bool {
        *self.open.borrow()
    }

    fn build(&mut self) {
        let trigger = SelectTrigger {
            options: self.options.clone(),
            selected: self.selected.clone(),
            open: self.open.clone(),
            width: self.width,
            focused: self.focused,
            disabled: self.disabled,
            swatch: self.swatch,
            size: None,
            origin: None,
        }
        .finish();
        let items = self
            .options
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let item = PopupMenuItem::new(label.clone());
                if index == self.selected.get() {
                    item.selected()
                } else {
                    item
                }
            })
            .collect();
        let selection = self.selected.clone();
        let on_change = self.on_change.clone();
        let menu = PopupMenu::new(trigger, items)
            .with_open(self.open.clone())
            .with_highlight_index(self.highlight.clone())
            .with_hover_index(self.hover.clone())
            .with_menu_id(self.menu_id)
            .with_position(PopupMenuPosition::Below)
            .with_max_width(self.width.max(200.0))
            .with_row_height(SELECT_TRIGGER_HEIGHT)
            .with_on_select(move |index| {
                if selection.get() == index {
                    return;
                }
                selection.set(index);
                if let Some(callback) = on_change.as_ref() {
                    (callback.borrow_mut())(index);
                }
            });
        // 315: the open panel paints in the window's top layer, above any
        // later settings row, instead of inside the form's paint order.
        self.root = Some(DeferredMenu::new(menu.finish(), *self.open.borrow()).finish());
    }

    fn root_mut(&mut self) -> &mut Box<dyn Element> {
        if self.root.is_none() {
            self.build();
        }
        self.root.as_mut().unwrap()
    }
}

impl Element for SettingsSelect {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // Rebuild from the current selection and open flag on every layout: the
        // popup owns the panel and the rows, and the selected check must follow
        // a value the page changed since the last frame. The highlight and hover
        // rows are shared, so rebuilding does not lose the keyboard position.
        self.build();
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root_mut().paint(origin, ctx, app);
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
        if self.disabled || self.options.is_empty() {
            return false;
        }
        let was_open = *self.open.borrow();
        // A closed select opens from the keyboard too: Down/Up, Enter and Space
        // all put the menu up, the way a native select does. The app-owned flag
        // (when supplied) keeps it up across the next frame.
        if !was_open {
            if let DispatchedEvent::KeyDown { key, modifiers } = event {
                if !modifiers.command && !modifiers.ctrl && !modifiers.alt {
                    if matches!(
                        key.as_str(),
                        "ArrowDown" | "ArrowUp" | "Enter" | " " | "Space"
                    ) {
                        *self.open.borrow_mut() = true;
                        if let Some(callback) = self.on_open_change.as_ref() {
                            (callback.borrow_mut())(true);
                        }
                        return true;
                    }
                }
            }
        }
        let consumed = self.root_mut().dispatch_event(event, ctx, app);
        let is_open = *self.open.borrow();
        if was_open != is_open {
            if let Some(callback) = self.on_open_change.as_ref() {
                (callback.borrow_mut())(is_open);
            }
        }
        consumed
    }
}

// ---------------------------------------------------------------------------
// Segmented control
// ---------------------------------------------------------------------------

/// A segment's text size.
const SEGMENT_TEXT_SIZE: f32 = SETTINGS_CONTROL_TEXT_SIZE;
/// The control's inner padding and the 2px gap between segments.
const SEGMENT_PADDING: f32 = 2.0;
const SEGMENT_GAP: f32 = 2.0;
/// The segment height inside the 32px control.
const SEGMENT_HEIGHT: f32 = SETTINGS_CONTROL_HEIGHT - SEGMENT_PADDING * 2.0;
/// The selected pill's radius (T3's `rounded-md`/`rounded-lg` register).
const SEGMENT_RADIUS: f32 = 8.0;
/// The control's own radius: the selected pill plus the 2px gutter.
const SEGMENT_CONTROL_RADIUS: f32 = 10.0;
/// A segment's minimum width, and the horizontal room its label gets.
const SEGMENT_MIN_WIDTH: f32 = 56.0;
const SEGMENT_LABEL_PADDING: f32 = 14.0;

/// A two-or-more-way switch: Queue / Send Immediately, On / Off, Compact /
/// Comfortable. Equal-width segments sit in a 32px capsule; the selected one is
/// an 8px pill in the raised surface with primary text, hover lifts the others,
/// and Left/Right (and Up/Down) walk the options while Enter/Space commits the
/// current one.
pub struct SettingsSegmented {
    options: Vec<String>,
    selected: usize,
    focused: bool,
    disabled: bool,
    width: Option<f32>,
    on_change: Option<CycleCallback>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsSegmented {
    pub fn new(options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            selected: 0,
            focused: false,
            disabled: false,
            width: None,
            on_change: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// A fixed control width; the segments share it equally. Without one the
    /// control is as wide as the longest label needs.
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width.max(SEGMENT_MIN_WIDTH * 2.0));
        self
    }

    pub fn with_on_change<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    fn segment_width(&self) -> f32 {
        let count = self.options.len().max(1) as f32;
        let longest = self
            .options
            .iter()
            .map(|label| {
                advance_width(
                    label,
                    SEGMENT_TEXT_SIZE,
                    FontWeight::Medium,
                    FontFamily::System,
                    false,
                )
            })
            .fold(0.0, f32::max);
        let natural = (longest + SEGMENT_LABEL_PADDING * 2.0).max(SEGMENT_MIN_WIDTH);
        match self.width {
            Some(width) => {
                ((width - SEGMENT_PADDING * 2.0 - SEGMENT_GAP * (count - 1.0)) / count).max(0.0)
            }
            None => natural,
        }
    }

    fn total_width(&self) -> f32 {
        let count = self.options.len().max(1) as f32;
        self.segment_width() * count + SEGMENT_GAP * (count - 1.0) + SEGMENT_PADDING * 2.0
    }

    fn segment_at(&self, x: f32) -> Option<usize> {
        if self.options.is_empty() {
            return None;
        }
        let width = self.segment_width();
        let local = (x - SEGMENT_PADDING).max(0.0);
        let index = (local / (width + SEGMENT_GAP)) as usize;
        Some(index.min(self.options.len() - 1))
    }

    fn select(&mut self, index: usize) -> bool {
        if self.disabled || self.options.is_empty() || index == self.selected {
            return false;
        }
        self.selected = index.min(self.options.len() - 1);
        if let Some(callback) = self.on_change.as_ref() {
            (callback.borrow_mut())(self.selected);
        }
        true
    }
}

impl Element for SettingsSegmented {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.total_width(), SETTINGS_CONTROL_HEIGHT);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let segment_width = self.segment_width();
        let disabled = self.disabled;
        let selected_index = self.selected;
        let focused = self.focused;
        let hovered: Vec<bool> = (0..self.options.len())
            .map(|index| {
                if disabled || index == selected_index {
                    return false;
                }
                let x = origin.x + SEGMENT_PADDING + index as f32 * (segment_width + SEGMENT_GAP);
                ctx.hovered(rectf(
                    x,
                    origin.y + SEGMENT_PADDING,
                    segment_width,
                    SEGMENT_HEIGHT,
                ))
            })
            .collect();
        {
            let Some(renderer) = ctx.renderer.as_mut() else {
                return;
            };
            let control = rectf(origin.x, origin.y, size.x, size.y);
            let fill = if disabled {
                control_fill(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
            } else {
                control_fill(app)
            };
            renderer.fill_rounded_rect(control, fill, SEGMENT_CONTROL_RADIUS);
            renderer.stroke_rect(
                control,
                if disabled {
                    control_border(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
                } else {
                    control_border(app)
                },
                1.0,
                SEGMENT_CONTROL_RADIUS,
            );
            for index in 0..self.options.len() {
                let x = origin.x + SEGMENT_PADDING + index as f32 * (segment_width + SEGMENT_GAP);
                let segment = rectf(x, origin.y + SEGMENT_PADDING, segment_width, SEGMENT_HEIGHT);
                let selected = index == selected_index;
                let hovered = hovered[index];
                let segment_fill = if selected {
                    if disabled {
                        Some(
                            app.theme
                                .color(ColorToken::SurfaceRaised)
                                .mix(&app.theme.color(ColorToken::Bg), 0.5),
                        )
                    } else {
                        Some(app.theme.color(ColorToken::SurfaceRaised))
                    }
                } else if hovered {
                    Some(app.theme.color(ColorToken::Hover))
                } else {
                    None
                };
                if let Some(color) = segment_fill {
                    renderer.fill_rounded_rect(segment, color, SEGMENT_RADIUS);
                }
                if selected {
                    renderer.stroke_rect(
                        segment,
                        if disabled {
                            control_border(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
                        } else {
                            control_border(app)
                        },
                        1.0,
                        SEGMENT_RADIUS,
                    );
                }
            }
            if focused {
                let ring = rectf(origin.x - 2.0, origin.y - 2.0, size.x + 4.0, size.y + 4.0);
                renderer.stroke_rect(
                    ring,
                    app.theme.color(ColorToken::Focus),
                    2.0,
                    SEGMENT_CONTROL_RADIUS + 2.0,
                );
            }
        }
        for (index, label) in self.options.iter().enumerate() {
            let x = origin.x + SEGMENT_PADDING + index as f32 * (segment_width + SEGMENT_GAP);
            let segment = rectf(x, origin.y + SEGMENT_PADDING, segment_width, SEGMENT_HEIGHT);
            let selected = index == selected_index;
            let hovered = hovered[index];
            let color = if disabled {
                app.theme.color(ColorToken::Muted)
            } else if selected || hovered {
                app.theme.color(ColorToken::Text)
            } else {
                app.theme.color(ColorToken::Muted)
            };
            let mut text = Text::new(label.clone())
                .with_font_size(SEGMENT_TEXT_SIZE)
                .with_line_height(1.4)
                .with_weight(FontWeight::Medium)
                .with_max_lines(1)
                .with_color(color)
                .finish();
            let max_width = (segment_width - 8.0).max(0.0);
            let text_size = text.layout(
                SizeConstraint::loose(vec2f(max_width, SEGMENT_HEIGHT)),
                &mut LayoutContext::default(),
                app,
            );
            text.paint(
                vec2f(
                    segment.min_x() + (segment_width - text_size.x).max(0.0) / 2.0,
                    segment.min_y() + (SEGMENT_HEIGHT - text_size.y).max(0.0) / 2.0,
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
        if self.disabled || self.options.is_empty() {
            return false;
        }
        match event {
            DispatchedEvent::MouseDown {
                position,
                button: 0,
            } => {
                let Some(bounds) = self.bounds() else {
                    return false;
                };
                if !bounds.contains(crate::geometry::PointF::new(position.x, position.y)) {
                    return false;
                }
                if let Some(index) = self.segment_at(position.x - bounds.min_x()) {
                    let _ = self.select(index);
                }
                true
            }
            DispatchedEvent::KeyDown { key, modifiers } => {
                if modifiers.command || modifiers.ctrl || modifiers.alt {
                    return false;
                }
                let count = self.options.len();
                match key.as_str() {
                    "ArrowRight" | "ArrowDown" => {
                        self.select((self.selected + 1) % count);
                        true
                    }
                    "ArrowLeft" | "ArrowUp" => {
                        self.select((self.selected + count - 1) % count);
                        true
                    }
                    "Enter" | " " | "Space" => {
                        let index = self.selected;
                        if let Some(callback) = self.on_change.as_ref() {
                            (callback.borrow_mut())(index);
                        }
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

/// The default settings button height (T3's `size="xs"`).
pub const SETTINGS_BUTTON_HEIGHT: f32 = 28.0;
/// The large settings button height (T3's `size="sm"`).
pub const SETTINGS_BUTTON_HEIGHT_LARGE: f32 = 32.0;
/// The button's label size.
const BUTTON_TEXT_SIZE: f32 = 13.0;
/// The button's corner radius.
const BUTTON_RADIUS: f32 = 8.0;
/// The button's horizontal inset around its icon + label.
const BUTTON_INSET: f32 = 10.0;
/// The leading icon's box, and the gap after it.
const BUTTON_ICON: f32 = 12.0;
const BUTTON_ICON_GAP: f32 = 5.0;

/// A small settings button: 28px (or 32px) tall, an 8px radius, a 13px label
/// and an optional leading icon. The variants are the page's registers:
/// primary (accent fill), secondary (raised surface + hairline, the default),
/// destructive (error ink and tint), ghost (text only) and icon-only.
pub struct SettingsButton {
    label: String,
    icon: Option<&'static str>,
    primary: bool,
    destructive: bool,
    ghost: bool,
    icon_only: bool,
    height: f32,
    disabled: bool,
    focused: bool,
    on_click: Option<ClickCallback>,
    state: crate::elements::interactive::InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsButton {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            primary: false,
            destructive: false,
            ghost: false,
            icon_only: false,
            height: SETTINGS_BUTTON_HEIGHT,
            disabled: false,
            focused: false,
            on_click: None,
            state: crate::elements::interactive::InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    /// An icon-only button, 28×28 by default.
    pub fn icon(icon: &'static str) -> Self {
        Self::new("").with_icon(icon).icon_only()
    }

    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Explicitly the default (secondary) register; the default already is.
    pub fn secondary(self) -> Self {
        self
    }

    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// A low-emphasis text button: no raised fill or outline, muted text. Used
    /// for a list row's secondary action (a palette's "Use" / "Selected").
    pub fn ghost(mut self) -> Self {
        self.ghost = true;
        self
    }

    /// Draw just the icon, as a square button.
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    /// 28px by default; the page may ask for T3's 32px `sm` register.
    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height.clamp(SETTINGS_BUTTON_HEIGHT, SETTINGS_BUTTON_HEIGHT_LARGE);
        self
    }

    /// The 32px register.
    pub fn large(self) -> Self {
        self.with_height(SETTINGS_BUTTON_HEIGHT_LARGE)
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn is_icon_only(&self) -> bool {
        self.icon_only || self.label.is_empty()
    }

    fn text_width(&self) -> f32 {
        if self.is_icon_only() {
            0.0
        } else {
            advance_width(
                &self.label,
                BUTTON_TEXT_SIZE,
                FontWeight::Medium,
                FontFamily::System,
                false,
            )
        }
    }

    fn icon_color(&self, app: &AppContext) -> ColorU {
        if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else if self.primary {
            app.theme.color(ColorToken::Bg)
        } else if self.destructive {
            app.theme.color(ColorToken::Error)
        } else if self.ghost {
            app.theme.color(ColorToken::Muted)
        } else {
            app.theme.color(ColorToken::Text)
        }
    }
}

impl Element for SettingsButton {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if self.is_icon_only() {
            self.height
        } else {
            self.text_width()
                + BUTTON_INSET * 2.0
                + if self.icon.is_some() {
                    BUTTON_ICON + BUTTON_ICON_GAP
                } else {
                    0.0
                }
        };
        let size = vec2f(width.max(self.height), self.height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or_else(|| vec2f(self.height, self.height));
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        let hovered = !self.disabled && (self.state.hover || ctx.hovered(rect));
        let pressed = !self.disabled && self.state.is_active();
        let bg = app.theme.color(ColorToken::Bg);
        let (fill, border) = if self.disabled {
            (
                Some(control_fill(app).mix(&bg, 0.5)),
                Some(control_border(app).mix(&bg, 0.5)),
            )
        } else if self.primary {
            let fill = if pressed {
                mix(app, ColorToken::Accent, ColorToken::Bg, 0.18)
            } else if hovered {
                mix(app, ColorToken::Accent, ColorToken::Text, 0.08)
            } else {
                app.theme.color(ColorToken::Accent)
            };
            (Some(fill), None)
        } else if self.destructive {
            let t = if pressed {
                0.32
            } else if hovered {
                0.22
            } else {
                0.14
            };
            (
                Some(bg.mix(&app.theme.color(ColorToken::Error), t)),
                Some(mix(app, ColorToken::Error, ColorToken::Border, 0.45)),
            )
        } else if self.ghost {
            (
                if hovered {
                    Some(app.theme.color(ColorToken::Hover))
                } else {
                    None
                },
                None,
            )
        } else {
            let fill = if pressed {
                mix(app, ColorToken::SurfaceRaised, ColorToken::Bg, 0.25)
            } else if hovered {
                app.theme.color(ColorToken::Hover)
            } else {
                app.theme.color(ColorToken::SurfaceRaised)
            };
            (Some(fill), Some(control_border(app)))
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            if let Some(fill) = fill {
                renderer.fill_rounded_rect(rect, fill, BUTTON_RADIUS);
            }
            if let Some(border) = border {
                renderer.stroke_rect(rect, border, 1.0, BUTTON_RADIUS);
            }
        }
        let ink = if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else if self.primary {
            app.theme.color(ColorToken::Bg)
        } else if self.destructive {
            app.theme.color(ColorToken::Error)
        } else if self.ghost {
            if hovered {
                app.theme.color(ColorToken::Text)
            } else {
                app.theme.color(ColorToken::Muted)
            }
        } else {
            app.theme.color(ColorToken::Text)
        };
        let content_width = self.text_width()
            + if self.icon.is_some() {
                BUTTON_ICON + BUTTON_ICON_GAP
            } else {
                0.0
            };
        let mut x = origin.x + (size.x - content_width).max(0.0) / 2.0;
        if let Some(name) = self.icon {
            let mut icon = Icon::new(name)
                .with_size(BUTTON_ICON)
                .with_color(self.icon_color(app))
                .finish();
            let _ = icon.layout(
                SizeConstraint::tight(vec2f(BUTTON_ICON, BUTTON_ICON)),
                &mut LayoutContext::default(),
                app,
            );
            icon.paint(vec2f(x, origin.y + (size.y - BUTTON_ICON) / 2.0), ctx, app);
            x += BUTTON_ICON + BUTTON_ICON_GAP;
        }
        if !self.is_icon_only() {
            let mut text = Text::new(self.label.clone())
                .with_font_size(BUTTON_TEXT_SIZE)
                .with_line_height(1.3)
                .with_weight(FontWeight::Medium)
                .with_color(ink)
                .finish();
            let text_size = text.layout(
                SizeConstraint::loose(vec2f(self.text_width().max(0.0), size.y)),
                &mut LayoutContext::default(),
                app,
            );
            text.paint(
                vec2f(x, origin.y + (size.y - text_size.y).max(0.0) / 2.0),
                ctx,
                app,
            );
        }
        if self.focused {
            let ring = rectf(origin.x - 2.0, origin.y - 2.0, size.x + 4.0, size.y + 4.0);
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.stroke_rect(
                    ring,
                    app.theme.color(ColorToken::Focus),
                    2.0,
                    BUTTON_RADIUS + 2.0,
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
        ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };
        let callback = self.on_click.clone();
        let mut on_click = move || {
            if let Some(callback) = callback.as_ref() {
                (callback.borrow_mut())();
            }
        };
        crate::elements::interactive::handle_mouse_event(
            &mut self.state,
            event,
            bounds,
            ctx,
            &mut on_click,
        )
    }
}

/// The one-line value text a read-only row shows where a control would go.
pub fn read_only_value(app: &AppContext, value: impl Into<String>) -> Box<dyn Element> {
    crate::elements::ConstrainedBox::new(
        Text::new(value)
            .with_font_size(SETTINGS_CONTROL_TEXT_SIZE)
            .with_line_height(1.4)
            .with_color(app.theme.color(ColorToken::Muted))
            .finish(),
    )
    .with_max_width(SETTINGS_CONTROL_MIN_WIDTH)
    .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Element, LayoutContext, PaintContext, SizeConstraint};
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use crate::theme::Theme;

    fn app() -> AppContext {
        let mut app = AppContext::default();
        app.theme = Theme::dark();
        app
    }

    fn press(element: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) -> bool {
        let mut ctx = EventContext::default();
        let down = element.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            &mut ctx,
            app,
        );
        let up = element.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
            &mut ctx,
            app,
        );
        down || up
    }

    fn key(element: &mut Box<dyn Element>, app: &AppContext, key: &str) -> bool {
        element.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut EventContext::default(),
            app,
        )
    }

    fn drawn(commands: &[RenderCommand], text: &str) -> bool {
        commands.iter().any(|command| match command {
            RenderCommand::DrawText { text: drawn, .. } => drawn == text,
            _ => false,
        })
    }

    #[test]
    fn select_opens_an_anchored_menu_and_takes_a_row() {
        let app = app();
        let chosen = Rc::new(Cell::new(usize::MAX));
        let seen = chosen.clone();
        let open = Rc::new(RefCell::new(false));
        let mut select: Box<dyn Element> = Box::new(
            SettingsSelect::new(["Manual", "Plan", "Auto-review"])
                .with_selected(0)
                .with_width(300.0)
                .with_open(open.clone())
                .with_on_change(move |index| seen.set(index)),
        );
        let _ = render_element(&mut select, vec2f(400.0, 300.0), &app);
        assert!(press(&mut select, &app, vec2f(20.0, 10.0)));
        let commands = render_element(&mut select, vec2f(400.0, 300.0), &app);
        assert!(drawn(&commands, "Plan"), "the menu lists every option");
        assert!(drawn(&commands, "Auto-review"));
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "check"
            )),
            "the current option is checked"
        );
        // The panel starts 6px under the trigger, then its 6px padding, so the
        // second row's centre is at 32 + 6 + 6 + 32 + 16.
        assert!(press(&mut select, &app, vec2f(20.0, 94.0)));
        assert_eq!(chosen.get(), 1, "the row runs the change callback");
        assert!(!*open.borrow());
    }

    #[test]
    fn select_follows_up_down_enter_and_escape() {
        let app = app();
        let chosen = Rc::new(Cell::new(usize::MAX));
        let seen = chosen.clone();
        let open = Rc::new(RefCell::new(false));
        let mut select: Box<dyn Element> = Box::new(
            SettingsSelect::new(["Manual", "Plan", "Auto-review"])
                .with_selected(0)
                .with_width(300.0)
                .with_open(open.clone())
                .with_on_change(move |index| seen.set(index)),
        );
        let _ = render_element(&mut select, vec2f(400.0, 300.0), &app);
        assert!(key(&mut select, &app, "ArrowDown"));
        assert!(*open.borrow(), "Down opens the closed select");
        let _ = render_element(&mut select, vec2f(400.0, 300.0), &app);
        assert!(key(&mut select, &app, "ArrowDown"));
        assert!(key(&mut select, &app, "Enter"));
        assert_eq!(chosen.get(), 1, "Down moved from Manual to Plan");
        assert!(!*open.borrow());
    }

    #[test]
    fn select_escape_and_outside_press_dismiss() {
        let app = app();
        let open = Rc::new(RefCell::new(true));
        let mut select: Box<dyn Element> = Box::new(
            SettingsSelect::new(["One", "Two"])
                .with_width(200.0)
                .with_open(open.clone()),
        );
        let _ = render_element(&mut select, vec2f(400.0, 300.0), &app);
        assert!(key(&mut select, &app, "Escape"));
        assert!(!*open.borrow(), "Escape closes the menu");

        *open.borrow_mut() = true;
        let _ = render_element(&mut select, vec2f(400.0, 300.0), &app);
        let _ = press(&mut select, &app, vec2f(390.0, 290.0));
        assert!(!*open.borrow(), "a press outside the panel closes it");
    }

    #[test]
    fn a_disabled_select_stays_shut() {
        let app = app();
        let open = Rc::new(RefCell::new(false));
        let mut select: Box<dyn Element> = Box::new(
            SettingsSelect::new(["One", "Two"])
                .with_disabled(true)
                .with_width(200.0)
                .with_open(open.clone()),
        );
        let _ = render_element(&mut select, vec2f(400.0, 300.0), &app);
        assert!(!press(&mut select, &app, vec2f(20.0, 10.0)));
        assert!(!*open.borrow());
    }

    #[test]
    fn segmented_clicks_left_and_right_and_hover() {
        let app = app();
        let chosen = Rc::new(Cell::new(usize::MAX));
        let seen = chosen.clone();
        let mut segmented: Box<dyn Element> = Box::new(
            SettingsSegmented::new(["Queue", "Send Immediately"])
                .with_selected(0)
                .with_on_change(move |index| seen.set(index)),
        );
        let commands = render_element(&mut segmented, vec2f(400.0, 60.0), &app);
        assert!(drawn(&commands, "Queue") && drawn(&commands, "Send Immediately"));
        let size = segmented.size().unwrap();
        let second_x = size.x - 20.0;
        assert!(press(&mut segmented, &app, vec2f(second_x, 16.0)));
        assert_eq!(chosen.get(), 1);

        assert!(key(&mut segmented, &app, "ArrowLeft"));
        assert_eq!(chosen.get(), 0);
    }

    #[test]
    fn segmented_selected_pill_is_rounded() {
        let app = app();
        let mut segmented: Box<dyn Element> =
            Box::new(SettingsSegmented::new(["On", "Off"]).with_selected(0));
        let commands = render_element(&mut segmented, vec2f(300.0, 60.0), &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { corner_radius, .. } if (*corner_radius - SEGMENT_RADIUS).abs() < 0.01
            )),
            "the selected segment is an 8px pill"
        );
    }

    #[test]
    fn buttons_are_28_or_32_and_keep_their_radius() {
        let app = app();
        let mut small: Box<dyn Element> = Box::new(SettingsButton::new("Change"));
        let _ = small.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!((small.size().unwrap().y - 28.0).abs() < 0.01);
        let mut large: Box<dyn Element> = Box::new(SettingsButton::new("Change").large());
        let _ = large.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!((large.size().unwrap().y - 32.0).abs() < 0.01);
        assert_eq!(large.size().unwrap().x, small.size().unwrap().x);

        let mut icon: Box<dyn Element> = Box::new(SettingsButton::icon("x"));
        let _ = icon.layout(
            SizeConstraint::loose(vec2f(300.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(icon.size().unwrap(), vec2f(28.0, 28.0));

        let commands = render_element(&mut small, vec2f(300.0, 100.0), &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { corner_radius, .. } if (*corner_radius - BUTTON_RADIUS).abs() < 0.01
            )),
            "the button fill is rounded to 8"
        );
        let _ = PaintContext::default();
    }
}
