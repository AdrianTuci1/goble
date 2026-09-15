use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    caret_beam, AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Fill,
    Flex, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{PointF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct TextInput {
    value: String,
    placeholder: String,
    focused: bool,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_submit: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    root: Option<Box<dyn Element>>,
}

impl TextInput {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            placeholder: String::new(),
            focused: false,
            on_change: None,
            on_focus_change: None,
            on_submit: None,
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

    pub fn with_on_submit<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_submit = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn rebuild(&mut self, app: &AppContext) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let empty = self.value.is_empty();
        let display = if empty && !self.placeholder.is_empty() {
            self.placeholder.clone()
        } else {
            self.value.clone()
        };
        let color = if empty {
            ColorToken::Muted
        } else {
            ColorToken::Text
        };
        let text = Text::new(display)
            .with_theme_color(color, app)
            .with_max_lines(1)
            .finish();
        // A focused field shows its insertion beam in front of the text while
        // the field is empty — over the placeholder's first character, where the
        // user's own text starts — and after it once there is a value. The beam
        // takes no room in the row, so neither position moves the text.
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        if self.focused && empty {
            row = row.with_child(caret_beam(app));
        }
        row = row.with_child(text);
        if self.focused && !empty {
            row = row.with_child(caret_beam(app));
        }
        let mut container = Container::new(row.finish())
            .with_padding(EdgeInsets::uniform(padding))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)));
        if self.focused {
            container = container.with_border(app.theme.color(ColorToken::Focus).into());
        } else {
            container = container.with_border(app.theme.color(ColorToken::Border).into());
        }
        self.root = Some(container.finish());
    }
}

impl Default for TextInput {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for TextInput {
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
                    if let Some(cb) = self.on_submit.as_ref() {
                        (cb.borrow_mut())();
                    }
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

    /// The rect of the insertion beam `commands` paint: the one fill in the
    /// focus blue that is a caret across.
    fn caret_rect(commands: &[RenderCommand], app: &AppContext) -> crate::geometry::RectF {
        use crate::elements::caret::{CARET_HEIGHT, CARET_WIDTH};

        let focus = app.theme.color(ColorToken::Focus);
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == focus
                        && (rect.width() - CARET_WIDTH).abs() < 0.5
                        && (rect.height() - CARET_HEIGHT).abs() < 0.5 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the focused field draws its beam: {commands:?}"))
    }

    /// The command that draws the run `text`, from which the field's own
    /// position for it is read.
    fn drawn_run<'a>(commands: &'a [RenderCommand], text: &str) -> &'a RenderCommand {
        commands
            .iter()
            .find(|command| {
                matches!(command, RenderCommand::DrawText { text: drawn, .. } if drawn == text)
            })
            .unwrap_or_else(|| panic!("the run {text:?} is drawn: {commands:?}"))
    }

    /// The width the row spends on `drawn`: a text box is as wide as its own
    /// ink, measured at the size and the wrap width the command carries.
    fn run_width(drawn: &RenderCommand) -> f32 {
        use crate::platform::text_atlas::measure_text_family;

        let RenderCommand::DrawText {
            text,
            font_size,
            line_height,
            max_width,
            font_weight,
            font_family,
            ..
        } = drawn
        else {
            panic!("a drawn run is a text command: {drawn:?}");
        };
        measure_text_family(
            text,
            *font_size,
            *line_height,
            *max_width,
            *font_weight,
            *font_family,
            false,
        )
        .x
    }

    /// A focused, empty field draws its beam over the first character of the
    /// placeholder, the way the rich input does: the guide stands where the
    /// user's own text starts, and the beam asks the row for no room, so the
    /// guide keeps its place and its muted colour under it.
    #[test]
    fn an_empty_focused_field_puts_its_beam_on_the_placeholder() {
        let app = AppContext::default();
        let mut field: Box<dyn Element> = Box::new(
            TextInput::new()
                .with_placeholder("New group name")
                .with_focused(true),
        );
        let commands = render_element(&mut field, vec2f(200.0, 40.0), &app);
        let beam = caret_rect(&commands, &app);
        let RenderCommand::DrawText { origin, color, .. } = drawn_run(&commands, "New group name")
        else {
            unreachable!()
        };
        assert_eq!(
            *color,
            app.theme.color(ColorToken::Muted),
            "the placeholder is still the muted guide: {commands:?}"
        );
        assert!(
            (beam.min_x() - origin.x).abs() < 0.5,
            "the beam sits on the first character of the placeholder, got x={} against {}",
            beam.min_x(),
            origin.x
        );
    }

    /// A field with a value in it is unchanged in shape: the whole value is
    /// drawn and the beam trails it at the pen, where the next character lands.
    #[test]
    fn a_field_with_a_value_keeps_its_beam_at_the_end_of_the_text() {
        let app = AppContext::default();
        let mut field: Box<dyn Element> = Box::new(
            TextInput::new()
                .with_value("hi")
                .with_placeholder("New group name")
                .with_focused(true),
        );
        let commands = render_element(&mut field, vec2f(200.0, 40.0), &app);
        let beam = caret_rect(&commands, &app);

        let value = drawn_run(&commands, "hi");
        let RenderCommand::DrawText { origin, .. } = value else {
            unreachable!()
        };
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text == "New group name"
            )),
            "a field with a value draws no placeholder: {commands:?}"
        );
        assert!(
            (beam.min_x() - (origin.x + run_width(value))).abs() < 0.5,
            "the beam trails the value at the pen, got x={} against {}",
            beam.min_x(),
            origin.x + run_width(value)
        );
    }

    #[test]
    fn text_input_accepts_text() {
        let app = AppContext::default();
        let mut input = TextInput::new();
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
                key: "a".to_string(),
                modifiers: Default::default(),
            },
            &mut event_ctx,
            &app,
        ));
        assert_eq!(input.value(), "a");
    }

    /// A focused field draws its insertion beam and its ring in the focus blue,
    /// and neither survives the blur: the beam is gone and the ring is back to
    /// the plain border.
    #[test]
    fn the_focused_field_paints_its_caret_and_ring_in_the_focus_blue() {
        use crate::elements::caret::{CARET_HEIGHT, CARET_WIDTH};
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = AppContext::default();
        let focus = app.theme.color(ColorToken::Focus);
        let border = app.theme.color(ColorToken::Border);

        let mut field: Box<dyn Element> =
            Box::new(TextInput::new().with_value("hi").with_focused(true));
        let commands = render_element(&mut field, vec2f(200.0, 40.0), &app);
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

        let mut blurred: Box<dyn Element> = Box::new(TextInput::new().with_value("hi"));
        let commands = render_element(&mut blurred, vec2f(200.0, 40.0), &app);
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
