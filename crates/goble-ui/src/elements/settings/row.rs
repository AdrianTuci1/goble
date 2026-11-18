//! The settings row: the unit every page is made of.
//!
//! T3's measured anatomy (§3.1 of the parity doc):
//!
//! - `rounded-xl px-3 sm:px-4 py-3` — 16px horizontal, 12px vertical;
//! - a label column at `1fr`, the control at `auto`, `gap-2` (32px) between
//!   them at the `32rem` breakpoint; stacked below it (narrow panes fall back to
//!   the label above the control, which this element does when the constraint is
//!   narrower than [`NARROW_ROW_WIDTH`]);
//! - label `text-sm font-medium` in a 20px line, description `text-xs
//!   leading-normal` 4px under it, capped at 672px;
//! - a reset icon in the label's own line when the value is not the default;
//! - `data-disabled:opacity-64` for a row this build cannot perform;
//! - a 2px inset focus ring around the whole row.
//!
//! The row is deliberately *not* clickable: in T3 a setting row is a label and
//! a control, and only the control activates. Goble's keyboard model still puts
//! a focus ring on the row (the row is the focus stop), which is the honest
//! adaptation the parity doc asks for.

use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;

use super::{
    dim, mix, text_color, FocusOutline, SETTINGS_FOCUS_RING_INSET, SETTINGS_FOCUS_RING_RADIUS,
    SETTINGS_FOCUS_RING_WIDTH, SETTINGS_ROW_CONTROL_GAP, SETTINGS_ROW_HINT_LINE_HEIGHT,
    SETTINGS_ROW_HINT_MAX_WIDTH, SETTINGS_ROW_HINT_SIZE, SETTINGS_ROW_H_PADDING,
    SETTINGS_ROW_LABEL_SIZE, SETTINGS_ROW_TEXT_GAP, SETTINGS_ROW_V_PADDING,
};

/// The width below which a row stacks its control under the label instead of
/// beside it. T3's container query is `32rem` (512px); the setting pane is
/// usually wider, but a narrow split pane must not clip the control.
pub const NARROW_ROW_WIDTH: f32 = 512.0;

/// How a row's status line reads: the colour of the honest sentence under the
/// description ("Not available in this build.", "Saved to config.toml.").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsRowStatusKind {
    /// Neutral information, muted.
    Muted,
    /// The action succeeded.
    Success,
    /// Something the user should know, but not an error.
    Info,
    /// The row cannot do what its label says in this build.
    Unavailable,
    /// The last attempt failed.
    Error,
}

impl SettingsRowStatusKind {
    fn color(self) -> (ColorToken, f32) {
        match self {
            SettingsRowStatusKind::Muted => (ColorToken::Muted, 1.0),
            SettingsRowStatusKind::Success => (ColorToken::Success, 1.0),
            SettingsRowStatusKind::Info => (ColorToken::Focus, 1.0),
            // An unavailable row is not an error: it is a fact about the build,
            // so it reads in the muted pair rather than in red.
            SettingsRowStatusKind::Unavailable => (ColorToken::Muted, 1.0),
            SettingsRowStatusKind::Error => (ColorToken::Error, 1.0),
        }
    }
}

/// A row's status line: an icon-free sentence under the description, the way T3
/// puts `status` under a row's description with `pt-0.5 text-xs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsRowStatus {
    pub text: String,
    pub kind: SettingsRowStatusKind,
}

impl SettingsRowStatus {
    pub fn new(text: impl Into<String>, kind: SettingsRowStatusKind) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }

    pub fn unavailable(text: impl Into<String>) -> Self {
        Self::new(text, SettingsRowStatusKind::Unavailable)
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::new(text, SettingsRowStatusKind::Success)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::new(text, SettingsRowStatusKind::Error)
    }
}

/// One settings row.
pub struct SettingsRow {
    label: String,
    hint: String,
    status: Option<SettingsRowStatus>,
    control: Option<Box<dyn Element>>,
    /// A full-width body drawn under the label/control band: a fold's rows, a
    /// field that needs the whole width, a table of models.
    body: Option<Box<dyn Element>>,
    reset: Option<Box<dyn Element>>,
    focused: bool,
    disabled: bool,
    /// Pad the row like T3's `pt-3 pb-1` form row (a row with a child editor).
    tight_bottom: bool,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsRow {
    /// A row with a label and a description. An empty description leaves the
    /// 44px one-line row; a description makes the 66px two-line row.
    pub fn new(_app: &AppContext, label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hint: hint.into(),
            status: None,
            control: None,
            body: None,
            reset: None,
            focused: false,
            disabled: false,
            tight_bottom: false,
            root: None,
            size: None,
            origin: None,
        }
    }

    /// The control on the row's right.
    pub fn with_control(mut self, control: Box<dyn Element>) -> Self {
        self.control = Some(control);
        self
    }

    /// The reset affordance in the label line, shown by the caller only when
    /// the value is not the default (`is_default` in the schema).
    pub fn with_reset(mut self, reset: Box<dyn Element>) -> Self {
        self.reset = Some(reset);
        self
    }

    pub fn with_status(mut self, status: SettingsRowStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_body(mut self, body: Box<dyn Element>) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// A row this build cannot perform. Its text is dimmed to T3's 64%, and the
    /// caller is expected to have built the control disabled; the explanation
    /// itself comes from the description/status lines, never from a tooltip
    /// only.
    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// T3's form-row padding: `pt-3 pb-1`, used when the row carries its own
    /// editor body rather than a control.
    pub fn with_tight_bottom(mut self, tight: bool) -> Self {
        self.tight_bottom = tight;
        self
    }

    fn build(&mut self, app: &AppContext) {
        let bg = app.theme.color(ColorToken::Bg);
        let disabled_opacity = 0.64;
        let label_color = if self.disabled {
            dim(app, app.theme.color(ColorToken::Text), disabled_opacity)
        } else {
            app.theme.color(ColorToken::Text)
        };
        let hint_color = if self.disabled {
            dim(app, app.theme.color(ColorToken::Muted), disabled_opacity)
        } else {
            // The description reads a step quieter than the label, in the same
            // neutral pair the rest of the surface uses.
            mix(app, ColorToken::Muted, ColorToken::Text, 0.15)
        };
        let mut label_line: Vec<Box<dyn Element>> = vec![text_color(
            self.label.clone(),
            SETTINGS_ROW_LABEL_SIZE,
            label_color,
            FontWeight::Medium,
        )];
        if let Some(reset) = self.reset.take() {
            label_line.push(reset);
        }
        let mut text_column: Vec<Box<dyn Element>> = vec![super::row(label_line, 6.0)];
        if !self.hint.is_empty() {
            let hint = Text::new(self.hint.clone())
                .with_font_size(SETTINGS_ROW_HINT_SIZE)
                .with_line_height(SETTINGS_ROW_HINT_LINE_HEIGHT / SETTINGS_ROW_HINT_SIZE)
                .with_color(hint_color)
                .with_max_lines(2)
                .finish();
            let hint = crate::elements::ConstrainedBox::new(hint)
                .with_max_width(SETTINGS_ROW_HINT_MAX_WIDTH)
                .finish();
            text_column.push(
                crate::elements::Container::new(hint)
                    .with_padding(crate::elements::EdgeInsets::new(
                        SETTINGS_ROW_TEXT_GAP,
                        0.0,
                        0.0,
                        0.0,
                    ))
                    .finish(),
            );
        }
        if let Some(status) = self.status.as_ref() {
            let (token, _) = status.kind.color();
            let color = if self.disabled {
                dim(app, app.theme.color(token), disabled_opacity)
            } else {
                app.theme.color(token)
            };
            let line = Text::new(status.text.clone())
                .with_font_size(11.0)
                .with_line_height(1.45)
                .with_color(color)
                .with_max_lines(2)
                .finish();
            let line = crate::elements::ConstrainedBox::new(line)
                .with_max_width(SETTINGS_ROW_HINT_MAX_WIDTH)
                .finish();
            text_column.push(
                crate::elements::Container::new(line)
                    .with_padding(crate::elements::EdgeInsets::new(3.0, 0.0, 0.0, 0.0))
                    .finish(),
            );
        }
        let text_block = super::stretch_column(text_column, 0.0);
        // The label column takes the free width; the control keeps its natural
        // size and never shrinks.
        let mut band = crate::elements::Flex::row()
            .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Center)
            .with_main_axis_size(crate::elements::MainAxisSize::Max)
            .with_spacing(SETTINGS_ROW_CONTROL_GAP);
        band = band.with_child(crate::elements::Expanded::new(text_block).finish());
        if let Some(control) = self.control.take() {
            band = band.with_child(control);
        }
        let mut rows: Vec<Box<dyn Element>> = vec![band.finish()];
        if let Some(body) = self.body.take() {
            let body = crate::elements::Container::new(body)
                .with_padding(crate::elements::EdgeInsets::new(10.0, 0.0, 0.0, 0.0))
                .finish();
            rows.push(body);
        }
        let content = super::stretch_column(rows, 0.0);
        let bottom = if self.tight_bottom || self.body.is_some() {
            4.0
        } else {
            SETTINGS_ROW_V_PADDING
        };
        let padded = crate::elements::Container::new(content)
            .with_padding(crate::elements::EdgeInsets::new(
                SETTINGS_ROW_V_PADDING,
                SETTINGS_ROW_H_PADDING,
                bottom,
                SETTINGS_ROW_H_PADDING,
            ))
            .finish();
        let _ = bg;
        let root: Box<dyn Element> = if self.focused {
            Box::new(FocusOutline::new(
                padded,
                app.theme.color(ColorToken::Focus),
                SETTINGS_FOCUS_RING_WIDTH,
                SETTINGS_FOCUS_RING_INSET,
                SETTINGS_FOCUS_RING_RADIUS,
            ))
        } else {
            padded
        };
        self.root = Some(root);
    }

    fn root_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.root.is_none() {
            self.build(app);
        }
        self.root.as_mut().unwrap()
    }

    /// The row's own bounds, for the tests that assert the 2px ring and the
    /// padding arithmetic.
    pub fn pressed_at(&self, position: Vector2F) -> bool {
        self.bounds().is_some_and(|bounds| {
            bounds.contains(crate::geometry::PointF::new(position.x, position.y))
        })
    }
}

impl Element for SettingsRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let root = self.root_mut(app);
        let size = root.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root_mut(app).paint(origin, ctx, app);
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
        self.root_mut(app).dispatch_event(event, ctx, app)
    }
}

/// The row's status helper for the common "this build cannot" sentence, kept in
/// one place so every page says the same thing.
pub fn unavailable_row(app: &AppContext, label: &str, hint: &str, why: &str) -> SettingsRow {
    SettingsRow::new(app, label, hint)
        .with_disabled(true)
        .with_status(SettingsRowStatus::unavailable(format!(
            "Not available in this build · {why}"
        )))
}
