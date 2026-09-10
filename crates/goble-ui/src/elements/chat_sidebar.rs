use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Empty, Fill, Flex, Icon,
    IconButton, Label, LabelSize, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext,
    Point, SizeConstraint, Spacer, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub const CHAT_SIDEBAR_WIDTH: f32 = 280.0;

/// A single routine shown in the right chat sidebar's Routines section.
///
/// Owned by the app (derived from the agent's scheduled tasks) and rendered
/// from scratch each frame, so it carries no interaction state of its own.
#[derive(Clone, Debug, Default)]
pub struct RoutineItem {
    pub title: String,
    pub schedule: String,
    pub enabled: bool,
}

impl RoutineItem {
    pub fn new(title: impl Into<String>, schedule: impl Into<String>, enabled: bool) -> Self {
        Self {
            title: title.into(),
            schedule: schedule.into(),
            enabled,
        }
    }
}

/// The right chat sidebar: a Computer Use preview plus a Routines list.
///
/// Rendered inside [`ChatLayout`](crate::elements::ChatLayout) next to the
/// chat surface. Hidden unless the user toggles it from the chat header.
pub struct ChatSidebar {
    routines: Vec<RoutineItem>,
    on_add: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatSidebar {
    pub fn new(_app: &AppContext) -> Self {
        Self {
            routines: Vec::new(),
            on_add: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_routines(mut self, routines: Vec<RoutineItem>) -> Self {
        self.routines = routines;
        self
    }

    pub fn with_on_add<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_add = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build_root(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let radius = app.theme.radius_px();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(spacing);

        // Computer Use section.
        column = column.with_child(
            Label::new("Computer Use")
                .with_size(LabelSize::Xs)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );

        let preview = Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_spacing(xs)
                .with_child(
                    Icon::new("terminal")
                        .with_size(32.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new("Active")
                        .with_theme_color(ColorToken::Success, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(radius)
        .with_padding(EdgeInsets::uniform(spacing))
        .finish();
        column = column.with_child(preview);

        column = column.with_child(crate::elements::Divider::horizontal().finish());

        // Routines section header: title + "new routine" button.
        let mut header = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Label::new("Routines")
                    .with_size(LabelSize::Xs)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(Spacer::new().finish());
        if let Some(cb) = self.on_add.clone() {
            header = header.with_child(
                IconButton::new(
                    Icon::new("plus")
                        .with_size(14.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_size(crate::geometry::vec2f(24.0, 24.0))
                .with_on_click(move || (cb.borrow_mut())())
                .finish(),
            );
        }
        column = column.with_child(header.finish());

        // Routines list.
        let mut routines = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        for item in &self.routines {
            let dot_color = if item.enabled {
                ColorToken::Accent
            } else {
                ColorToken::Muted
            };
            let dot = Container::new(Empty::new().with_size(crate::geometry::vec2f(6.0, 6.0)).finish())
                .with_background(Fill::Solid(app.theme.color(dot_color)))
                .with_corner_radius(3.0)
                .finish();
            let row = Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(sm)
                .with_child(dot)
                .with_child(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(2.0)
                        .with_child(
                            Text::new(item.title.clone())
                                .with_theme_color(ColorToken::Text, app)
                                .with_font_size(12.0)
                                .finish(),
                        )
                        .with_child(
                            Text::new(item.schedule.clone())
                                .with_theme_color(ColorToken::Muted, app)
                                .with_font_size(11.0)
                                .finish(),
                        )
                        .finish(),
                )
                .finish();
            routines = routines.with_child(
                Container::new(row)
                    .with_padding(EdgeInsets::uniform(sm))
                    .with_corner_radius(radius / 2.0)
                    .finish(),
            );
        }
        column = column.with_child(routines.finish());

        column = column.with_child(Spacer::new().finish());

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_border(app.theme.color(ColorToken::Border).into())
                .with_padding(EdgeInsets::uniform(spacing))
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
        self.build_root(app);
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
    use crate::elements::{ChatLayout, Empty};
    use crate::geometry::vec2f;

    #[test]
    fn chat_sidebar_layouts() {
        let app = AppContext::default();
        let mut sidebar = ChatSidebar::new(&app)
            .with_routines(vec![
                RoutineItem::new("Morning social", "Every day 8 AM", true),
                RoutineItem::new("Outbound weekly", "Fridays 10 AM", false),
            ])
            .finish();
        let size = sidebar.layout(
            SizeConstraint::loose(vec2f(400.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn routines_render_add_button_when_callback_set() {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = ChatSidebar::new(&app)
            .with_routines(vec![RoutineItem::new("Morning social", "Every day 8 AM", true)])
            .with_on_add(|| {})
            .finish();
        let commands = crate::test_util::render_element(&mut element, vec2f(280.0, 600.0), &app);
        let has_plus = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawIcon { name, .. } if name == "plus")
        });
        assert!(has_plus, "sidebar with an add callback should render a plus button");
    }

    #[test]
    fn chat_sidebar_renders_routines_inside_chat_layout() {
        let app = AppContext::default();
        let sidebar = ChatSidebar::new(&app)
            .with_routines(vec![
                RoutineItem::new("Morning social", "Every day 8 AM", true),
                RoutineItem::new("Outbound weekly", "Fridays 10 AM", false),
            ])
            .with_on_add(|| {})
            .finish();
        let mut layout: Box<dyn Element> = ChatLayout::new(
            Empty::new()
                .with_size(vec2f(400.0, 600.0))
                .finish(),
        )
        .with_right_sidebar(sidebar)
        .finish();
        let commands =
            crate::test_util::render_element(&mut layout, vec2f(680.0, 600.0), &app);
        let has_title = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawText { text, .. } if text == "Morning social")
        });
        let has_schedule = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawText { text, .. }
                if text == "Every day 8 AM")
        });
        assert!(has_title, "right sidebar should render routine titles");
        assert!(has_schedule, "right sidebar should render routine schedules");
    }
}
