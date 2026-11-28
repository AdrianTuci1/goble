//! The settings surface's own elements: the rail, the page, the section card,
//! the row, and the controls a row puts on its right.
//!
//! The geometry is T3 Code's settings page, measured in the recovered design
//! document (`/tmp/settings-t3-parity-spec-recovered.md`, §1.13 and §3.1) and
//! translated into goble's tokens:
//!
//! - rail **200px**, 8px padding, 32px rows at a 10px inset, radius 8, 14px
//!   label, selected 14/500, 2px row gap, no dividers, 2px focus ring;
//! - page `24px` padding, 896px content cap, `32px` between sections;
//! - section header **16/600** in a 28px line, `10px` above its group;
//! - group card **radius 12**, 1px `Border` at 60%, fill `Surface` at 40% over
//!   `Bg`, one 1px `Border`/50 rule *between* rows and none at the edges;
//! - row padding **16×12**, label `14/500`, hint `12` muted at 18px leading and
//!   a 672px cap, control right-aligned with a 32px gap, 2px focus ring.
//!
//! Every colour is a [`ColorToken`] resolved through the app's [`Theme`], so a
//! page renders the same in the dark and the light theme; nothing here hardcodes
//! a palette value. Elements that stand for something this build cannot do are
//! drawn disabled (see [`row::SettingsRow::with_disabled`]) and carry the
//! explanation in the row's own text, not in a hover tooltip only.
//!
//! The app composes these elements in `apps/desktop/src/ui/settings/`; keeping
//! them here instead means the harness and the settings tab cannot drift into
//! two different row anatomies (the design document's D4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, RectF, Vector2F};
use crate::theme::ColorToken;

pub mod fields;
pub mod fold;
pub mod key_chip;
pub mod nav;
pub mod row;
pub mod secret;
pub mod section;
pub mod slider;
pub mod status;
pub mod toggle;

#[cfg(test)]
mod tests;

pub use fields::{
    SettingsButton, SettingsPathField, SettingsSegmented, SettingsSelect, SettingsTextField,
};
pub use fold::SettingsFold;
pub use key_chip::SettingsKeyChip;
pub use nav::{SettingsNav, SettingsNavGroup, SettingsNavItem};
pub use row::{SettingsRow, SettingsRowStatus, SettingsRowStatusKind};
pub use secret::SettingsSecretField;
pub use section::SettingsSection;
pub use slider::SettingsSlider;
pub use status::{SettingsNotice, SettingsNoticeKind};
pub use toggle::SettingsToggle;

// ---------------------------------------------------------------------------
// Geometry, in one place so a page cannot grow a second row anatomy
// ---------------------------------------------------------------------------

/// The rail's width: T3's `w-60` (240) is narrowed to what a 14px label plus a
/// 10px inset needs with no icons in the row.
pub const SETTINGS_RAIL_WIDTH: f32 = 200.0;
/// The rail's inner padding, all four sides.
pub const SETTINGS_RAIL_PADDING: f32 = 8.0;
/// One rail row: 32px tall (T3's `h-8`).
pub const SETTINGS_NAV_ROW_HEIGHT: f32 = 32.0;
/// The rail row's horizontal inset, T3's `--sidebar-row-content-inset` (10).
pub const SETTINGS_NAV_ROW_INSET: f32 = 10.0;
/// The rail row's corner radius (T3's `--control-radius` = 8).
pub const SETTINGS_NAV_ROW_RADIUS: f32 = 8.0;
/// The gap between rail rows (2px: tighter than T3's 4px because these rows are
/// 32px tall and carry no leading icon).
pub const SETTINGS_NAV_ROW_GAP: f32 = 2.0;
/// A rail row's label size.
pub const SETTINGS_NAV_LABEL_SIZE: f32 = 14.0;
/// A rail group's muted heading.
pub const SETTINGS_NAV_GROUP_LABEL_SIZE: f32 = 11.0;
/// The rail search row's height (T3's 32px search row).
pub const SETTINGS_SEARCH_ROW_HEIGHT: f32 = 32.0;

/// The settings page's content cap (T3's `max-w-4xl` = 896).
pub const SETTINGS_PAGE_MAX_WIDTH: f32 = 896.0;
/// The page's own padding, all four sides (T3's `px-5 pt-6 pb-12` at desktop
/// is 24/24/24/48; goble uses 24 uniformly, which the parity doc allows).
pub const SETTINGS_PAGE_PADDING: f32 = 24.0;
/// The gap between two sections on a page (T3's `gap-8`).
pub const SETTINGS_SECTION_GAP: f32 = 32.0;
/// A section header's line box (`min-h-7`).
pub const SETTINGS_SECTION_HEADER_HEIGHT: f32 = 28.0;
/// A section header's type size (`text-base font-semibold`).
pub const SETTINGS_SECTION_HEADER_SIZE: f32 = 16.0;
/// The gap between a section header and its group card (`space-y-2.5` = 10).
pub const SETTINGS_SECTION_HEADER_GAP: f32 = 10.0;
/// The group card's corner radius (`rounded-xl` = 12).
pub const SETTINGS_GROUP_RADIUS: f32 = 12.0;
/// A fold's header line: T3's `min-h-9` group header, compact enough that the
/// Providers page's one-fold-per-CLI list reads as a list of headers.
pub const SETTINGS_FOLD_HEADER_HEIGHT: f32 = 36.0;

/// The side sheet's width: the add/edit form docked to the settings pane's
/// trailing edge (T3's detail pane / DSH's side panel).
pub const SETTINGS_SHEET_WIDTH: f32 = 380.0;
/// The side sheet's inner padding.
pub const SETTINGS_SHEET_PADDING: f32 = 16.0;

/// A row's horizontal padding (`px-3 sm:px-4` = 16).
pub const SETTINGS_ROW_H_PADDING: f32 = 16.0;
/// A row's vertical padding (`py-3` = 12).
pub const SETTINGS_ROW_V_PADDING: f32 = 12.0;
/// A row's label size (`text-sm font-medium` = 14).
pub const SETTINGS_ROW_LABEL_SIZE: f32 = 14.0;
/// A row's description size (`text-xs` = 12).
pub const SETTINGS_ROW_HINT_SIZE: f32 = 12.0;
/// A row's description line box (`leading-normal`, 1.5 × 12 = 18).
pub const SETTINGS_ROW_HINT_LINE_HEIGHT: f32 = 18.0;
/// The description's width cap (T3's `max-w-2xl` = 672).
pub const SETTINGS_ROW_HINT_MAX_WIDTH: f32 = 672.0;
/// The label column / control gap (`gap-8` = 32).
pub const SETTINGS_ROW_CONTROL_GAP: f32 = 32.0;
/// The 4px between a label and its description (`space-y-1`).
pub const SETTINGS_ROW_TEXT_GAP: f32 = 4.0;

/// A control's height: T3's compact trigger is 32px; goble's fields sit here.
pub const SETTINGS_CONTROL_HEIGHT: f32 = 32.0;
/// A control's corner radius (`rounded-lg` = 8).
pub const SETTINGS_CONTROL_RADIUS: f32 = 8.0;
/// A select trigger's minimum width (`min-w-40` = 160).
pub const SETTINGS_CONTROL_MIN_WIDTH: f32 = 160.0;
/// A control's type size.
pub const SETTINGS_CONTROL_TEXT_SIZE: f32 = 13.0;

/// The grouped key-chip pill's height, and the muted chord inside it.
pub const SETTINGS_KEY_CHIP_HEIGHT: f32 = 24.0;
pub const SETTINGS_KEY_CHIP_TEXT_SIZE: f32 = 12.0;

/// The square switch's track (DSH/T3 switches are a rounded pill; the owner
/// asked for the square switch kept, so the track and thumb are right angles).
pub const SETTINGS_TOGGLE_WIDTH: f32 = 32.0;
pub const SETTINGS_TOGGLE_HEIGHT: f32 = 18.0;

/// A row's own focus ring: 2px, inset 2 (`focus-visible:ring-2 ring-inset`).
pub const SETTINGS_FOCUS_RING_WIDTH: f32 = 2.0;
pub const SETTINGS_FOCUS_RING_INSET: f32 = 2.0;
/// The row's focus-ring radius: the group's 12.
pub const SETTINGS_FOCUS_RING_RADIUS: f32 = 12.0;

// ---------------------------------------------------------------------------
// Colour helpers: token maths, never a literal colour
// ---------------------------------------------------------------------------

/// A token mixed toward the background by `t` — the shader-free way to say
/// "this fill is 40% of the card over the page".
pub(crate) fn over_bg(app: &AppContext, token: ColorToken, t: f32) -> ColorU {
    app.theme
        .color(ColorToken::Bg)
        .mix(&app.theme.color(token), t)
}

/// A token mixed toward another token by `t`.
pub(crate) fn mix(app: &AppContext, from: ColorToken, toward: ColorToken, t: f32) -> ColorU {
    app.theme.color(from).mix(&app.theme.color(toward), t)
}

/// The group card's fill: `bg-card/40` — `Surface` at 40% over `Bg`.
pub(crate) fn group_fill(app: &AppContext) -> ColorU {
    over_bg(app, ColorToken::Surface, 0.40)
}

/// The group card's border: `border-border/60` over the card fill.
pub(crate) fn group_border(app: &AppContext) -> ColorU {
    group_fill(app).mix(&app.theme.color(ColorToken::Border), 0.60)
}

/// A rule between two rows: `border-border/50` over the card fill.
pub(crate) fn row_rule(app: &AppContext) -> ColorU {
    group_fill(app).mix(&app.theme.color(ColorToken::Border), 0.50)
}

/// A row's content at `opacity` over the card fill — the honest stand-in for
/// CSS opacity in a renderer that has no group-alpha command. At `0.64` it is
/// exactly T3's `data-disabled:opacity-64`.
pub(crate) fn dim(app: &AppContext, color: ColorU, opacity: f32) -> ColorU {
    app.theme
        .color(ColorToken::Bg)
        .mix(&color, opacity.clamp(0.0, 1.0))
}

// ---------------------------------------------------------------------------
// Building blocks shared by the elements in this module
// ---------------------------------------------------------------------------
pub(crate) fn text_color(
    value: impl Into<String>,
    size: f32,
    color: ColorU,
    weight: crate::platform::text_atlas::FontWeight,
) -> Box<dyn Element> {
    crate::elements::Text::new(value)
        .with_font_size(size)
        .with_line_height(1.4)
        .with_weight(weight)
        .with_color(color)
        .finish()
}

pub(crate) fn row(children: Vec<Box<dyn Element>>, spacing: f32) -> Box<dyn Element> {
    crate::elements::Flex::row()
        .with_spacing(spacing)
        .with_children(children)
        .finish()
}

pub(crate) fn column(children: Vec<Box<dyn Element>>, spacing: f32) -> Box<dyn Element> {
    crate::elements::Flex::column()
        .with_spacing(spacing)
        .with_children(children)
        .finish()
}

/// A vertical stack whose children fill the available width. A Flex column
/// hands its children an unbounded cross axis unless it is told to stretch, and
/// every settings surface is a full-width stack — the card, the row, the page —
/// so this is the default for them.
pub(crate) fn stretch_column(children: Vec<Box<dyn Element>>, spacing: f32) -> Box<dyn Element> {
    crate::elements::Flex::column()
        .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Stretch)
        .with_spacing(spacing)
        .with_children(children)
        .finish()
}

/// Wraps a child in the 2px inset focus ring the design document specifies for
/// a focused row or rail row. The ring is drawn *after* the child, so it sits
/// over the selected fill instead of replacing it.
pub(crate) struct FocusOutline {
    child: Box<dyn Element>,
    color: ColorU,
    width: f32,
    inset: f32,
    radius: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl FocusOutline {
    pub(crate) fn new(
        child: Box<dyn Element>,
        color: ColorU,
        width: f32,
        inset: f32,
        radius: f32,
    ) -> Self {
        Self {
            child,
            color,
            width,
            inset,
            radius,
            size: None,
            origin: None,
        }
    }
}

impl Element for FocusOutline {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
        if let Some(size) = self.size {
            let rect = rectf(
                origin.x + self.inset,
                origin.y + self.inset,
                (size.x - self.inset * 2.0).max(0.0),
                (size.y - self.inset * 2.0).max(0.0),
            );
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.stroke_rect(rect, self.color, self.width, self.radius);
            }
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// A 44px section heading line isn't part of this module; keeping the one
/// helper for the "—" empty-value glyph means every page renders an unset value
/// the same way./// Whether an event is a primary-button press inside `bounds`. Pages route
/// keyboard themselves; a pointer press is the one thing every settings control
/// adds on top of its paint./// A click callback shared between a built row and the app. `Rc<RefCell<...>>`
/// matches the rest of the app's element callbacks and keeps the settings
/// elements allocation-only (no `Arc`, no channels).
pub type ClickCallback = Rc<RefCell<dyn FnMut()>>;

pub(crate) fn call(callback: &Option<ClickCallback>) {
    if let Some(callback) = callback.as_ref() {
        (callback.borrow_mut())();
    }
}
/// The fill colour a row's control surface uses, shared by text fields and
/// select triggers so they read as one control family.
pub(crate) fn control_fill(app: &AppContext) -> ColorU {
    over_bg(app, ColorToken::SurfaceRaised, 0.55)
}

/// The hairline around a control, `--input` in T3: a step stronger than a rule.
pub(crate) fn control_border(app: &AppContext) -> ColorU {
    app.theme
        .color(ColorToken::Border)
        .mix(&app.theme.color(ColorToken::Text), 0.08)
}

/// The event-side click helper every settings control shares: a primary press
/// inside the element's bounds runs the callback and claims the event.
pub(crate) fn dispatch_click(
    event: &DispatchedEvent,
    bounds: Option<RectF>,
    callback: &Option<ClickCallback>,
) -> bool {
    let Some(bounds) = bounds else {
        return false;
    };
    match event {
        DispatchedEvent::MouseDown { position, button } if *button == 0 => {
            if bounds.contains(crate::geometry::PointF::new(position.x, position.y)) {
                call(callback);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
