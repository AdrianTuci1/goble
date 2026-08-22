use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Avatar, AvatarShape, Button, ButtonVariant, Container, CrossAxisAlignment,
    EdgeInsets, Element, Empty, EventContext, Fill, Flex, Icon, LayoutContext, MainAxisAlignment,
    PaintContext, Point, SizeConstraint, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::theme::{AccentColor, ColorToken, SpacingToken};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConversationStatus {
    #[default]
    Default,
    Success,
    Error,
    Stopped,
}

impl ConversationStatus {
    fn icon_name(&self) -> &'static str {
        match self {
            Self::Default => "agentmode",
            Self::Success => "check",
            Self::Error => "x-circle",
            Self::Stopped => "cancelled",
        }
    }

    fn color(&self, app: &AppContext) -> crate::color::ColorU {
        match self {
            Self::Default => app.theme.color(ColorToken::Muted),
            Self::Success => app.theme.color(ColorToken::Success),
            Self::Error => app.theme.color(ColorToken::Error),
            Self::Stopped => app.theme.color(ColorToken::Warning),
        }
    }
}

pub struct ConversationListItem {
    id: String,
    name: String,
    last_response: String,
    timestamp: String,
    status: ConversationStatus,
    selected: bool,
    hover: bool,
    menu_open: Rc<RefCell<bool>>,
    state: InteractiveState,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ConversationListItem {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        last_response: impl Into<String>,
        timestamp: impl Into<String>,
        status: ConversationStatus,
        selected: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            last_response: last_response.into(),
            timestamp: timestamp.into(),
            status,
            selected,
            hover: false,
            menu_open: Rc::new(RefCell::new(false)),
            state: InteractiveState::default(),
            on_click: None,
            on_delete: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_delete<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_delete = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn ensure_root(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let radius = app.theme.radius_px();

        let bg = if self.selected {
            app.theme.color(ColorToken::Selected)
        } else if self.hover {
            app.theme.color(ColorToken::Hover)
        } else {
            app.theme.color(ColorToken::Surface)
        };

        let avatar = Avatar::new(&self.name)
            .with_size(34.0)
            .with_shape(AvatarShape::Squircle)
            .with_background(avatar_color(&self.name, app))
            .with_theme_foreground(ColorToken::Text, app)
            .finish();

        let name_row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new(self.name.clone())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(13.0)
                    .finish(),
            )
            .with_child(
                Text::new(self.timestamp.clone())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            )
            .finish();

        let status_icon = Icon::new(self.status.icon_name())
            .with_size(12.0)
            .with_color(self.status.color(app))
            .finish();
        let last_response = Text::new(self.last_response.clone())
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(12.0)
            .with_max_lines(1)
            .finish();
        let detail_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(status_icon)
            .with_child(last_response)
            .finish();

        let text_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(name_row)
            .with_child(detail_row)
            .finish();

        let show_actions = self.hover || *self.menu_open.borrow();
        let menu_open = Rc::clone(&self.menu_open);
        let actions = TopbarButton::new(
            Icon::new("dots-horizontal")
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(26.0)
        .with_on_click(move || {
            let mut open = menu_open.borrow_mut();
            *open = !*open;
        })
        .finish();
        let action_container = Container::new(if show_actions {
            actions
        } else {
            Empty::new().with_size(vec2f(26.0, 26.0)).finish()
        })
        .finish();

        let main_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(avatar)
            .with_child(text_column)
            .with_child(action_container)
            .finish();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(0.0)
            .with_child(main_row);

        if *self.menu_open.borrow() {
            let on_delete = self.on_delete.clone();
            let menu_open = Rc::clone(&self.menu_open);
            let delete_button = Button::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(xs)
                    .with_child(
                        Icon::new("trash")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Error, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new("Delete")
                            .with_theme_color(ColorToken::Error, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .finish(),
            )
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || {
                *menu_open.borrow_mut() = false;
                if let Some(cb) = on_delete.as_ref() {
                    (cb.borrow_mut())();
                }
            })
            .finish();
            column = column.with_child(
                Container::new(
                    Flex::row()
                        .with_main_axis_alignment(MainAxisAlignment::End)
                        .with_child(delete_button)
                        .finish(),
                )
                .with_padding(EdgeInsets::new(0.0, spacing, xs, spacing))
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_corner_radius(radius / 2.0)
                .finish(),
            );
        }

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(bg))
                .with_padding(EdgeInsets::uniform(spacing))
                .with_corner_radius(radius)
                .finish(),
        );
    }
}

impl Element for ConversationListItem {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_root(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.ensure_root(app);
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
        self.ensure_root(app);

        // Let internal controls (e.g. the three-dots menu) consume the event first.
        if let Some(root) = self.root.as_mut() {
            if root.dispatch_event(event, ctx, app) {
                return true;
            }
        }

        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        if let DispatchedEvent::MouseMove { position } = event {
            let inside = contains(bounds, *position);
            if !inside {
                *self.menu_open.borrow_mut() = false;
            }
            self.hover = inside;
        }

        let on_click = self.on_click.clone();
        let mut click = move || {
            if let Some(cb) = on_click.as_ref() {
                (cb.borrow_mut())();
            }
        };

        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut click)
    }
}

fn avatar_color(name: &str, app: &AppContext) -> ColorU {
    let hash = name
        .as_bytes()
        .iter()
        .fold(0u32, |a, b| a.wrapping_add(*b as u32));
    let colors = [
        app.theme.accent_color(),
        app.theme.color(ColorToken::Success),
        app.theme.color(ColorToken::Warning),
        app.theme.color(ColorToken::Error),
        AccentColor::Purple.color(),
        AccentColor::Orange.color(),
    ];
    colors[(hash as usize) % colors.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn conversation_list_item_layouts() {
        let app = AppContext::default();
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "I finished the task.",
            "10:42",
            ConversationStatus::Success,
            false,
        );
        let size = item.layout(
            SizeConstraint::loose(vec2f(260.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn conversation_list_item_click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let app = AppContext::default();
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "Hello",
            "09:00",
            ConversationStatus::Default,
            false,
        )
        .with_on_click(move || *clicked_clone.borrow_mut() = true);

        item.layout(
            SizeConstraint::loose(vec2f(260.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        item.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(item.dispatch_event(&down, &mut event_ctx, &app));
        assert!(item.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*clicked.borrow());
    }
}
