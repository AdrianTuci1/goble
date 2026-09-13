use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, Element, EventContext,
    Expanded, Fill, Flex, Icon, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext,
    Point, SizeConstraint, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, RectF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

/// Extra height above and below a card's rows. The card is a full-width band,
/// so without it the two text rows sit flush against the cards above and below
/// and the digest reads as one block of text.
const CARD_EXTRA_HEIGHT: f32 = 4.0;

/// Per-card interaction state that must survive the per-frame element rebuild.
/// Owned by the app (a map keyed by conversation id) and shared with the card
/// through `Rc<RefCell<_>>`, so hover / the delete menu persist across frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct AgentCardUi {
    pub hover: bool,
    pub menu_open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConversationStatus {
    #[default]
    Default,
    Success,
    Error,
    Stopped,
}

pub struct ConversationListItem {
    id: String,
    name: String,
    last_response: String,
    timestamp: String,
    /// The directory the conversation works in, drawn under its subject. `None`
    /// (or empty) draws no row, so a conversation that has not run anywhere yet
    /// keeps the two-line card.
    directory: Option<String>,
    selected: bool,
    /// Whether the conversation is starred, which the card marks and the
    /// sidebar's Starred section lists.
    starred: bool,
    /// The work environment this conversation runs in (`"local"` / `"remote"`),
    /// used to pick the environment SVG shown in the card.
    workspace_routing: String,
    ui: Rc<RefCell<AgentCardUi>>,
    on_select: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_toggle_star: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    bg: crate::color::ColorU,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ConversationListItem {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        last_response: impl Into<String>,
        timestamp: impl Into<String>,
        ui: Rc<RefCell<AgentCardUi>>,
        selected: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            last_response: last_response.into(),
            timestamp: timestamp.into(),
            directory: None,
            selected,
            starred: false,
            workspace_routing: "local".to_string(),
            ui,
            on_select: None,
            on_delete: None,
            on_toggle_star: None,
            root: None,
            bg: crate::color::ColorU::default(),
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_delete<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_delete = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether the conversation is starred. A starred card marks itself, so a
    /// card that also sits in the sidebar's Starred section still says why.
    pub fn with_starred(mut self, starred: bool) -> Self {
        self.starred = starred;
        self
    }

    /// Star or unstar the conversation, from the card's own menu.
    pub fn with_on_toggle_star<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_star = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the conversation's work environment, which picks the SVG shown in
    /// the card (local vs remote avatar).
    pub fn with_workspace_routing(mut self, routing: impl Into<String>) -> Self {
        self.workspace_routing = routing.into();
        self
    }

    /// Set the working directory drawn under the card's subject. An empty path
    /// draws no row.
    pub fn with_directory(mut self, directory: impl Into<String>) -> Self {
        let directory = directory.into();
        self.directory = if directory.trim().is_empty() {
            None
        } else {
            Some(directory)
        };
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Build the root tree once, reflecting the current hover/menu state. It is
    /// called from `layout` (which has `app`); it is *not* rebuilt during
    /// `paint`/`dispatch`, so the sizes computed by `layout` are preserved.
    fn ensure_root(&mut self, app: &AppContext) {
        if self.root.is_some() {
            return;
        }
        let spacing = 10.0_f32;
        let xs = 6.0_f32;

        let ui = self.ui.borrow();
        let hover = ui.hover;
        let menu_open = ui.menu_open;

        let bg = if self.selected {
            app.theme.color(ColorToken::Selected)
        } else if hover {
            app.theme.color(ColorToken::Hover)
        } else {
            app.theme.color(ColorToken::Surface)
        };
        self.bg = bg;

        let mut subject = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Text::new(self.name.clone())
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            );
        if self.starred {
            subject = subject.with_child(
                Icon::new("star-filled")
                    .with_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        let name_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_spacing(xs)
            .with_child(subject.finish())
            .with_child(
                Text::new(self.timestamp.clone())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();

        let last_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Text::new(self.last_response.clone())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish();

        let mut text_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(2.0)
            .with_child(name_row);
        // The directory the conversation works in, directly under its subject:
        // it says where the work happens, which the title alone cannot.
        if let Some(directory) = self.directory.clone() {
            text_column = text_column.with_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(4.0)
                    .with_child(
                        Icon::new("folder")
                            .with_size(11.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new(directory)
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(11.0)
                            .with_max_lines(1)
                            .finish(),
                    )
                    .finish(),
            );
        }
        let text_column = text_column.with_child(last_row).finish();

        // Environment SVG badge: the conversation's work medium (Local or
        // Remote) is shown directly on the card, mirroring the topbar medium
        // selector so it is obvious where the conversation runs.
        let icon_name = if self.workspace_routing == "remote" {
            "conversation-remote"
        } else {
            "conversation-local"
        };
        let avatar = Container::new(
            Icon::new(icon_name)
                .with_size(16.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(crate::style::EdgeInsets::new(5.0, 5.0, 5.0, 5.0))
        .finish();

        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(avatar)
            .with_child(Expanded::new(text_column).finish());

        // 3-dot menu, visible on hover or while the delete menu is open.
        if hover || menu_open {
            let ui_dots = Rc::clone(&self.ui);
            let dots = TopbarButton::new(
                Icon::new("dots-horizontal")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(26.0)
            .with_corner_radius(0.0)
            .with_on_click(move || {
                let mut ui = ui_dots.borrow_mut();
                ui.menu_open = !ui.menu_open;
            })
            .finish();
            row = row.with_child(dots);
        }

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_child(row.finish());

        // The card's own menu, shown as a row under it while open: star the
        // conversation, or delete it.
        if menu_open {
            let mut actions = Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::End)
                .with_spacing(xs);
            if let Some(star) = self.star_button(app) {
                actions = actions.with_child(star);
            }
            actions = actions.with_child(self.delete_button(app));
            column = column.with_child(
                Container::new(actions.finish())
                    .with_padding(crate::style::EdgeInsets::new(0.0, 0.0, xs, 0.0))
                    .finish(),
            );
        }

        // Square: the sidebar is a flat surface, so a card is a full-width band
        // rather than a rounded widget.
        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(bg))
                .with_padding(crate::style::EdgeInsets::new(
                    0.0,
                    CARD_EXTRA_HEIGHT,
                    0.0,
                    CARD_EXTRA_HEIGHT,
                ))
                .finish(),
        );
    }

    /// The menu's star row, or `None` when the card was given no star action.
    /// Starring closes the menu: the choice is made, and the card and the
    /// sidebar's Starred section already show it.
    fn star_button(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let on_toggle = self.on_toggle_star.clone()?;
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let starred = self.starred;
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Icon::new(if starred { "star-filled" } else { "star" })
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new(if starred { "Unstar" } else { "Star" })
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();
        let ui = Rc::clone(&self.ui);
        Some(
            Button::new(row)
                .with_variant(ButtonVariant::Ghost)
                .with_corner_radius(0.0)
                .with_on_click(move || {
                    ui.borrow_mut().menu_open = false;
                    (on_toggle.borrow_mut())();
                })
                .finish(),
        )
    }

    fn delete_button(&self, app: &AppContext) -> Box<dyn Element> {
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let on_delete = self.on_delete.clone();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(xs)
            .with_child(
                Icon::new("trash")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Error, app)
                    .finish(),
            )
            .with_child(
                Text::new("Delete agent")
                    .with_theme_color(ColorToken::Error, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();
        Button::new(row)
            .with_variant(ButtonVariant::Ghost)
            .with_corner_radius(0.0)
            .with_on_click(move || {
                if let Some(cb) = on_delete.as_ref() {
                    (cb.borrow_mut())();
                }
            })
            .finish()
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
        self.origin = Some(Point::from_vec2f(origin, Default::default()));

        // Clip the card content to its own bounds so a long one-line last
        // message is cut at the card's right edge instead of spilling into the
        // main area. The fade (drawn after pop) stays unclipped so it can cover
        // that edge.
        if let Some(b) = self.size.map(|s| rectf(origin.x, origin.y, s.x, s.y)) {
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.clip_rect(b);
            }
        }
        if let Some(root) = self.root.as_mut() {
            root.paint(origin, ctx, app);
        }
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.pop_clip();
        }

        // Fade the right edge of the row until the 3-dot menu appears (which
        // then truncates the text itself). Skip while the menu is open so the
        // fade never covers the delete button.
        let ui = self.ui.borrow();
        if !ui.hover && !ui.menu_open {
            if let Some(size) = self.size {
                let fade_w = 24.0_f32;
                if size.x > fade_w {
                    let rect = rectf(origin.x + size.x - fade_w, origin.y, fade_w, size.y);
                    if let Some(renderer) = ctx.renderer.as_mut() {
                        renderer.fill_rect_fade_right(rect, self.bg, 0.0);
                    }
                }
            }
        }
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
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        // Let the inner 3-dot button (and the delete button) consume the event
        // first; otherwise the row itself would swallow the click.
        if let Some(root) = self.root.as_mut() {
            if root.dispatch_event(event, ctx, app) {
                return true;
            }
        }

        if let DispatchedEvent::MouseMove { position } = event {
            let inside = contains(bounds, *position);
            let mut ui = self.ui.borrow_mut();
            ui.hover = inside;
            if !inside {
                ui.menu_open = false;
            }
            return false;
        }

        let on_select = self.on_select.clone();
        let mut select = move || {
            if let Some(cb) = on_select.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut select)
    }
}

/// A small helper used by tests to find a card's hover bounds.
pub fn card_bounds(item: &ConversationListItem) -> Option<RectF> {
    item.bounds()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn conversation_list_item_layouts() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "I finished the task.",
            "40 min ago",
            ui,
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
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "Hello",
            "5 min ago",
            ui,
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

    #[test]
    fn hover_updates_shared_state() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let mut item = ConversationListItem::new(
            "c1",
            "Ada",
            "Hello",
            "5 min ago",
            Rc::clone(&ui),
            false,
        );
        item.layout(
            SizeConstraint::loose(vec2f(260.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        item.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        item.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(10.0, 10.0),
            },
            &mut event_ctx,
            &app,
        );
        assert!(ui.borrow().hover);
    }

    #[test]
    fn fade_emits_gradient_command() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi::default()));
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "A very long last message that should fade out at the right edge",
            "5 min ago",
            ui,
            false,
        );
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);
        let counts = crate::test_util::command_counts(&commands);
        assert_eq!(counts.fill_rect_fade, 1, "idle card should emit a right fade");
        assert!(counts.draw_text > 0, "card should render text");
    }

    #[test]
    fn hover_renders_dots_and_menu_renders_delete() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            hover: true,
            menu_open: true,
        }));
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "A long last message",
            "5 min ago",
            ui,
            false,
        );
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

        let has_dots = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawIcon { name, .. } if name == "dots-horizontal")
        });
        let has_delete = commands.iter().any(|c| {
            matches!(c, crate::render::RenderCommand::DrawText { text, .. } if text.contains("Delete agent"))
        });
        assert!(has_dots, "hover/menu card should render the 3-dot icon");
        assert!(has_delete, "open menu should render a 'Delete agent' action");
    }

    /// The card's menu carries the star toggle, and clicking it stars the
    /// conversation and puts the menu away: the card and the sidebar's Starred
    /// section are where the choice now shows.
    #[test]
    fn the_menus_star_row_stars_the_conversation_and_closes_the_menu() {
        let app = AppContext::default();
        let ui = Rc::new(RefCell::new(AgentCardUi {
            hover: true,
            menu_open: true,
        }));
        let toggled = Rc::new(RefCell::new(0usize));
        let ran = Rc::clone(&toggled);
        let item = ConversationListItem::new(
            "c1",
            "Ada",
            "A long last message",
            "5 min ago",
            Rc::clone(&ui),
            false,
        )
        .with_on_toggle_star(move || *ran.borrow_mut() += 1);
        let mut element: Box<dyn Element> = Box::new(item);
        let commands = crate::test_util::render_element(&mut element, vec2f(260.0, 200.0), &app);

        let offers_star = commands.iter().any(|command| {
            matches!(command, crate::render::RenderCommand::DrawText { text, .. } if text == "Star")
        });
        assert!(offers_star, "the open menu should offer Star");

        let origin = commands
            .iter()
            .find_map(|command| match command {
                crate::render::RenderCommand::DrawIcon { name, origin, .. } if name == "star" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the star row's icon is drawn");
        let at = origin + vec2f(7.0, 7.0);
        let mut event_ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            element.dispatch_event(&event, &mut event_ctx, &app);
        }

        assert_eq!(*toggled.borrow(), 1, "the star row ran the toggle");
        assert!(!ui.borrow().menu_open, "starring closes the menu");
    }
}
