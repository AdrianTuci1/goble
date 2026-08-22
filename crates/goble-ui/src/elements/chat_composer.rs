use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Chip, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, Icon,
    LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, Spacer, TextArea,
    TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct ChatComposer {
    value: Rc<RefCell<String>>,
    placeholder: String,
    attachments: Vec<String>,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_attach: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_model: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_key: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_variant: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            value: Rc::new(RefCell::new(String::new())),
            placeholder: String::from("Ask anything..."),
            attachments: Vec::new(),
            on_change: None,
            on_send: None,
            on_attach: None,
            on_select_model: None,
            on_select_key: None,
            on_select_variant: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_value(self, value: impl Into<String>) -> Self {
        *self.value.borrow_mut() = value.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_attachments(mut self, attachments: Vec<String>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_send<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_attach<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_attach = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_model<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_key<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_variant<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_variant = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn attachments(&self) -> &[String] {
        &self.attachments
    }

    fn rebuild(&mut self, app: &AppContext) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let radius = app.theme.radius_px();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        if !self.attachments.is_empty() {
            let mut attachment_row = Flex::row().with_spacing(spacing);
            for attachment in &self.attachments {
                let chip = Chip::new(
                    crate::elements::Text::new(attachment.clone())
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .finish();
                attachment_row = attachment_row.with_child(chip);
            }
            column = column.with_child(attachment_row.finish());
        }

        let value = self.value.clone();
        let on_change = self.on_change.clone();
        let textarea = TextArea::new()
            .with_value(self.value.borrow().clone())
            .with_placeholder(self.placeholder.clone())
            .with_min_height(60.0)
            .with_on_change(move |text| {
                *value.borrow_mut() = text.clone();
                if let Some(cb) = on_change.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .finish();
        column = column.with_child(textarea);

        let value_for_send = self.value.clone();
        let on_send = self.on_send.clone();
        let send = move || {
            let text = value_for_send.borrow().clone();
            if !text.is_empty() {
                if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text);
                }
                *value_for_send.borrow_mut() = String::new();
            }
        };

        let mut footer = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);

        let mut left_group = Flex::row().with_spacing(spacing);
        if let Some(cb) = self.on_select_model.clone() {
            left_group = left_group.with_child(
                TopbarButton::new(
                    Icon::new("cpu")
                        .with_size(18.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_on_click(move || (cb.borrow_mut())())
                .finish(),
            );
        }
        if let Some(cb) = self.on_select_key.clone() {
            left_group = left_group.with_child(
                TopbarButton::new(
                    Icon::new("key")
                        .with_size(18.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_on_click(move || (cb.borrow_mut())())
                .finish(),
            );
        }
        if let Some(cb) = self.on_select_variant.clone() {
            left_group = left_group.with_child(
                TopbarButton::new(
                    Icon::new("sliders")
                        .with_size(18.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_on_click(move || (cb.borrow_mut())())
                .finish(),
            );
        }
        if let Some(cb) = self.on_attach.clone() {
            left_group = left_group.with_child(
                TopbarButton::new(
                    Icon::new("paperclip")
                        .with_size(18.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_on_click(move || (cb.borrow_mut())())
                .finish(),
            );
        }
        footer = footer.with_child(left_group.finish());
        footer = footer.with_child(Spacer::new().finish());
        footer = footer.with_child(
            TopbarButton::new(
                Icon::new("send")
                    .with_size(18.0)
                    .with_theme_color(ColorToken::Accent, app)
                    .finish(),
            )
            .with_on_click(send)
            .finish(),
        );

        column = column.with_child(
            Container::new(footer.finish())
                .with_padding(EdgeInsets::uniform(padding))
                .finish(),
        );

        self.root = Some(
            Container::new(column.finish())
                .with_padding(EdgeInsets::uniform(padding))
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_border(app.theme.color(ColorToken::Border).into())
                .with_corner_radius(radius)
                .finish(),
        );
    }
}

impl Default for ChatComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for ChatComposer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let size = self
            .root
            .as_mut()
            .unwrap()
            .layout(constraint, ctx, app);
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
    use crate::elements::LayoutContext;
    use crate::geometry::vec2f;

    #[test]
    fn composer_layouts_non_zero() {
        let app = AppContext::default();
        let mut composer = ChatComposer::new();
        let size = composer.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn composer_keeps_value_and_attachments() {
        let app = AppContext::default();
        let mut composer = ChatComposer::new()
            .with_value("hello")
            .with_attachments(vec!["doc.md".to_string()]);

        let size = composer.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );

        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
        assert_eq!(composer.value(), "hello");
        assert_eq!(composer.attachments(), &["doc.md".to_string()]);
    }
}
