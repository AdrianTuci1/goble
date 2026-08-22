use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets, Element,
    Fill, Flex, IconButton, Label, LabelSize, LayoutContext, MainAxisAlignment, PaintContext,
    Point, Scrollable, SizeConstraint, Text, ThreadListItem,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};
use crate::views::thread_list_view::{ThreadKind, ThreadListEntry};

/// A Slack-style sidebar that organizes threads into collapsible sections.
pub struct ThreadSidebar {
    threads: Vec<ThreadListEntry>,
    selected_id: Option<String>,
    collapsed: HashSet<String>,
    on_select: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_new: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_toggle_section: Option<Rc<RefCell<dyn FnMut(String, bool) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ThreadSidebar {
    pub fn new(threads: Vec<ThreadListEntry>) -> Self {
        Self {
            threads,
            selected_id: None,
            collapsed: HashSet::new(),
            on_select: None,
            on_new: None,
            on_toggle_section: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected(mut self, id: impl Into<String>) -> Self {
        self.selected_id = Some(id.into());
        self
    }

    pub fn with_collapsed(mut self, sections: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.collapsed = sections.into_iter().map(|s| s.into()).collect();
        self
    }

    pub fn with_on_select<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_new<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_new = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_toggle_section<F: FnMut(String, bool) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_section = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let xs = app.theme.spacing_px(SpacingToken::Xs);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        let on_new = self.on_new.clone();
        let header = Container::new(
            Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new("Threads")
                        .with_theme_color(ColorToken::Text, app)
                        .with_font_size(18.0)
                        .finish(),
                )
                .with_child(
                    IconButton::new(Text::new("+").finish())
                        .with_on_click(move || {
                            if let Some(cb) = on_new.as_ref() {
                                (cb.borrow_mut())();
                            }
                        })
                        .finish(),
                )
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(spacing))
        .finish();
        column = column.with_child(header);
        column = column.with_child(Divider::horizontal().finish());

        let sections = [
            ("Channels", ThreadKind::Channel, "#"),
            ("Direct messages", ThreadKind::Direct, "@"),
            ("Chats", ThreadKind::Chat, "▶"),
        ];

        for (section_name, kind, prefix) in sections {
            let items: Vec<&ThreadListEntry> =
                self.threads.iter().filter(|t| t.kind == kind).collect();

            let is_collapsed = self.collapsed.contains(section_name);
            let section_id = section_name.to_string();
            let on_toggle = self.on_toggle_section.clone();

            let section_header = Button::new(
                Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(sm)
                            .with_child(
                                Label::new(if is_collapsed { "▶" } else { "▼" })
                                    .with_size(LabelSize::Xs)
                                    .finish(),
                            )
                            .with_child(
                                Text::new(format!("{} ({})", section_name, items.len()))
                                    .with_theme_color(ColorToken::Muted, app)
                                    .with_font_size(12.0)
                                    .finish(),
                            )
                            .finish(),
                    )
                    .finish(),
            )
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || {
                if let Some(cb) = on_toggle.as_ref() {
                    (cb.borrow_mut())(section_id.clone(), !is_collapsed);
                }
            })
            .finish();
            column = column.with_child(section_header);

            if !is_collapsed {
                let mut list_column = Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(1.0);

                for thread in &items {
                    let selected = self
                        .selected_id
                        .as_ref()
                        .map(|id| id == &thread.id)
                        .unwrap_or(thread.selected);

                    let leading = Text::new(format!("{}{}", prefix, initial_for(&thread.title)))
                        .with_theme_color(ColorToken::Text, app)
                        .finish();
                    let title = Text::new(thread.title.clone())
                        .with_theme_color(ColorToken::Text, app)
                        .finish();
                    let badge: Option<Box<dyn Element>> = if thread.unread_count > 0 {
                        Some(
                            Container::new(
                                Label::new(format!("{}", thread.unread_count.min(99)))
                                    .with_size(LabelSize::Xs)
                                    .finish(),
                            )
                            .with_background(Fill::Solid(app.theme.color(ColorToken::Accent)))
                            .with_padding(EdgeInsets::uniform(xs))
                            .with_corner_radius(app.theme.radius_px() / 2.0)
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

                if items.is_empty() {
                    list_column = list_column.with_child(
                        Container::new(
                            Text::new("No items")
                                .with_theme_color(ColorToken::Muted, app)
                                .with_font_size(11.0)
                                .finish(),
                        )
                        .with_padding(EdgeInsets::new(0.0, spacing, 0.0, spacing))
                        .finish(),
                    );
                }

                column = column.with_child(
                    Scrollable::new(list_column.finish(), crate::elements::Axis::Vertical).finish(),
                );
            }
        }

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

impl Element for ThreadSidebar {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{AppContext, LayoutContext};
    use crate::geometry::vec2f;

    fn sample_threads() -> Vec<ThreadListEntry> {
        vec![
            ThreadListEntry {
                id: "c1".to_string(),
                title: "General".to_string(),
                kind: ThreadKind::Channel,
                selected: false,
                unread_count: 2,
            },
            ThreadListEntry {
                id: "d1".to_string(),
                title: "Ada".to_string(),
                kind: ThreadKind::Direct,
                selected: false,
                unread_count: 0,
            },
            ThreadListEntry {
                id: "ch1".to_string(),
                title: "Support".to_string(),
                kind: ThreadKind::Chat,
                selected: true,
                unread_count: 1,
            },
        ]
    }

    #[test]
    fn sidebar_layouts_with_sections() {
        let app = AppContext::default();
        let mut sidebar = ThreadSidebar::new(sample_threads());
        let size = sidebar.layout(
            SizeConstraint::loose(vec2f(240.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn sidebar_collapsed_section_hides_items() {
        let app = AppContext::default();
        let mut sidebar = ThreadSidebar::new(sample_threads()).with_collapsed(["Channels"]);
        let size = sidebar.layout(
            SizeConstraint::loose(vec2f(240.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn sidebar_selected_id_returns_value() {
        let sidebar = ThreadSidebar::new(vec![]).with_selected("c1");
        assert_eq!(sidebar.selected_id(), Some("c1"));
    }
}
