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
    /// The insertion point, shared with the host so it survives the per-frame
    /// rebuild. Without one the field edits at the end of its value.
    caret: Option<Rc<RefCell<usize>>>,
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
            caret: None,
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

    /// Share the insertion point with the host, so the caret survives the
    /// per-frame rebuild of the element tree. Without one the field edits at
    /// the end of its value, which is what it did before it had a caret.
    pub fn with_caret(mut self, caret: Rc<RefCell<usize>>) -> Self {
        self.caret = Some(caret);
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

    /// The insertion point, in characters, clamped to the value: a cell left
    /// past the end by a value shortened elsewhere lands on the end.
    fn caret_index(&self) -> usize {
        let len = self.value.chars().count();
        match self.caret.as_ref() {
            Some(caret) => (*caret.borrow()).min(len),
            None => len,
        }
    }

    fn set_caret(&self, index: usize) {
        if let Some(caret) = self.caret.as_ref() {
            *caret.borrow_mut() = index.min(self.value.chars().count());
        }
    }

    /// Insert `ch` at the character index `index`, which is where the caret
    /// then lands past.
    fn insert_char(&mut self, index: usize, ch: char) {
        let at = self
            .value
            .char_indices()
            .nth(index)
            .map(|(at, _)| at)
            .unwrap_or(self.value.len());
        self.value.insert(at, ch);
    }

    /// Remove the character at `index`, reporting whether there was one.
    fn remove_char(&mut self, index: usize) -> bool {
        let Some((start, ch)) = self.value.char_indices().nth(index) else {
            return false;
        };
        self.value.replace_range(start..start + ch.len_utf8(), "");
        true
    }

    fn notify_change(&self) {
        if let Some(cb) = self.on_change.as_ref() {
            (cb.borrow_mut())(self.value.clone());
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        let token = if self.compact {
            SpacingToken::Xs
        } else {
            SpacingToken::Md
        };
        let padding = app.theme.spacing_px(token);
        let gap = app.theme.spacing_px(SpacingToken::Sm);
        let empty = self.value.is_empty();
        let icon = Icon::new("search")
            .with_theme_color(ColorToken::Muted, app)
            .finish();
        // The run and the beam that belongs to it, packed without the row's own
        // spacing: a beam asks the row for no room, so it is only on the
        // character it belongs to when nothing stands between them. The group
        // takes the rest of the row, so the run is measured at the width the
        // field gives it whether or not the beam is in front of it. While the
        // field is empty the beam leads the placeholder — the guide stands
        // where the user's own text starts — and once there is a value it is
        // drawn between the run before the caret and the run after it, so the
        // beam is at the insertion point and the two runs stay adjacent.
        let mut run = crate::elements::Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        if empty {
            if self.focused {
                run = run.with_child(caret_beam(app));
            }
            run = run.with_child(text_run(self.placeholder.clone(), ColorToken::Muted, app));
        } else if self.focused {
            let chars: Vec<char> = self.value.chars().collect();
            let (before, after) = chars.split_at(self.caret_index());
            if !before.is_empty() {
                run = run.with_child(text_run(before.iter().collect(), ColorToken::Text, app));
            }
            run = run.with_child(caret_beam(app));
            if !after.is_empty() {
                run = run.with_child(text_run(after.iter().collect(), ColorToken::Text, app));
            }
        } else {
            run = run.with_child(text_run(self.value.clone(), ColorToken::Text, app));
        }
        // Fill the available width so the box has equal margins on both sides
        // when placed in a stretched column (e.g. the sidebar).
        let mut row = crate::elements::Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(gap);
        if self.icon {
            row = row.with_child(icon);
        }
        row = row.with_child(run.finish());
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
                let len = self.value.chars().count();
                let caret = self.caret_index();
                match key.as_str() {
                    "ArrowLeft" => {
                        self.set_caret(caret.saturating_sub(1));
                        true
                    }
                    "ArrowRight" => {
                        self.set_caret((caret + 1).min(len));
                        true
                    }
                    "Home" => {
                        self.set_caret(0);
                        true
                    }
                    "End" => {
                        self.set_caret(len);
                        true
                    }
                    "Enter" => true,
                    "Backspace" => {
                        if caret == 0 || !self.remove_char(caret - 1) {
                            return true;
                        }
                        self.set_caret(caret - 1);
                        self.notify_change();
                        true
                    }
                    "Delete" => {
                        if !self.remove_char(caret) {
                            return true;
                        }
                        self.notify_change();
                        true
                    }
                    _ => {
                        if key.len() != 1 {
                            return false;
                        }
                        let ch = key.chars().next().expect("a one-byte key is a character");
                        self.insert_char(caret, ch);
                        self.set_caret(caret + 1);
                        self.notify_change();
                        true
                    }
                }
            }
            _ => false,
        }
    }
}

/// One run of the field's text. The value is drawn in two of these with the
/// caret's beam between them; the placeholder is drawn in one.
fn text_run(text: String, color: ColorToken, app: &AppContext) -> Box<dyn Element> {
    Text::new(text)
        .with_theme_color(color, app)
        .with_max_lines(1)
        .finish()
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

    /// The field the conversations sidebar builds: compact, no magnifier, and a
    /// placeholder standing in for a value nobody has typed yet.
    fn sidebar_field(focused: bool) -> Box<dyn Element> {
        Box::new(
            SearchInput::new()
                .with_placeholder("Search")
                .with_compact(true)
                .with_icon(false)
                .with_extra_height(3.0)
                .with_focused(focused),
        )
    }

    /// A focused field holding `value` with its insertion point shared at
    /// `caret`, laid out and painted once, the way a frame leaves it before a
    /// key arrives.
    fn field(value: &str, caret: usize) -> (SearchInput, Rc<RefCell<usize>>) {
        let app = AppContext::default();
        let cell = Rc::new(RefCell::new(caret));
        let mut input = SearchInput::new()
            .with_value(value)
            .with_placeholder("Search")
            .with_focused(true)
            .with_caret(Rc::clone(&cell));
        input.layout(
            SizeConstraint::loose(vec2f(240.0, 60.0)),
            &mut LayoutContext::default(),
            &app,
        );
        input.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        (input, cell)
    }

    /// The frame after `input` took keys: the app builds a fresh element over
    /// the value and the caret the last one left, and so does this.
    fn next_frame(input: &SearchInput, caret: &Rc<RefCell<usize>>) -> Vec<RenderCommand> {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(
            SearchInput::new()
                .with_value(input.value())
                .with_placeholder("Search")
                .with_focused(true)
                .with_caret(Rc::clone(caret)),
        );
        render_element(&mut element, vec2f(240.0, 200.0), &app)
    }

    fn press(input: &mut SearchInput, key: &str) -> bool {
        input.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers: Default::default(),
            },
            &mut EventContext::default(),
            &AppContext::default(),
        )
    }

    /// A focused, empty search field draws its beam over the first character of
    /// the placeholder, the way the rich input does: the guide stands where the
    /// user's own text starts, and the beam asks the row for no room, so the
    /// guide keeps its place and its muted colour under it.
    #[test]
    fn an_empty_focused_field_puts_its_beam_on_the_placeholder() {
        let app = AppContext::default();
        let muted = app.theme.color(ColorToken::Muted);

        for (what, guide_text, mut field) in [
            ("the sidebar field", "Search", sidebar_field(true)),
            (
                "the field with its magnifier",
                "Search in files",
                Box::new(
                    SearchInput::new()
                        .with_placeholder("Search in files")
                        .with_focused(true),
                ) as Box<dyn Element>,
            ),
        ] {
            let commands = render_element(&mut field, vec2f(240.0, 200.0), &app);
            let beam = caret_rect(&commands, &app);
            let RenderCommand::DrawText { origin, color, .. } = drawn_run(&commands, guide_text)
            else {
                unreachable!()
            };
            assert_eq!(
                *color, muted,
                "{what} keeps its placeholder muted: {commands:?}"
            );
            assert!(
                (beam.min_x() - origin.x).abs() < 0.5,
                "{what} draws its beam on the first character of {guide_text:?}, got x={} against {}",
                beam.min_x(),
                origin.x
            );
        }
    }

    /// A field with a value in it is unchanged in shape: the whole value is
    /// drawn and the beam trails it at the pen, where the next character lands.
    #[test]
    fn a_field_with_a_value_keeps_its_beam_at_the_end_of_the_text() {
        let app = AppContext::default();
        let mut field: Box<dyn Element> = Box::new(
            SearchInput::new()
                .with_value("hi")
                .with_placeholder("Search")
                .with_focused(true),
        );
        let commands = render_element(&mut field, vec2f(240.0, 200.0), &app);
        let beam = caret_rect(&commands, &app);

        let value = drawn_run(&commands, "hi");
        let RenderCommand::DrawText { origin, .. } = value else {
            unreachable!()
        };
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text == "Search"
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

    /// The index of the command that draws `text`, for the order the frame
    /// painted the two runs and the beam in.
    fn run_index(commands: &[RenderCommand], text: &str) -> usize {
        commands
            .iter()
            .position(|command| {
                matches!(command, RenderCommand::DrawText { text: drawn, .. } if drawn == text)
            })
            .unwrap_or_else(|| panic!("the run {text:?} is drawn: {commands:?}"))
    }

    /// Where the run `text` starts and how wide it is: the pen the next piece
    /// of the frame is laid out at.
    fn run_box(commands: &[RenderCommand], text: &str) -> (f32, f32) {
        let run = drawn_run(commands, text);
        let RenderCommand::DrawText { origin, .. } = run else {
            unreachable!()
        };
        (origin.x, run_width(run))
    }

    /// The arrows move the insertion point one character and the beam with it:
    /// left lands before the last character, right walks back past it.
    #[test]
    fn the_arrows_move_the_beam_and_the_caret() {
        let (mut input, caret) = field("abc", 3);
        assert!(press(&mut input, "ArrowLeft"));
        assert_eq!(*caret.borrow(), 2, "the caret stepped back one character");

        let app = AppContext::default();
        let commands = next_frame(&input, &caret);
        let beam = caret_rect(&commands, &app);
        let (before_x, before_width) = run_box(&commands, "ab");
        let (after_x, _) = run_box(&commands, "c");
        assert!(
            (beam.min_x() - (before_x + before_width)).abs() < 0.5,
            "the beam stands after the run before the caret, got x={} against {}",
            beam.min_x(),
            before_x + before_width
        );
        assert!(
            (after_x - beam.min_x()).abs() < 0.5,
            "and the run after the caret starts under it, got x={after_x}",
        );

        assert!(press(&mut input, "ArrowRight"));
        assert_eq!(*caret.borrow(), 3, "the caret walked back to the end");
    }

    /// `Home` and `End` jump the caret to the value's own ends, where the beam
    /// leads the whole value or trails it at the pen.
    #[test]
    fn home_and_end_jump_to_the_ends_of_the_value() {
        let app = AppContext::default();
        let (mut input, caret) = field("abc", 1);

        assert!(press(&mut input, "Home"));
        assert_eq!(*caret.borrow(), 0);
        let commands = next_frame(&input, &caret);
        let beam = caret_rect(&commands, &app);
        let (value_x, _) = run_box(&commands, "abc");
        assert!(
            (beam.min_x() - value_x).abs() < 0.5,
            "home puts the beam before the value, got x={} against {value_x}",
            beam.min_x()
        );

        assert!(press(&mut input, "End"));
        assert_eq!(*caret.borrow(), 3);
        let commands = next_frame(&input, &caret);
        let beam = caret_rect(&commands, &app);
        let (value_x, value_width) = run_box(&commands, "abc");
        assert!(
            (beam.min_x() - (value_x + value_width)).abs() < 0.5,
            "end puts it back at the pen, got x={} against {}",
            beam.min_x(),
            value_x + value_width
        );
    }

    /// The beam is painted between the two runs, with both adjacent to it: a
    /// gap on either side would leave the bar off the character it marks.
    #[test]
    fn the_beam_is_painted_between_the_two_runs() {
        let app = AppContext::default();
        let (input, caret) = field("abcd", 2);
        let commands = next_frame(&input, &caret);
        let beam = caret_rect(&commands, &app);
        let (before_x, before_width) = run_box(&commands, "ab");
        let (after_x, _) = run_box(&commands, "cd");

        assert!(
            run_index(&commands, "ab") < run_index(&commands, "cd"),
            "the run before the caret is painted first: {commands:?}"
        );
        assert!(
            (beam.min_x() - (before_x + before_width)).abs() < 0.5,
            "the beam touches the run before it, got x={} against {}",
            beam.min_x(),
            before_x + before_width
        );
        assert!(
            (after_x - beam.min_x()).abs() < 0.5,
            "and the run after it starts where the beam does, got x={after_x}",
        );
    }

    /// A character typed with the caret in the middle lands there, not at the
    /// end, and the field reports the value it now holds.
    #[test]
    fn a_character_typed_in_the_middle_is_inserted_there() {
        let changed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&changed);
        let cell = Rc::new(RefCell::new(1));
        let mut input = SearchInput::new()
            .with_value("ac")
            .with_focused(true)
            .with_caret(Rc::clone(&cell))
            .with_on_change(move |value: String| sink.borrow_mut().push(value));

        assert!(press(&mut input, "b"));
        assert_eq!(input.value(), "abc");
        assert_eq!(*cell.borrow(), 2, "the caret lands past what it typed");
        assert_eq!(
            *changed.borrow(),
            vec!["abc".to_string()],
            "the edit is reported once, with the new value"
        );
    }

    /// `Backspace` takes the character before the caret and `Delete` the one
    /// at it, each leaving the caret where the edit began.
    #[test]
    fn backspace_and_delete_remove_on_the_correct_side() {
        let (mut input, caret) = field("abc", 1);

        assert!(press(&mut input, "Backspace"));
        assert_eq!(
            input.value(),
            "bc",
            "backspace took the character before the caret"
        );
        assert_eq!(*caret.borrow(), 0);

        assert!(press(&mut input, "Delete"));
        assert_eq!(input.value(), "c", "delete took the character at the caret");
        assert_eq!(*caret.borrow(), 0);
    }

    /// The arrows stop at the value's ends instead of running past them, and a
    /// move is not an edit.
    #[test]
    fn the_arrows_are_no_ops_at_the_ends() {
        let (mut input, caret) = field("ab", 0);

        assert!(press(&mut input, "ArrowLeft"));
        assert_eq!(*caret.borrow(), 0);

        assert!(press(&mut input, "End"));
        assert_eq!(*caret.borrow(), 2);

        assert!(press(&mut input, "ArrowRight"));
        assert_eq!(*caret.borrow(), 2);
        assert_eq!(input.value(), "ab");
    }

    /// A caret index left past the end by a value shortened elsewhere clamps to
    /// the end: the beam draws there and typing extends the value.
    #[test]
    fn a_caret_past_the_end_clamps_to_it() {
        let app = AppContext::default();
        let (mut input, caret) = field("ab", 5);
        let commands = next_frame(&input, &caret);
        let beam = caret_rect(&commands, &app);
        let (value_x, value_width) = run_box(&commands, "ab");
        assert!(
            (beam.min_x() - (value_x + value_width)).abs() < 0.5,
            "the beam lands at the end of the shortened value, got x={}",
            beam.min_x()
        );

        assert!(press(&mut input, "c"));
        assert_eq!(input.value(), "abc", "and typing extends it from there");
        assert_eq!(*caret.borrow(), 3);
    }
}
