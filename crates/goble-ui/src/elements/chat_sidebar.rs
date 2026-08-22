use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Axis, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment,
    EdgeInsets, Element, EventContext, Fill, Flex, LayoutContext, PaintContext, Point, Scrollable,
    SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChatSidebarTab {
    #[default]
    Info,
    History,
}

pub struct ChatSidebar {
    active_tab: ChatSidebarTab,
    info_content: Option<Box<dyn Element>>,
    history_items: Vec<Option<Box<dyn Element>>>,
    on_change_tab: Option<Rc<RefCell<dyn FnMut(ChatSidebarTab) + 'static>>>,
    width: f32,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatSidebar {
    pub fn new(active_tab: ChatSidebarTab) -> Self {
        Self {
            active_tab,
            info_content: None,
            history_items: Vec::new(),
            on_change_tab: None,
            width: 240.0,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_info_content(mut self, content: Box<dyn Element>) -> Self {
        self.info_content = Some(content);
        self
    }

    pub fn with_history_items(mut self, items: Vec<Box<dyn Element>>) -> Self {
        self.history_items = items.into_iter().map(Some).collect();
        self
    }

    pub fn with_on_change_tab<F: FnMut(ChatSidebarTab) + 'static>(mut self, callback: F) -> Self {
        self.on_change_tab = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let info_selected = self.active_tab == ChatSidebarTab::Info;
        let history_selected = self.active_tab == ChatSidebarTab::History;
        let on_change = self.on_change_tab.clone();

        let tabs = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(
                Button::new(
                    Text::new("Info")
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_variant(if info_selected {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .with_on_click({
                    let on_change = on_change.clone();
                    move || {
                        if let Some(cb) = on_change.as_ref() {
                            (cb.borrow_mut())(ChatSidebarTab::Info);
                        }
                    }
                })
                .finish(),
            )
            .with_child(
                Button::new(
                    Text::new("History")
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_variant(if history_selected {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .with_on_click(move || {
                    if let Some(cb) = on_change.as_ref() {
                        (cb.borrow_mut())(ChatSidebarTab::History);
                    }
                })
                .finish(),
            )
            .finish();

        let mut body_col = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        match self.active_tab {
            ChatSidebarTab::Info => {
                let info = self
                    .info_content
                    .take()
                    .unwrap_or_else(|| Container::new(Text::new("No info").finish()).finish());
                body_col = body_col.with_child(info);
            }
            ChatSidebarTab::History => {
                if self.history_items.is_empty() {
                    body_col = body_col.with_child(
                        Text::new("No executions yet.")
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    );
                } else {
                    for item in self.history_items.iter_mut().filter_map(|i| i.take()) {
                        body_col = body_col.with_child(item);
                    }
                }
            }
        }

        let body = Scrollable::new(body_col.finish(), Axis::Vertical).finish();

        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(tabs)
            .with_child(body)
            .finish();

        let root = Container::new(column)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish();

        self.root = Some(
            ConstrainedBox::new(root)
                .with_width(self.width)
                .with_min_width(self.width)
                .with_max_width(self.width)
                .finish(),
        );
    }
}

impl Element for ChatSidebar {
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
