use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Fill, Flex, Icon,
    LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct DropdownItem {
    pub label: String,
    pub value: String,
}

impl DropdownItem {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

pub struct DropdownMenu {
    label: String,
    items: Vec<DropdownItem>,
    selected_index: Option<usize>,
    open: bool,
    state: InteractiveState,
    on_change: Option<Rc<RefCell<dyn FnMut(Option<usize>) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl DropdownMenu {
    pub fn new(label: impl Into<String>, items: Vec<DropdownItem>) -> Self {
        Self {
            label: label.into(),
            items,
            selected_index: None,
            open: false,
            state: InteractiveState::default(),
            on_change: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected_index(mut self, index: usize) -> Self {
        self.selected_index = Some(index);
        self
    }

    pub fn with_on_change<F: FnMut(Option<usize>) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    pub fn selected_value(&self) -> Option<&str> {
        self.selected_index
            .and_then(|i| self.items.get(i))
            .map(|i| i.value.as_str())
    }

    fn rebuild(&mut self, app: &AppContext) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let gap = app.theme.spacing_px(SpacingToken::Sm);
        let label = if let Some(i) = self.selected_index {
            self.items
                .get(i)
                .map(|it| it.label.clone())
                .unwrap_or_default()
        } else {
            self.label.clone()
        };
        let color = if self.selected_index.is_some() {
            ColorToken::Text
        } else {
            ColorToken::Muted
        };
        let text = Text::new(label).with_theme_color(color, app).finish();
        let icon_name = if self.open {
            "chevron-up"
        } else {
            "chevron-down"
        };
        let icon = Icon::new(icon_name)
            .with_theme_color(ColorToken::Muted, app)
            .finish();
        let mut row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(gap)
            .with_child(text)
            .with_child(icon);

        if self.open {
            let spacing = app.theme.spacing_px(SpacingToken::Sm);
            let mut list = Flex::column()
                .with_main_axis_alignment(MainAxisAlignment::Start)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            for item in &self.items {
                list = list.with_child(
                    Container::new(
                        Text::new(item.label.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_padding(EdgeInsets::uniform(padding / 2.0))
                    .finish(),
                );
            }
            row = row.with_child(list.finish());
        }

        let root = Container::new(row.finish())
            .with_padding(EdgeInsets::uniform(padding))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .finish();
        self.root = Some(root);
    }
}

impl Element for DropdownMenu {
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
        ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        let change = self.on_change.clone();
        let mut toggle = || {
            self.open = !self.open;
            if self.open {
                if self.items.is_empty() {
                    self.selected_index = None;
                } else {
                    self.selected_index = Some(0);
                }
            }
            if let Some(cb) = change.as_ref() {
                (cb.borrow_mut())(self.selected_index);
            }
        };

        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut toggle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn dropdown_opens_and_selects_first() {
        let items = vec![DropdownItem::new("One", "1"), DropdownItem::new("Two", "2")];
        let mut menu = DropdownMenu::new("Choose", items);
        let app = AppContext::default();
        menu.layout(
            SizeConstraint::loose(vec2f(200.0, 300.0)),
            &mut LayoutContext::default(),
            &app,
        );
        menu.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        menu.dispatch_event(&down, &mut event_ctx, &app);
        menu.dispatch_event(&up, &mut event_ctx, &app);
        assert_eq!(menu.selected_index(), Some(0));

        menu.dispatch_event(&down, &mut event_ctx, &app);
        menu.dispatch_event(&up, &mut event_ctx, &app);
        assert_eq!(menu.selected_index(), Some(0));
    }
}
