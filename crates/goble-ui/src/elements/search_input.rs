use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    caret_beam, AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Fill,
    Icon, LayoutContext, MainAxisSize, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{PointF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct SearchInput {
    value: String,
    placeholder: String,
    focused: bool,
    compact: bool,
    icon: bool,
    extra_height: f32,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    root: Option<Box<dyn Element>>,
}

impl SearchInput {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            placeholder: "Search...".to_string(),
            focused: false,
            compact: false,
            icon: true,
            extra_height: 0.0,
            on_change: None,
            on_focus_change: None,
            size: None,
            origin: None,
            root: None,
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

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the initial focus state. Useful when the tree is rebuilt every frame
    /// from app state (e.g. hot-reload dev loops) so focus survives a rebuild.
    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Use a tighter box for a sidebar: less padding around the row.
    pub fn with_compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Show or hide the leading magnifier. A sidebar search reads fine as a
    /// plain field, without the glyph taking the row's first slot.
    pub fn with_icon(mut self, icon: bool) -> Self {
        self.icon = icon;
        self
    }

    /// Grow the box by `px` above and below its content, so a short row still
    /// has a comfortable click target.
    pub fn with_extra_height(mut self, px: f32) -> Self {
        self.extra_height = px;
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn rebuild(&mut self, app: &AppContext) {
        let token = if self.compact {
            SpacingToken::Xs
        } else {
            SpacingToken::Md
        };
        let padding = app.theme.spacing_px(token);
        let gap = app.theme.spacing_px(SpacingToken::Sm);
        let display = if self.value.is_empty() && !self.placeholder.is_empty() {
            self.placeholder.clone()
        } else {
            self.value.clone()
        };
        let color = if self.value.is_empty() {
            ColorToken::Muted
        } else {
            ColorToken::Text
        };
        let icon = Icon::new("search")
            .with_theme_color(ColorToken::Muted, app)
            .finish();
        let text = Text::new(display)
            .with_theme_color(color, app)
            .with_max_lines(1)
            .finish();
        // Fill the available width so the box has equal margins on both sides
        // when placed in a stretched column (e.g. the sidebar).
        let mut row = crate::elements::Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(gap);
        if self.icon {
            row = row.with_child(icon);
        }
        row = row.with_child(text);
        if self.focused {
            row = row.with_child(caret_beam(app));
        }
        let row = row.finish();
        let v_pad = padding + self.extra_height;
        let mut container = Container::new(row)
            .with_padding(EdgeInsets::new(padding, v_pad, padding, v_pad))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)));
        if self.focused {
            container = container.with_border(app.theme.color(ColorToken::Focus).into());
        } else {
            container = container.with_border(app.theme.color(ColorToken::Border).into());
        }
        self.root = Some(container.finish());
    }
}

impl Default for SearchInput {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for SearchInput {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        if self.root.is_none() {
            self.rebuild(app);
        }
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
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
            DispatchedEvent::MouseDown { position, .. } => {
                if let Some(bounds) = self.bounds() {
                    if bounds.contains(PointF::new(position.x, position.y)) {
                        let was_focused = self.focused;
                        self.focused = true;
                        if !was_focused {
                            if let Some(cb) = self.on_focus_change.as_ref() {
                                (cb.borrow_mut())(true);
                            }
                        }
                        return true;
                    }
                    if self.focused {
                        self.focused = false;
                        if let Some(cb) = self.on_focus_change.as_ref() {
                            (cb.borrow_mut())(false);
                        }
                    }
                }
                false
            }
            DispatchedEvent::KeyDown { key, .. } => {
                if !self.focused {
                    return false;
                }
                if key == "Backspace" {
                    self.value.pop();
                } else if key == "Enter" {
                    return true;
                } else if key.len() == 1 {
                    self.value.push_str(key);
                } else {
                    return false;
                }
                if let Some(cb) = self.on_change.as_ref() {
                    (cb.borrow_mut())(self.value.clone());
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    /// Renders the input at `extra` px of extra height, returning its size and
    /// the commands it painted.
    fn render(extra: f32, icon: bool) -> (Vector2F, Vec<RenderCommand>) {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(
            SearchInput::new()
                .with_placeholder("Search")
                .with_compact(true)
                .with_icon(icon)
                .with_extra_height(extra),
        );
        let commands = render_element(&mut element, vec2f(240.0, 200.0), &app);
        (element.size().expect("the field lays out"), commands)
    }

    #[test]
    fn search_input_accepts_text() {
        let app = AppContext::default();
        let mut input = SearchInput::new();
        input.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        input.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        assert!(input.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(5.0, 5.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        ));
        assert!(input.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "h".to_string(),
                modifiers: Default::default(),
            },
            &mut event_ctx,
            &app,
        ));
        assert_eq!(input.value(), "h");
    }

    /// The sidebar's field, as the sidebar configures it: square, without the
    /// magnifier, and a few pixels taller than the text it holds.
    #[test]
    fn the_sidebar_search_is_a_taller_square_field_without_a_magnifier() {
        let (plain, commands) = render(0.0, false);
        let (taller, _) = render(3.0, false);

        assert_eq!(
            taller.y - plain.y,
            6.0,
            "the extra height is added above and below the text"
        );
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "search"
            )),
            "the sidebar search draws no magnifier"
        );
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect {
                    corner_radius,
                    ..
                } if *corner_radius > 0.0
            )),
            "the sidebar search is square"
        );
    }

    /// The magnifier is still the field's default, so the flag above is a real
    /// switch and not a no-op.
    #[test]
    fn the_search_field_keeps_its_magnifier_by_default() {
        let (_, commands) = render(0.0, true);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "search"
            )),
            "a plain search field keeps its magnifier"
        );
    }

    /// The sidebar field's focused state is the focus blue, ring and beam
    /// alike; blurred, neither is drawn and the ring is the plain border.
    #[test]
    fn the_focused_field_paints_its_caret_and_ring_in_the_focus_blue() {
        use crate::elements::caret::{CARET_HEIGHT, CARET_WIDTH};
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = AppContext::default();
        let focus = app.theme.color(ColorToken::Focus);
        let border = app.theme.color(ColorToken::Border);

        let mut field: Box<dyn Element> =
            Box::new(SearchInput::new().with_value("hi").with_focused(true));
        let commands = render_element(&mut field, vec2f(240.0, 200.0), &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, color, .. }
                    if *color == focus
                        && (rect.width() - CARET_WIDTH).abs() < 0.5
                        && (rect.height() - CARET_HEIGHT).abs() < 0.5
            )),
            "the focused field draws its beam in the focus blue: {commands:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::StrokeRect { color, .. } if *color == focus
            )),
            "the focused field's ring is the focus blue: {commands:?}"
        );

        let mut blurred: Box<dyn Element> = Box::new(SearchInput::new().with_value("hi"));
        let commands = render_element(&mut blurred, vec2f(240.0, 200.0), &app);
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { color, .. } if *color == focus
            )),
            "a blurred field draws no caret: {commands:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::StrokeRect { color, .. } if *color == border
            )),
            "a blurred field keeps the plain border ring: {commands:?}"
        );
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::StrokeRect { color, .. } if *color == focus
            )),
            "a blurred field draws no focus ring: {commands:?}"
        );
    }
}
