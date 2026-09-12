use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    caret, AppContext, CaretShape, Container, CrossAxisAlignment, Element, EventContext, Flex,
    LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::{PointF, Vector2F};
use crate::theme::ColorToken;
use crate::vim::{Clipboard, VimBuffer, VimOutcome, VimState};

pub struct TextArea {
    value: String,
    placeholder: String,
    focused: bool,
    min_height: f32,
    masked: bool,
    /// Where the insertion beam sits, as a character index into the value.
    /// Shared with the host when the host owns the state (the composer does:
    /// the element is rebuilt every frame, so an index kept here would reset to
    /// the start on every one of them).
    caret: Option<Rc<RefCell<usize>>>,
    /// The index used when no shared one was handed over.
    local_caret: usize,
    /// Whether a press outside the field drops its focus. The terminal pane's
    /// rich input keeps it: it is the only place a command can be typed, so a
    /// click on the output above it must not take the keyboard away.
    blur_on_outside_click: bool,
    /// Modal (vim) editing, when the host turned it on. The state lives beside
    /// the host's own, because this element is rebuilt every frame.
    vim: Option<Rc<RefCell<VimState>>>,
    /// The system clipboard the engine's `"+`/`"*` registers reach, when the
    /// host supplies one.
    clipboard: Option<Rc<RefCell<dyn Clipboard>>>,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_submit: Option<Rc<RefCell<dyn FnMut(ModifiersState) + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    root: Option<Box<dyn Element>>,
}

impl TextArea {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            placeholder: String::new(),
            focused: false,
            min_height: 80.0,
            masked: false,
            caret: None,
            local_caret: 0,
            blur_on_outside_click: true,
            vim: None,
            clipboard: None,
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

    pub fn with_min_height(mut self, height: f32) -> Self {
        self.min_height = height;
        self
    }

    /// Share the insertion point with the host, so the beam survives the
    /// per-frame rebuild of the element tree.
    pub fn with_caret(mut self, caret: Rc<RefCell<usize>>) -> Self {
        self.caret = Some(caret);
        self
    }

    /// Render the typed value as a masked (bullet) string while keeping the real
    /// value intact — used for credential/secret fields.
    pub fn with_masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Turn on modal (vim) editing for this field: the host's [`VimState`] owns
    /// the mode, so it survives the per-frame rebuild.
    pub fn with_vim(mut self, vim: Rc<RefCell<VimState>>) -> Self {
        self.vim = Some(vim);
        self
    }

    /// The system clipboard the `"+`/`"*` registers read and write.
    pub fn with_clipboard(mut self, clipboard: Rc<RefCell<dyn Clipboard>>) -> Self {
        self.clipboard = Some(clipboard);
        self
    }

    /// [`Self::with_vim`] for a host that already holds an `Option`, so a
    /// composer can pass its own setting straight through.
    pub fn with_vim_opt(mut self, vim: Option<Rc<RefCell<VimState>>>) -> Self {
        self.vim = vim;
        self
    }

    /// [`Self::with_clipboard`] for a host that already holds an `Option`.
    pub fn with_clipboard_opt(mut self, clipboard: Option<Rc<RefCell<dyn Clipboard>>>) -> Self {
        self.clipboard = clipboard;
        self
    }

    /// Whether a press outside the field blurs it (the default). A host that
    /// owns the keyboard with this field — the terminal pane's rich input —
    /// turns it off, so a click elsewhere in the pane leaves the caret in place.
    pub fn with_blur_on_outside_click(mut self, blur: bool) -> Self {
        self.blur_on_outside_click = blur;
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// When set, pressing Enter fires the callback instead of inserting a
    /// newline. The callback receives the modifiers of the Enter key event so
    /// the host can tell a plain Enter (terminal command) from Cmd/Ctrl+Enter
    /// (submit to agent).
    pub fn with_on_submit<F: FnMut(ModifiersState) + 'static>(mut self, callback: F) -> Self {
        self.on_submit = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn set_focused(&mut self, focused: bool) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if let Some(cb) = self.on_focus_change.as_ref() {
            (cb.borrow_mut())(focused);
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        let empty = self.value.is_empty();
        let display = if self.masked && !empty {
            "•".repeat(self.value.chars().count())
        } else if empty {
            self.placeholder.clone()
        } else {
            self.value.clone()
        };
        // An empty field draws its placeholder in the muted colour, so a guide
        // never reads as text the user typed.
        let color = if empty {
            ColorToken::Muted
        } else {
            ColorToken::Text
        };

        // Transparent: no background/border or internal padding so the textarea
        // reads as part of the surrounding rich-input bar.
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        if !self.focused {
            row = row.with_child(Text::new(display).with_theme_color(color, app).finish());
        } else {
            // The beam is its own slot in the row, with the text it precedes on
            // one side and the rest on the other, so it lands between the two
            // characters it sits between instead of always trailing the line.
            // A placeholder is only a guide, so the beam leads it: that is where
            // the user's own text starts.
            let at = if empty { 0 } else { self.caret_index() };
            let len = display.chars().count();
            // Modal editing draws the caret in the mode's own shape and paints
            // the character a visual selection covers. The placeholder is never
            // a character the caret covers.
            let (shape, selection) = match self.vim.as_ref() {
                Some(vim) => {
                    let vim = vim.borrow();
                    (caret_shape(vim.mode()), vim.selection(at, len))
                }
                None => (CaretShape::Bar, None),
            };
            let under = if empty { None } else { display.chars().nth(at) };
            match selection {
                Some(selected) => {
                    row = self.selection_row(app, row, &display, at, shape, under, color, selected)
                }
                None => {
                    let (before, after) = split_at_char(&display, at);
                    row = row
                        .with_child(Text::new(before).with_theme_color(color, app).finish())
                        .with_child(caret(app, shape, under))
                        .with_child(Text::new(after).with_theme_color(color, app).finish());
                }
            }
        }
        self.root = Some(Container::new(row.finish()).finish());
    }

    /// The focused row while a visual selection is live: the text is cut at the
    /// selection's edges and the caret's own position, the covered segments get
    /// the selection background, and the caret keeps its place between them.
    #[allow(clippy::too_many_arguments)]
    fn selection_row(
        &self,
        app: &AppContext,
        mut row: Flex,
        display: &str,
        at: usize,
        shape: CaretShape,
        under: Option<char>,
        color: ColorToken,
        selected: std::ops::Range<usize>,
    ) -> Flex {
        let len = display.chars().count();
        let mut bounds = vec![0, selected.start.min(len), at.min(len), selected.end.min(len), len];
        bounds.sort_unstable();
        bounds.dedup();
        for pair in bounds.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            if start == at {
                row = row.with_child(caret(app, shape, under));
            }
            if end > start {
                let segment = split_at_char(display, start).1;
                let segment = split_at_char(&segment, end - start).0;
                let text = Text::new(segment).with_theme_color(color, app).finish();
                let text = if start >= selected.start && end <= selected.end {
                    Container::new(text)
                        .with_background(crate::elements::Fill::Solid(app.theme.color(ColorToken::Selected)))
                        .finish()
                } else {
                    text
                };
                row = row.with_child(text);
            }
        }
        if bounds.last() == Some(&at) {
            row = row.with_child(caret(app, shape, under));
        }
        row
    }

    /// The insertion point, clamped to the value's length.
    fn caret_index(&self) -> usize {
        let index = match self.caret.as_ref() {
            Some(caret) => *caret.borrow(),
            None => self.local_caret,
        };
        index.min(self.value.chars().count())
    }

    fn set_caret_index(&mut self, index: usize) {
        let index = index.min(self.value.chars().count());
        match self.caret.as_ref() {
            Some(caret) => *caret.borrow_mut() = index,
            None => self.local_caret = index,
        }
    }

    /// Hand one key to the modal engine, writing its buffer back when the engine
    /// moved the insertion point or edited the text. The engine owns the mode
    /// and the registers; the field owns the text and the caret.
    fn vim_key(
        &mut self,
        vim: &Rc<RefCell<VimState>>,
        key: &str,
        modifiers: &ModifiersState,
    ) -> VimOutcome {
        let mut buffer = VimBuffer::new(self.value.clone(), self.caret_index());
        // The clipboard handle is cloned out of `self` first: the engine holds
        // it across the call, and the field's own value/caret are written back
        // right after.
        let clipboard = self.clipboard.clone();
        let mut clipboard_borrow: Option<std::cell::RefMut<'_, dyn Clipboard>> = match clipboard.as_ref() {
            Some(cell) => Some(cell.borrow_mut()),
            None => None,
        };
        let outcome = match clipboard_borrow.as_mut() {
            Some(clip) => vim
                .borrow_mut()
                .handle(key, modifiers, &mut buffer, Some(&mut **clip)),
            None => vim.borrow_mut().handle(key, modifiers, &mut buffer, None),
        };
        match outcome {
            VimOutcome::Moved => self.set_caret_index(buffer.caret),
            VimOutcome::Edited => {
                self.value = buffer.text;
                self.set_caret_index(buffer.caret);
            }
            VimOutcome::Passthrough | VimOutcome::Consumed => {}
        }
        outcome
    }
}

/// The caret shape a vim mode draws: a bar where typing inserts, a block where
/// a command reads the character under the caret, an underline for replace.
fn caret_shape(mode: crate::vim::VimMode) -> CaretShape {
    use crate::vim::VimMode;
    match mode {
        VimMode::Insert => CaretShape::Bar,
        VimMode::Normal | VimMode::Visual(_) => CaretShape::Block,
        VimMode::Replace => CaretShape::Underline,
    }
}

/// The byte offset of the `index`-th character, or the end of the string.
fn char_offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

/// Split `text` around its `index`-th character, for the beam to sit between.
fn split_at_char(text: &str, index: usize) -> (String, String) {
    let offset = char_offset(text, index);
    (text[..offset].to_string(), text[offset..].to_string())
}

fn insert_char(text: &mut String, index: usize, ch: char) {
    let offset = char_offset(text, index);
    text.insert(offset, ch);
}

/// Remove the character that begins at `index`, if there is one.
fn remove_char(text: &mut String, index: usize) {
    let offset = char_offset(text, index);
    if let Some(ch) = text[offset..].chars().next() {
        text.replace_range(offset..offset + ch.len_utf8(), "");
    }
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for TextArea {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // Rebuild every layout so externally-driven value/focus changes render.
        self.rebuild(app);
        let mut inner_constraint = constraint;
        inner_constraint.min.y = inner_constraint.min.y.max(self.min_height);
        let mut size = self
            .root
            .as_mut()
            .unwrap()
            .layout(inner_constraint, ctx, app);
        // The root is transparent, so keep the reported/hit area at least the
        // requested minimum: the whole composer bar row stays clickable even
        // when the displayed text is empty.
        size.x = size.x.max(inner_constraint.min.x);
        size.y = size.y.max(inner_constraint.min.y);
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
                        self.set_focused(true);
                        return true;
                    }
                    if self.blur_on_outside_click {
                        self.set_focused(false);
                    }
                }
                false
            }
            DispatchedEvent::KeyDown { key, modifiers } => {
                if !self.focused {
                    return false;
                }
                if let Some(vim) = self.vim.clone() {
                    match self.vim_key(&vim, key, modifiers) {
                        VimOutcome::Passthrough => {}
                        VimOutcome::Consumed => return true,
                        VimOutcome::Moved => return true,
                        VimOutcome::Edited => {
                            if let Some(cb) = self.on_change.as_ref() {
                                (cb.borrow_mut())(self.value.clone());
                            }
                            return true;
                        }
                    }
                }
                let caret = self.caret_index();
                let length = self.value.chars().count();
                match key.as_str() {
                    // Moving the beam changes nothing about the value, so it is
                    // consumed without reporting a change.
                    "ArrowLeft" => {
                        self.set_caret_index(caret.saturating_sub(1));
                        return true;
                    }
                    "ArrowRight" => {
                        self.set_caret_index(caret + 1);
                        return true;
                    }
                    "Home" => {
                        self.set_caret_index(0);
                        return true;
                    }
                    "End" => {
                        self.set_caret_index(length);
                        return true;
                    }
                    "Backspace" => {
                        if caret > 0 {
                            remove_char(&mut self.value, caret - 1);
                            self.set_caret_index(caret - 1);
                        }
                    }
                    "Delete" => {
                        if caret < length {
                            remove_char(&mut self.value, caret);
                        }
                    }
                    "Enter" => {
                        if let Some(cb) = self.on_submit.as_ref() {
                            (cb.borrow_mut())(*modifiers);
                            return true;
                        }
                        insert_char(&mut self.value, caret, '\n');
                        self.set_caret_index(caret + 1);
                    }
                    // One typed character is inserted where the beam is, not
                    // appended: a named key the editor does not model is left
                    // to the rest of the app.
                    _ => {
                        if key.len() != 1 {
                            return false;
                        }
                        insert_char(&mut self.value, caret, key.chars().next().unwrap());
                        self.set_caret_index(caret + 1);
                    }
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

    fn app() -> AppContext {
        AppContext::default()
    }

    #[test]
    fn enter_fires_submit_instead_of_newline() {
        let app = app();
        let submitted = Rc::new(RefCell::new(0));
        let submitted_clone = submitted.clone();
        let mut area = TextArea::new()
            .with_value("hello")
            .with_focused(true)
            .with_on_submit(move |_mods| *submitted_clone.borrow_mut() += 1);
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let handled = area.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Enter".to_string(),
                modifiers: Default::default(),
            },
            &mut event_ctx,
            &app,
        );
        assert!(handled, "Enter should be handled when focused");
        assert_eq!(*submitted.borrow(), 1, "submit callback should fire");
        assert_eq!(
            area.value(),
            "hello",
            "Enter should not insert a newline when submitting"
        );
    }

    /// A placeholder is a guide, not text: the beam leads it, and the first
    /// character the user types opens the line at the start.
    #[test]
    fn the_beam_leads_a_placeholder_and_typing_starts_the_line() {
        let app = app();
        let mut area = TextArea::new()
            .with_placeholder("Ask anything...")
            .with_focused(true);
        area.layout(
            SizeConstraint::loose(Vector2F::new(300.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        for key in ["l", "s"] {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            );
        }
        assert_eq!(
            area.value(),
            "ls",
            "the guide never becomes part of the value the user typed"
        );
    }

    /// The beam is a position in the value, not a mark at its end: the arrow
    /// keys move it and the next character lands where it sits.
    #[test]
    fn the_beam_moves_with_the_arrows_and_typing_lands_where_it_sits() {
        let app = app();
        let caret = Rc::new(RefCell::new(0));
        let mut area = TextArea::new()
            .with_value("abcd")
            .with_focused(true)
            .with_caret(caret.clone());
        let mut event_ctx = EventContext::default();
        let mut press = |area: &mut TextArea, key: &str| {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            )
        };

        // The beam starts where the host left it, then walks left twice.
        press(&mut area, "End");
        assert_eq!(*caret.borrow(), 4, "End puts the beam after the value");
        press(&mut area, "ArrowLeft");
        press(&mut area, "ArrowLeft");
        assert_eq!(*caret.borrow(), 2, "two lefts move the beam two places");
        press(&mut area, "X");
        assert_eq!(
            area.value(),
            "abXcd",
            "the character lands where the beam sits, not at the end"
        );
        assert_eq!(*caret.borrow(), 3, "the beam advances over what it inserted");

        // Backspace takes the character before the beam, not the last one.
        press(&mut area, "Home");
        press(&mut area, "ArrowRight");
        press(&mut area, "Backspace");
        assert_eq!(area.value(), "bXcd", "Backspace deletes behind the beam");
        assert_eq!(*caret.borrow(), 0, "and the beam stays at the start");
    }

    #[test]
    fn mouse_down_fires_focus_change() {        let app = app();
        let focus_changes = Rc::new(RefCell::new(Vec::new()));
        let changes_clone = focus_changes.clone();
        // The textarea is transparent, so the clickable area is the text
        // extent; a placeholder gives it a real hit box at (10, 10).
        let mut area = TextArea::new()
            .with_placeholder("Type a message")
            .with_on_focus_change(move |focused| {
                changes_clone.borrow_mut().push(focused);
            });
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        area.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        );
        assert_eq!(
            *focus_changes.borrow(),
            vec![true],
            "clicking inside should report gaining focus"
        );

        area.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(500.0, 500.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        );
        assert_eq!(
            *focus_changes.borrow(),
            vec![true, false],
            "clicking outside should report losing focus"
        );
    }

    /// With vim on, Escape leaves insert mode and the next key is a command the
    /// engine runs on the field's own text; `u` puts the edit back.
    #[test]
    fn vim_commands_edit_the_field_and_undo_restores_it() {
        let app = app();
        let vim = Rc::new(RefCell::new(VimState::new()));
        let mut area = TextArea::new()
            .with_value("abc")
            .with_focused(true)
            .with_vim(vim.clone());
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        let mut event_ctx = EventContext::default();
        let mut press = |area: &mut TextArea, key: &str| {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            )
        };

        press(&mut area, "Escape");
        assert_eq!(
            vim.borrow().mode(),
            crate::vim::VimMode::Normal,
            "Escape leaves insert mode"
        );
        assert_eq!(vim.borrow().selection(0, 3), None);

        press(&mut area, "x");
        assert_eq!(area.value(), "bc", "x deletes the character under the caret");
        press(&mut area, "u");
        assert_eq!(area.value(), "abc", "u puts the deleted character back");
    }

    /// The field's own keys still work with vim on: an arrow in insert mode is
    /// the engine's passthrough, and the plain caret handling moves the beam.
    #[test]
    fn arrows_still_move_the_caret_with_vim_on() {
        let app = app();
        let vim = Rc::new(RefCell::new(VimState::new()));
        let caret = Rc::new(RefCell::new(3));
        let mut area = TextArea::new()
            .with_value("abc")
            .with_focused(true)
            .with_caret(caret.clone())
            .with_vim(vim);
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        let mut event_ctx = EventContext::default();
        let handled = area.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "ArrowLeft".to_string(),
                modifiers: Default::default(),
            },
            &mut event_ctx,
            &app,
        );
        assert!(handled, "the arrow is still the field's key");
        assert_eq!(*caret.borrow(), 2, "insert mode keeps the plain arrow move");
    }

    /// A visual selection is the engine's, and the field renders it: the layout
    /// must accept the highlighted run without dropping a character.
    #[test]
    fn a_visual_selection_renders_without_losing_text() {
        let app = app();
        let vim = Rc::new(RefCell::new(VimState::new()));
        let mut area = TextArea::new()
            .with_value("hello")
            .with_focused(true)
            .with_vim(vim.clone());
        let mut event_ctx = EventContext::default();
        let mut press = |area: &mut TextArea, key: &str| {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            )
        };
        press(&mut area, "Escape");
        press(&mut area, "v");
        press(&mut area, "l");
        press(&mut area, "l");
        assert!(
            vim.borrow().selection(2, 5).is_some(),
            "the selection covers the beam's run"
        );
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        assert_eq!(area.value(), "hello", "selection changes no text");
    }
}
