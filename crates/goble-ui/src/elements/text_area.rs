use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Container, EdgeInsets, Element, EventContext, Fill, LayoutContext, PaintContext,
    Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{PointF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct TextArea {
    value: String,
    placeholder: String,
    focused: bool,
    min_height: f32,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_submit: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
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

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// When set, pressing Enter fires the callback instead of inserting a newline.
    pub fn with_on_submit<F: FnMut() + 'static>(mut self, callback: F) -> Self {
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
        let padding = app.theme.spacing_px(SpacingToken::Md);
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
        let text = Text::new(display).with_theme_color(color, app).finish();
        let mut container = Container::new(text)
            .with_padding(EdgeInsets::uniform(padding))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)));
        if self.focused {
            container = container.with_border(app.theme.color(ColorToken::Accent).into());
        } else {
            container = container.with_border(app.theme.color(ColorToken::Border).into());
        }
        self.root = Some(container.finish());
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
        let size = self
            .root
            .as_mut()
            .unwrap()
            .layout(inner_constraint, ctx, app);
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
                    self.set_focused(false);
                }
                false
            }
            DispatchedEvent::KeyDown { key } => {
                if !self.focused {
                    return false;
                }
                if key == "Backspace" {
                    self.value.pop();
                } else if key == "Enter" {
                    if let Some(cb) = self.on_submit.as_ref() {
                        (cb.borrow_mut())();
                        return true;
                    }
                    self.value.push('\n');
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
            .with_on_submit(move || *submitted_clone.borrow_mut() += 1);
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

    #[test]
    fn mouse_down_fires_focus_change() {
        let app = app();
        let focus_changes = Rc::new(RefCell::new(Vec::new()));
        let changes_clone = focus_changes.clone();
        let mut area = TextArea::new().with_on_focus_change(move |focused| {
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
}
