use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Axis, Container, CrossAxisAlignment, EdgeInsets, Element, Flex, Icon, IconButton,
    LayoutContext, MainAxisAlignment, PaintContext, Point, Scrollable, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

use super::conversation_list_item::ConversationListItem;

#[derive(Clone, Debug, Default)]
pub struct ConversationEntry {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub timestamp: String,
    pub selected: bool,
}

pub struct ConversationSidebar {
    title: String,
    conversations: Vec<ConversationEntry>,
    on_select: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_create_new: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ConversationSidebar {
    pub fn new(title: impl Into<String>, conversations: Vec<ConversationEntry>) -> Self {
        Self {
            title: title.into(),
            conversations,
            on_select: None,
            on_delete: None,
            on_create_new: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_select<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_delete<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_delete = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_create_new<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_create_new = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let text_color = app.theme.color(ColorToken::Text);

        let title = Text::new(self.title.clone())
            .with_font_size(13.0)
            .with_color(text_color)
            .finish();

        let create_new = self.on_create_new.clone();
        let new_button = IconButton::new(
            Icon::new("plus")
                .with_size(16.0)
                .with_color(text_color)
                .finish(),
        )
        .with_on_click(move || {
            if let Some(cb) = create_new.as_ref() {
                (cb.borrow_mut())();
            }
        })
        .finish();

        let header = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(title)
            .with_child(new_button)
            .finish();

        let mut list = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(1.0);

        for entry in &self.conversations {
            let id = entry.id.clone();
            let on_select = self.on_select.clone();
            let on_delete = self.on_delete.clone();
            let item = ConversationListItem::new(
                entry.title.clone(),
                entry.preview.clone(),
                entry.timestamp.clone(),
                entry.selected,
                app,
            )
            .with_on_select(move || {
                if let Some(cb) = on_select.as_ref() {
                    (cb.borrow_mut())(id.clone());
                }
            })
            .with_on_delete({
                let id = entry.id.clone();
                move || {
                    if let Some(cb) = on_delete.as_ref() {
                        (cb.borrow_mut())(id.clone());
                    }
                }
            })
            .finish();
            list = list.with_child(item);
        }

        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm)
            .with_child(header)
            .with_child(Scrollable::new(list.finish(), Axis::Vertical).finish())
            .finish();

        self.root = Some(
            Container::new(column)
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
    }
}

impl Default for ConversationSidebar {
    fn default() -> Self {
        Self::new("Conversations", Vec::new())
    }
}

impl Element for ConversationSidebar {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
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
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}
