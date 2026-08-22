use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{ChatAction, ChatFragment, ChatRole};
use crate::elements::{
    Chip, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex,
    LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

const BUBBLE_MAX_WIDTH_RATIO: f32 = 0.8;

pub struct ChatMessageBubble {
    role: ChatRole,
    fragments: Vec<ChatFragment>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatMessageBubble {
    pub fn new(role: ChatRole, fragments: Vec<ChatFragment>) -> Self {
        Self {
            role,
            fragments,
            on_action: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_action<F: FnMut(ChatAction) + 'static>(mut self, callback: F) -> Self {
        self.on_action = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn role(&self) -> ChatRole {
        self.role
    }

    pub fn fragments(&self) -> &[ChatFragment] {
        &self.fragments
    }

    fn rebuild(&mut self, app: &crate::elements::AppContext, max_width: f32) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let radius = app.theme.radius_px();

        let bg = match self.role {
            ChatRole::User => app.theme.color(ColorToken::SurfaceRaised),
            ChatRole::Assistant => app.theme.color(ColorToken::Surface),
        };

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        for fragment in &self.fragments {
            match &fragment.kind {
                crate::elements::chat_content::ChatFragmentKind::Text(text) => {
                    column = column.with_child(
                        Text::new(text.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    );
                }
                crate::elements::chat_content::ChatFragmentKind::Bold(text) => {
                    column = column.with_child(
                        Text::new(text.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(16.0)
                            .finish(),
                    );
                }
                crate::elements::chat_content::ChatFragmentKind::Italic(text) => {
                    column = column.with_child(
                        Text::new(text.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(14.0)
                            .finish(),
                    );
                }
                crate::elements::chat_content::ChatFragmentKind::BoldItalic(text) => {
                    column = column.with_child(
                        Text::new(text.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(16.0)
                            .finish(),
                    );
                }
                crate::elements::chat_content::ChatFragmentKind::Code(code) => {
                    let code_chip = Container::new(
                        Text::new(code.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                    .with_padding(EdgeInsets::uniform(spacing / 2.0))
                    .with_corner_radius(radius / 2.0)
                    .finish();
                    column = column.with_child(code_chip);
                }
                crate::elements::chat_content::ChatFragmentKind::Link { label, url } => {
                    let on_action = self.on_action.clone();
                    let url = url.clone();
                    let link_chip = Chip::new(
                        Text::new(label.clone())
                            .with_theme_color(ColorToken::Accent, app)
                            .finish(),
                    )
                    .with_on_click(move || {
                        if let Some(cb) = on_action.as_ref() {
                            (cb.borrow_mut())(ChatAction::OpenUrl(url.clone()));
                        }
                    })
                    .finish();
                    column = column.with_child(link_chip);
                }
                crate::elements::chat_content::ChatFragmentKind::List { items, ordered } => {
                    let mut list_column = Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_spacing(spacing / 2.0);
                    for (i, item) in items.iter().enumerate() {
                        let prefix = if *ordered {
                            format!("{}. ", i + 1)
                        } else {
                            "• ".to_string()
                        };
                        let row = Flex::row().with_child(
                            Text::new(format!("{}{}", prefix, item))
                                .with_theme_color(ColorToken::Text, app)
                                .finish(),
                        );
                        list_column = list_column.with_child(row.finish());
                    }
                    column = column.with_child(list_column.finish());
                }
                crate::elements::chat_content::ChatFragmentKind::BlockQuote(text) => {
                    let quote = Container::new(
                        Text::new(text.clone())
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                    .with_padding(EdgeInsets::new(
                        padding / 2.0,
                        padding,
                        padding / 2.0,
                        padding,
                    ))
                    .with_corner_radius(radius)
                    .finish();
                    column = column.with_child(quote);
                }
                crate::elements::chat_content::ChatFragmentKind::CodeBlock { lang, code } => {
                    let mut code_column = Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_spacing(2.0);
                    if let Some(lang) = lang {
                        code_column = code_column.with_child(
                            Text::new(lang.clone())
                                .with_theme_color(ColorToken::Muted, app)
                                .with_font_size(10.0)
                                .finish(),
                        );
                    }
                    code_column = code_column.with_child(
                        Text::new(code.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    );
                    let code_container = Container::new(code_column.finish())
                        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                        .with_padding(EdgeInsets::uniform(padding))
                        .with_corner_radius(radius)
                        .finish();
                    column = column.with_child(code_container);
                }
                crate::elements::chat_content::ChatFragmentKind::Heading { level, text } => {
                    let font_size = match level {
                        1 => 20.0,
                        2 => 18.0,
                        3 => 16.0,
                        _ => 14.0,
                    };
                    column = column.with_child(
                        Text::new(text.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(font_size)
                            .finish(),
                    );
                }
                crate::elements::chat_content::ChatFragmentKind::LineBreak => {
                    column = column.with_child(crate::elements::Spacer::new().finish());
                }
                crate::elements::chat_content::ChatFragmentKind::Action { label, payload } => {
                    let on_action = self.on_action.clone();
                    let payload = payload.clone();
                    let chip = Chip::new(
                        Text::new(label.clone())
                            .with_theme_color(ColorToken::Accent, app)
                            .finish(),
                    )
                    .with_on_click(move || {
                        if let Some(cb) = on_action.as_ref() {
                            (cb.borrow_mut())(payload.clone());
                        }
                    })
                    .finish();
                    column = column.with_child(chip);
                }
            }
        }

        let bubble = Container::new(column.finish())
            .with_background(Fill::Solid(bg))
            .with_padding(EdgeInsets::uniform(padding))
            .with_corner_radius(radius)
            .finish();

        let alignment = match self.role {
            ChatRole::User => MainAxisAlignment::End,
            ChatRole::Assistant => MainAxisAlignment::Start,
        };

        let bubble_max_width = (max_width * BUBBLE_MAX_WIDTH_RATIO).max(80.0);
        self.root = Some(
            Flex::row()
                .with_main_axis_alignment(alignment)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(
                    ConstrainedBox::new(bubble)
                        .with_max_width(bubble_max_width)
                        .finish(),
                )
                .finish(),
        );
    }
}

impl Element for ChatMessageBubble {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &crate::elements::AppContext,
    ) -> Vector2F {
        self.rebuild(app, constraint.max.x);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(
        &mut self,
        origin: Vector2F,
        ctx: &mut PaintContext,
        app: &crate::elements::AppContext,
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
        app: &crate::elements::AppContext,
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
    fn bubble_layouts_non_zero() {
        let app = AppContext::default();
        let mut bubble =
            ChatMessageBubble::new(ChatRole::Assistant, vec![ChatFragment::text("Hello")]);
        let size = bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn action_click_fires_callback() {
        let app = AppContext::default();
        let action = ChatAction::Custom("test".to_string());
        let triggered = Rc::new(RefCell::new(None));
        let triggered_clone = triggered.clone();
        let mut bubble = ChatMessageBubble::new(
            ChatRole::Assistant,
            vec![ChatFragment::action("Run", action.clone())],
        )
        .with_on_action(move |a| *triggered_clone.borrow_mut() = Some(a));

        bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        bubble.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = crate::elements::EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(20.0, 20.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(20.0, 20.0),
            button: 0,
        };

        assert!(bubble.dispatch_event(&down, &mut event_ctx, &app));
        assert!(bubble.dispatch_event(&up, &mut event_ctx, &app));
        assert_eq!(triggered.borrow().as_ref(), Some(&action));
    }
}
