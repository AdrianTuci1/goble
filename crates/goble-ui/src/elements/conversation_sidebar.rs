use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Container, ConversationListItem, ConversationStatus, CrossAxisAlignment,
    EdgeInsets, Element, EventContext, Fill, Flex, Icon, LayoutContext, PaintContext, Point,
    Scrollable, SearchInput, SizeConstraint, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub const CONVERSATION_SIDEBAR_WIDTH: f32 = 260.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConversationEntry {
    pub id: String,
    pub name: String,
    pub last_response: String,
    pub timestamp: String,
    pub status: ConversationStatus,
}

impl ConversationEntry {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        last_response: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            last_response: last_response.into(),
            timestamp: timestamp.into(),
            status: ConversationStatus::Default,
        }
    }

    pub fn with_status(mut self, status: ConversationStatus) -> Self {
        self.status = status;
        self
    }
}

pub struct ConversationSidebar {
    search: String,
    conversations: Vec<ConversationEntry>,
    selected_id: Option<String>,
    on_search_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_create: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ConversationSidebar {
    pub fn new(conversations: Vec<ConversationEntry>) -> Self {
        Self {
            search: String::new(),
            conversations,
            selected_id: None,
            on_search_change: None,
            on_create: None,
            on_select: None,
            on_delete: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected(mut self, id: impl Into<String>) -> Self {
        self.selected_id = Some(id.into());
        self
    }

    pub fn with_on_search_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_search_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_create<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_create = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_delete<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_delete = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let _radius = app.theme.radius_px();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        // Header: search + new conversation.
        let on_search = self.on_search_change.clone();
        let search_value = self.search.clone();
        let search = SearchInput::new()
            .with_value(search_value)
            .with_placeholder("Search conversations...")
            .with_on_change(move |value| {
                if let Some(cb) = on_search.as_ref() {
                    (cb.borrow_mut())(value);
                }
            })
            .finish();

        let search_width = CONVERSATION_SIDEBAR_WIDTH - spacing * 2.0 - 32.0 - sm;
        let constrained_search = crate::elements::ConstrainedBox::new(search)
            .with_width(search_width.max(60.0))
            .finish();

        let on_create = self.on_create.clone();
        let create_button = TopbarButton::new(
            Icon::new("plus")
                .with_size(18.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_size(32.0)
        .with_on_click(move || {
            if let Some(cb) = on_create.as_ref() {
                (cb.borrow_mut())();
            }
        })
        .finish();

        let header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(constrained_search)
            .with_child(create_button)
            .finish();
        column = column.with_child(header);

        // Conversation list.
        if self.conversations.is_empty() {
            column = column.with_child(
                Container::new(
                    Text::new("No conversations yet.")
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
            );
        } else {
            let mut list = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(2.0);
            for entry in &self.conversations {
                let selected = self
                    .selected_id
                    .as_ref()
                    .map(|id| id == &entry.id)
                    .unwrap_or(false);
                let select_id = entry.id.clone();
                let delete_id = entry.id.clone();
                let on_select = self.on_select.clone();
                let on_delete = self.on_delete.clone();
                let item = ConversationListItem::new(
                    entry.id.clone(),
                    entry.name.clone(),
                    entry.last_response.clone(),
                    entry.timestamp.clone(),
                    entry.status,
                    selected,
                )
                .with_on_click(move || {
                    if let Some(cb) = on_select.as_ref() {
                        (cb.borrow_mut())(select_id.clone());
                    }
                })
                .with_on_delete(move || {
                    if let Some(cb) = on_delete.as_ref() {
                        (cb.borrow_mut())(delete_id.clone());
                    }
                })
                .finish();
                list = list.with_child(item);
            }
            column = column.with_child(
                Scrollable::new(list.finish(), crate::elements::Axis::Vertical).finish(),
            );
        }

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn conversation_sidebar_layouts() {
        let app = AppContext::default();
        let conversations = vec![
            ConversationEntry::new("c1", "Ada", "Hello!", "10:00"),
            ConversationEntry::new("c2", "Coder", "Done.", "09:30"),
        ];
        let mut sidebar = ConversationSidebar::new(conversations);
        let size = sidebar.layout(
            SizeConstraint::loose(vec2f(CONVERSATION_SIDEBAR_WIDTH, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
