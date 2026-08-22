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

    pub fn value(&self) -> &str {
        &self.value
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
        let text = Text::new(display)
            .with_theme_color(color, app)
            .finish();
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
        if self.root.is_none() {
            self.rebuild(app);
        }
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
                        self.focused = true;
                        return true;
                    }
                    self.focused = false;
                }
                false
            }
            DispatchedEvent::KeyDown { key, shift } => {
                if !self.focused {
                    return false;
                }
                if key == "Backspace" {
                    self.value.pop();
                } else if key == "Enter" {
if *shift {
                        self.value.push('\n');
                    } else {
                        // Unfocused newline; let the parent composer handle send.
                        return false;
                    }
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
