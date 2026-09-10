use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Avatar, Axis, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Fill,
    Flex, Label, LabelSize, LayoutContext, MainAxisAlignment, Point, Scrollable, SizeConstraint,
    Text, ThreadListItem,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

/// Summary of a thread displayed in the thread list.
#[derive(Clone, Debug)]
pub struct ThreadListEntry {
    pub id: String,
    pub title: String,
    pub kind: ThreadKind,
    pub selected: bool,
    pub unread_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadKind {
    Chat,
    Channel,
    Direct,
}

pub struct ThreadListView {
    threads: Vec<ThreadListEntry>,
    selected_id: Option<String>,
    on_select: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ThreadListView {
    pub fn new(threads: Vec<ThreadListEntry>) -> Self {
        Self {
            threads,
            selected_id: None,
            on_select: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected(mut self, id: impl Into<String>) -> Self {
        self.selected_id = Some(id.into());
        self
    }

    pub fn with_on_select<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        let header = Container::new(
            Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(Label::new("Threads").with_size(LabelSize::Sm).finish())
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(spacing))
        .finish();
        column = column.with_child(header);
        column = column.with_child(Divider::horizontal().finish());

        let mut list_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(1.0);

        for thread in &self.threads {
            let selected = self
                .selected_id
                .as_ref()
                .map(|id| id == &thread.id)
                .unwrap_or(thread.selected);

            let (bg_token, fg_token, kind_label) = kind_style(thread.kind);
            let leading = Avatar::new(format!("{}{}", kind_label, initial_for(&thread.title)))
                .with_theme_background(bg_token, app)
                .with_theme_foreground(fg_token, app)
                .finish();
            let title = Text::new(thread.title.clone()).finish();
            let badge: Option<Box<dyn Element>> = if thread.unread_count > 0 {
                Some(
                    Container::new(
                        Label::new(format!("{}", thread.unread_count.min(99)))
                            .with_size(LabelSize::Xs)
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Accent)))
                    .with_padding(EdgeInsets::uniform(sm))
                    .finish(),
                )
            } else {
                None
            };

            let thread_id = thread.id.clone();
            let on_select = self.on_select.clone();
            let item = ThreadListItem::new(leading, title, badge, selected, app)
                .with_on_click(move || {
                    if let Some(cb) = on_select.as_ref() {
                        (cb.borrow_mut())(thread_id.clone());
                    }
                })
                .finish();
            list_column = list_column.with_child(item);
        }

        column = column.with_child(Scrollable::new(list_column.finish(), Axis::Vertical).finish());

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
    }
}

fn initial_for(title: &str) -> String {
    title
        .trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "#".to_string())
}

fn kind_style(kind: ThreadKind) -> (ColorToken, ColorToken, &'static str) {
    match kind {
        ThreadKind::Channel => (ColorToken::Accent, ColorToken::Text, "#"),
        ThreadKind::Chat => (ColorToken::Success, ColorToken::Text, "C"),
        ThreadKind::Direct => (ColorToken::Warning, ColorToken::Text, "@"),
    }
}

impl Element for ThreadListView {
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

    fn paint(
        &mut self,
        origin: Vector2F,
        ctx: &mut crate::elements::PaintContext,
        app: &AppContext,
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{AppContext, LayoutContext};
    use crate::geometry::vec2f;

    #[test]
    fn thread_list_view_layouts_with_threads() {
        let app = AppContext::default();
        let threads = vec![
            ThreadListEntry {
                id: "t1".to_string(),
                title: "General".to_string(),
                kind: ThreadKind::Channel,
                selected: true,
                unread_count: 0,
            },
            ThreadListEntry {
                id: "t2".to_string(),
                title: "Chat".to_string(),
                kind: ThreadKind::Chat,
                selected: false,
                unread_count: 2,
            },
        ];
        let mut view = ThreadListView::new(threads);
        let size = view.layout(
            SizeConstraint::loose(vec2f(240.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn thread_list_view_select_callback_fires() {
        let app = AppContext::default();
        let selected = Rc::new(RefCell::new(None));
        let selected_clone = selected.clone();
        let threads = vec![ThreadListEntry {
            id: "t1".to_string(),
            title: "General".to_string(),
            kind: ThreadKind::Channel,
            selected: false,
            unread_count: 0,
        }];
        let mut view = ThreadListView::new(threads)
            .with_selected("t1")
            .with_on_select(move |id| {
                *selected_clone.borrow_mut() = Some(id);
            });
        view.layout(
            SizeConstraint::loose(vec2f(240.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        view.paint(
            vec2f(0.0, 0.0),
            &mut crate::elements::PaintContext::default(),
            &app,
        );
        view.dispatch_event(
            &crate::event::DispatchedEvent::MouseDown {
                position: crate::geometry::vec2f(60.0, 100.0),
                button: 0,
            },
            &mut crate::elements::EventContext::default(),
            &app,
        );
        view.dispatch_event(
            &crate::event::DispatchedEvent::MouseUp {
                position: crate::geometry::vec2f(60.0, 100.0),
                button: 0,
            },
            &mut crate::elements::EventContext::default(),
            &app,
        );
        assert_eq!(selected.borrow().as_ref(), Some(&"t1".to_string()));
    }
}
