//! One focus ring and one hit-target rule for the primitives.
//!
//! Every control paints the same inset ring: `2px` in `Focus` for a control,
//! `3px` for a field, drawn `width` pixels inside the control's bounds at a
//! corner radius two points larger than the control's own. The ring is painted
//! last, over the control, so it cannot be hidden by the fill it marks.
//!
//! [`focus_ring`] wraps an element; a primitive that paints itself can call
//! [`paint_focus_ring`] directly. Both share [`FocusStyle`] from the token
//! layer, so a high-contrast theme widens every ring through the same code.
//!
//! The hit-target rule is [`hit_target`]: a row is at least 32px, an icon
//! control 24px, a close control 20px. A coarse pointer grows icon-sized
//! controls to a 44×44 hit box, but the *paint* stays the control's drawn size,
//! centred in that box, so touch targets do not restyle the chrome.

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::render::Renderer;
use crate::theme::{FocusStyle, Theme};

/// The minimum laid-out side of a row control.
pub const ROW_TARGET: f32 = 32.0;
/// The minimum laid-out side of an icon control.
pub const ICON_TARGET: f32 = 24.0;
/// The minimum laid-out side of a close control.
pub const CLOSE_TARGET: f32 = 20.0;
/// The hit box a coarse pointer needs around an icon-sized control.
pub const COARSE_TARGET: f32 = 44.0;

/// Which minimum-target rule a primitive follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetClass {
    /// A row: at least [`ROW_TARGET`] in both directions.
    Row,
    /// An icon button: at least [`ICON_TARGET`] in both directions.
    #[default]
    Icon,
    /// A close button: at least [`CLOSE_TARGET`] in both directions.
    Close,
    /// No minimum beyond the caller's explicit size. For a glyph box that is
    /// deliberately drawn smaller than an icon target (a card's 16px overflow
    /// dots), the caller's size is the whole target.
    None,
}

impl TargetClass {
    /// The minimum laid-out side for this class.
    pub const fn min_size(self) -> f32 {
        match self {
            TargetClass::Row => ROW_TARGET,
            TargetClass::Icon => ICON_TARGET,
            TargetClass::Close => CLOSE_TARGET,
            TargetClass::None => 0.0,
        }
    }
}

/// Resolve a control's laid-out size and the size it paints at.
///
/// `visual` is the size the control would take from its content. The first
/// value is the hit target (what `layout` returns, so pointer input and the
/// parent's layout both use it). The second is the drawn size: a row grows
/// into its target because the row *is* its hit band; an icon or close control
/// only grows the invisible box, so a 16px glyph stays a 16px glyph, and a
/// coarse pointer takes the same rule to [`COARSE_TARGET`]. The primitive
/// centres its paint in the larger box.
pub fn hit_target(
    visual: Vector2F,
    class: TargetClass,
    coarse_pointer: bool,
) -> (Vector2F, Vector2F) {
    let icon_sized = matches!(class, TargetClass::Icon | TargetClass::Close);
    let coarse_icon = coarse_pointer && icon_sized;
    let min = if coarse_icon {
        COARSE_TARGET
    } else {
        class.min_size()
    };
    let laid_out = vec2f(visual.x.max(min), visual.y.max(min));
    // A row grows into its 32px target, because the row *is* the hit band. An
    // icon or close control only grows the invisible hit box: its paint keeps
    // the drawn size and is centred, so a 16px glyph stays a 16px glyph.
    let painted = if class == TargetClass::Row {
        laid_out
    } else {
        visual
    };
    (laid_out, painted)
}

/// The top-left corner that centres `painted` inside `laid_out`.
pub fn centred_origin(origin: Vector2F, laid_out: Vector2F, painted: Vector2F) -> Vector2F {
    vec2f(
        origin.x + (laid_out.x - painted.x).max(0.0) / 2.0,
        origin.y + (laid_out.y - painted.y).max(0.0) / 2.0,
    )
}

/// Paint one inset focus ring over `bounds`.
///
/// `radius` is the control's own corner radius; the ring is drawn at
/// `radius + 2`. The ring's rect is inset by its own width, so a 2px control
/// ring sits 2px inside the control and a 3px field ring 3px inside it.
pub fn paint_focus_ring(
    renderer: &mut Renderer,
    bounds: RectF,
    focused: bool,
    style: FocusStyle,
    radius: f32,
    theme: &Theme,
) {
    if !focused {
        return;
    }
    let width = theme.focus_width(style);
    let rect = rectf(
        bounds.min_x() + width,
        bounds.min_y() + width,
        (bounds.width() - width * 2.0).max(0.0),
        (bounds.height() - width * 2.0).max(0.0),
    );
    renderer.stroke_rect(rect, style.color(theme), width, radius + 2.0);
}

/// One focus ring around an element, painted over it.
///
/// The wrapper takes no layout room of its own: it lays its child out at the
/// same constraint and reports the child's size, then strokes the ring inside
/// those bounds. An unfocused ring is a pure pass-through.
pub struct FocusRing {
    child: Box<dyn Element>,
    focused: bool,
    style: FocusStyle,
    /// The child's own corner radius. `None` uses the theme's radius.
    radius: Option<f32>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl FocusRing {
    pub fn new(child: Box<dyn Element>, focused: bool, style: FocusStyle) -> Self {
        Self {
            child,
            focused,
            style,
            radius: None,
            size: None,
            origin: None,
        }
    }

    /// The child's own corner radius; the ring is painted at this plus two.
    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }
}

impl Element for FocusRing {
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
        let size = self.size.unwrap_or(Vector2F::zero());
        if let Some(renderer) = ctx.renderer.as_mut() {
            let radius = self.radius.unwrap_or_else(|| app.theme.radius_px());
            paint_focus_ring(
                renderer,
                rectf(origin.x, origin.y, size.x, size.y),
                self.focused,
                self.style,
                radius,
                &app.theme,
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}

/// Wrap `element` in one focus ring.
///
/// The ring is the control style by default; a field passes
/// [`FocusStyle::Field`]. The wrapper is a no-op visually when `focused` is
/// false, so a host can call it unconditionally.
pub fn focus_ring(element: Box<dyn Element>, focused: bool, style: FocusStyle) -> Box<dyn Element> {
    FocusRing::new(element, focused, style).finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{
        Button, Checkbox, Chip, ComposerButton, Empty, Flex, HoverRow, Icon, SearchInput, Text,
        TextInput, TopbarButton,
    };
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use crate::theme::{ColorToken, FocusStyle};

    /// Every `Focus`-coloured stroke the commands paint, as `(rect, width,
    /// radius)`.
    fn focus_rings(commands: &[RenderCommand], app: &AppContext) -> Vec<(RectF, f32, f32)> {
        let focus = app.theme.color(ColorToken::Focus);
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::StrokeRect {
                    rect,
                    color,
                    width,
                    corner_radius,
                } if *color == focus => Some((*rect, *width, *corner_radius)),
                _ => None,
            })
            .collect()
    }

    fn assert_one_ring(
        what: &str,
        commands: &[RenderCommand],
        app: &AppContext,
        width: f32,
        radius: f32,
    ) {
        let rings = focus_rings(commands, app);
        assert_eq!(
            rings.len(),
            1,
            "{what} paints exactly one focus ring, got {rings:?}"
        );
        let (rect, got_width, got_radius) = rings[0];
        assert_eq!(got_width, width, "{what}'s ring width: {rect:?}");
        assert_eq!(got_radius, radius, "{what}'s ring radius: {rect:?}");
    }

    fn render(element: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        render_element(element, vec2f(400.0, 200.0), app)
    }

    fn button(focused: bool) -> Box<dyn Element> {
        Box::new(
            Button::new(
                Icon::new("x")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, &AppContext::default())
                    .finish(),
            )
            .with_focused(focused),
        )
    }

    #[test]
    fn every_primitive_paints_one_control_ring_when_focused() {
        let app = AppContext::default();
        let control = app.theme.focus_width(FocusStyle::Control);
        let field = app.theme.focus_width(FocusStyle::Field);
        let radius = app.theme.radius_px();

        let mut cases: Vec<(&str, Box<dyn Element>, f32, f32)> = vec![
            ("Button", button(true), control, radius + 2.0),
            (
                "Checkbox",
                Box::new(
                    Checkbox::new()
                        .with_label(Text::new("Enabled").finish())
                        .with_focused(true),
                ),
                control,
                radius * 0.5 + 2.0,
            ),
            (
                "HoverRow",
                Box::new(
                    HoverRow::new(Text::new("Row").finish())
                        .with_corner_radius(radius)
                        .with_focused(true),
                ),
                control,
                radius + 2.0,
            ),
            (
                "Chip",
                Box::new(Chip::new(Text::new("Chip").finish()).with_focused(true)),
                control,
                2.0,
            ),
            (
                "ComposerButton",
                Box::new(ComposerButton::new(Text::new("Model").finish()).with_focused(true)),
                control,
                4.0 + 2.0,
            ),
            (
                "TopbarButton",
                Box::new(
                    TopbarButton::new(Icon::new("settings").with_size(16.0).finish())
                        .with_focused(true),
                ),
                control,
                radius / 2.0 + 2.0,
            ),
            (
                "TextInput",
                Box::new(TextInput::new().with_value("hi").with_focused(true)),
                field,
                radius + 2.0,
            ),
            (
                "SearchInput",
                Box::new(SearchInput::new().with_value("hi").with_focused(true)),
                field,
                2.0,
            ),
        ];

        for (what, element, width, ring_radius) in cases.iter_mut() {
            let commands = render(element, &app);
            assert_one_ring(what, &commands, &app, *width, *ring_radius);
        }
    }

    #[test]
    fn every_primitive_paints_no_ring_when_unfocused() {
        let app = AppContext::default();
        let mut cases: Vec<(&str, Box<dyn Element>)> = vec![
            ("Button", button(false)),
            (
                "Checkbox",
                Box::new(Checkbox::new().with_label(Text::new("Enabled").finish())),
            ),
            (
                "HoverRow",
                Box::new(HoverRow::new(Text::new("Row").finish())),
            ),
            ("Chip", Box::new(Chip::new(Text::new("Chip").finish()))),
            (
                "ComposerButton",
                Box::new(ComposerButton::new(Text::new("Model").finish())),
            ),
            (
                "TopbarButton",
                Box::new(TopbarButton::new(
                    Icon::new("settings").with_size(16.0).finish(),
                )),
            ),
            ("TextInput", Box::new(TextInput::new().with_value("hi"))),
            ("SearchInput", Box::new(SearchInput::new().with_value("hi"))),
        ];
        for (what, element) in cases.iter_mut() {
            let commands = render(element, &app);
            assert!(
                focus_rings(&commands, &app).is_empty(),
                "an unfocused {what} paints no focus ring: {commands:?}"
            );
        }
    }

    #[test]
    fn a_button_with_a_ten_pixel_glyph_is_at_least_an_icon_target() {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(Button::new(
            Icon::new("x")
                .with_size(10.0)
                .with_theme_color(ColorToken::Muted, &app)
                .finish(),
        ));
        let size = element.layout(
            SizeConstraint::loose(vec2f(400.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x >= ICON_TARGET, "width {size:?}");
        assert!(size.y >= ICON_TARGET, "height {size:?}");
    }

    #[test]
    fn a_row_control_is_at_least_32px() {
        let app = AppContext::default();
        let mut row = HoverRow::new(Text::new("Row").finish());
        let size = row.layout(
            SizeConstraint::loose(vec2f(400.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.y >= ROW_TARGET, "row height {size:?}");
    }

    #[test]
    fn a_coarse_pointer_is_a_44px_hit_box_around_the_unchanged_paint() {
        let app = AppContext::default();
        let child = || Empty::new().with_size(vec2f(10.0, 10.0)).finish();
        let paint_rects = |commands: &[RenderCommand]| -> Vec<RectF> {
            commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::FillRect { rect, .. } => Some(*rect),
                    _ => None,
                })
                .collect()
        };

        let mut fine: Box<dyn Element> = Box::new(Button::new(child()));
        let mut coarse: Box<dyn Element> = Box::new(Button::new(child()).with_coarse_pointer(true));
        let fine_size = fine.layout(
            SizeConstraint::loose(vec2f(400.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let coarse_size = coarse.layout(
            SizeConstraint::loose(vec2f(400.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(coarse_size, vec2f(COARSE_TARGET, COARSE_TARGET));
        assert!(
            fine_size.x < COARSE_TARGET && fine_size.y < COARSE_TARGET,
            "a fine pointer keeps the button's drawn box: {fine_size:?}"
        );

        let fine_commands = render(&mut fine, &app);
        let coarse_commands = render(&mut coarse, &app);
        let fine_paint = paint_rects(&fine_commands);
        let coarse_paint = paint_rects(&coarse_commands);
        assert_eq!(
            fine_paint.len(),
            coarse_paint.len(),
            "the coarse pointer paints the same commands"
        );
        for (fine, coarse) in fine_paint.iter().zip(coarse_paint.iter()) {
            assert_eq!(
                (fine.width(), fine.height()),
                (coarse.width(), coarse.height()),
                "the coarse pointer changes the hit box, not the paint size"
            );
        }
        assert!(
            coarse_paint
                .iter()
                .all(|rect| rect.width() < COARSE_TARGET && rect.height() < COARSE_TARGET),
            "the coarse box paints at the drawn size: {coarse_commands:?}"
        );
        let painted = coarse_paint[0];
        assert_eq!(
            (painted.min_x(), painted.min_y()),
            (
                (COARSE_TARGET - painted.width()) / 2.0,
                (COARSE_TARGET - painted.height()) / 2.0
            ),
            "the drawn box is centred in the coarse hit box"
        );
    }

    #[test]
    fn the_close_target_is_20px_and_an_icon_target_is_24px() {
        let app = AppContext::default();
        let size = |element: &mut Box<dyn Element>| {
            element.layout(
                SizeConstraint::loose(vec2f(400.0, 200.0)),
                &mut LayoutContext::default(),
                &app,
            )
        };
        let mut close: Box<dyn Element> = Box::new(
            TopbarButton::new(Icon::new("x").with_size(10.0).finish())
                .with_size(CLOSE_TARGET)
                .with_target_class(TargetClass::Close),
        );
        assert_eq!(size(&mut close), vec2f(CLOSE_TARGET, CLOSE_TARGET));

        let mut icon: Box<dyn Element> = Box::new(
            TopbarButton::new(Icon::new("x").with_size(10.0).finish())
                .with_size(CLOSE_TARGET)
                .with_target_class(TargetClass::Icon),
        );
        assert_eq!(size(&mut icon), vec2f(ICON_TARGET, ICON_TARGET));
    }

    #[test]
    fn a_focused_field_paints_one_3px_ring_not_two() {
        let app = AppContext::default();
        let mut field: Box<dyn Element> =
            Box::new(TextInput::new().with_value("hi").with_focused(true));
        let commands = render(&mut field, &app);
        let rings = focus_rings(&commands, &app);
        assert_eq!(rings.len(), 1, "the field paints one ring: {rings:?}");
        assert_eq!(rings[0].1, 3.0, "the field ring is 3px: {rings:?}");
        // The plain 1px border stays `Border`, so only the ring is blue.
        let border = app.theme.color(ColorToken::Border);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::StrokeRect { color, width, .. } if *color == border && *width == 1.0
            )),
            "the field keeps its 1px border under the ring: {commands:?}"
        );
    }

    #[test]
    fn a_focused_button_inside_a_focused_field_paints_only_its_own_ring() {
        let app = AppContext::default();
        let field: Box<dyn Element> =
            Box::new(TextInput::new().with_value("hi").with_focused(true));
        let button = button(true);
        let mut row: Box<dyn Element> = Flex::row()
            .with_spacing(8.0)
            .with_child(field)
            .with_child(button)
            .finish();
        let commands = render(&mut row, &app);
        let rings = focus_rings(&commands, &app);
        assert_eq!(rings.len(), 2, "one ring per focused control: {rings:?}");
        let widths: Vec<f32> = rings.iter().map(|(_, width, _)| *width).collect();
        assert!(
            widths.contains(&3.0) && widths.contains(&2.0),
            "the field's 3px ring and the button's 2px ring, nothing else: {rings:?}"
        );
    }
}
