use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Border, Button, ButtonVariant, Checkbox, Container, CrossAxisAlignment,
    EdgeInsets, Element, Fill, Flex, Icon, LayoutContext, MainAxisAlignment, PaintContext, Point,
    SizeConstraint, Text, TextArea,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

/// The renderable state of a pending ask: the agent's question and its suggested
/// quick replies. Mirrors `PendingAsk` from goble-core minus the internal ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskUserUi {
    pub question: String,
    pub quick_replies: Vec<String>,
}

impl AskUserUi {
    pub fn new(question: impl Into<String>, quick_replies: Vec<String>) -> Self {
        Self {
            question: question.into(),
            quick_replies,
        }
    }
}

/// An inline card that renders an "ask the user" interaction in the conversation
/// stream (warp-new `AskUserQuestion` block). It is not pinned to the bottom:
/// it sits among the chat messages and scrolls with them.
///
/// Offers single-select quick replies, a free-text answer and a masked
/// credential field. Submitting calls `on_answer` with the composed response;
/// "Skip" calls `on_skip`.
pub struct AskUserCard {
    question: String,
    quick_replies: Vec<String>,
    selected: Rc<RefCell<Option<usize>>>,
    free_text: Rc<RefCell<String>>,
    credential_name: Rc<RefCell<String>>,
    credential: Rc<RefCell<String>>,
    focused: Rc<RefCell<bool>>,
    on_answer: Option<Rc<RefCell<dyn FnMut(String, Option<(String, String)>) + 'static>>>,
    on_skip: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl AskUserCard {
    pub fn new(question: impl Into<String>, quick_replies: Vec<String>) -> Self {
        Self {
            question: question.into(),
            quick_replies,
            selected: Rc::new(RefCell::new(None)),
            free_text: Rc::new(RefCell::new(String::new())),
            credential_name: Rc::new(RefCell::new(String::new())),
            credential: Rc::new(RefCell::new(String::new())),
            focused: Rc::new(RefCell::new(false)),
            on_answer: None,
            on_skip: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_answer<F: FnMut(String, Option<(String, String)>) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_answer = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_skip<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_skip = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Compose the answer delivered to the harness. The credential, when present,
    /// is returned separately as `(name, value)` so the secret never enters the
    /// answer string (and thus never the transcript); the harness stores it by
    /// name and references it by name.
    #[cfg(test)]
    fn compose(&self) -> (String, Option<(String, String)>) {
        let mut parts = Vec::new();
        if let Some(idx) = *self.selected.borrow() {
            if let Some(label) = self.quick_replies.get(idx) {
                parts.push(label.clone());
            }
        }
        let ft = self.free_text.borrow().trim().to_string();
        if !ft.is_empty() {
            parts.push(ft);
        }
        let answer = parts.join("\n");
        let cval = self.credential.borrow().trim().to_string();
        if cval.is_empty() {
            return (answer, None);
        }
        let cname = self.credential_name.borrow().trim().to_string();
        (answer, Some((cname, cval)))
    }

    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);

        let on_answer = self.on_answer.clone();
        let on_skip = self.on_skip.clone();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm);

        // Header: label + warning icon + Skip.
        let mut header = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        let title = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(
                Icon::new("stop")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Warning, app)
                    .finish(),
            )
            .with_child(
                Text::new("Agent asks")
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(13.0)
                    .finish(),
            );
        header = header.with_child(title.finish());
        if let Some(cb) = on_skip.clone() {
            let skip = Button::new(
                Text::new("Skip")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || (cb.borrow_mut())())
            .finish();
            header = header.with_child(skip);
        }
        column = column.with_child(header.finish());

        // Question text.
        column = column.with_child(
            Text::new(self.question.clone())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        );

        // Quick replies as single-select checkbox rows.
        if !self.quick_replies.is_empty() {
            let mut option_column = Flex::column().with_spacing(4.0);
            let selected = self.selected.clone();
            let free_text = self.free_text.clone();
            let quick_replies = self.quick_replies.clone();
            for (index, label) in quick_replies.iter().enumerate() {
                let is_selected = *self.selected.borrow() == Some(index);
                let sel = selected.clone();
                let ft = free_text.clone();
                let checkbox = Checkbox::new()
                    .with_label(
                        Text::new(label.clone())
                            .with_theme_color(ColorToken::Text, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_checked(is_selected)
                    .with_on_change(move |checked| {
                        if checked {
                            *sel.borrow_mut() = Some(index);
                            ft.borrow_mut().clear();
                        } else {
                            *sel.borrow_mut() = None;
                        }
                    })
                    .finish();
                option_column = option_column.with_child(checkbox);
            }
            column = column.with_child(option_column.finish());
        }

        // Free-text answer.
        let free_text_value = self.free_text.borrow().clone();
        let free_text = self.free_text.clone();
        column = column.with_child(
            TextArea::new()
                .with_value(free_text_value)
                .with_placeholder("Or type an answer…")
                .with_min_height(44.0)
                .with_focused(*self.focused.borrow())
                .with_on_change(move |text| {
                    *free_text.borrow_mut() = text;
                })
                .finish(),
        );

        // Credential (optional): a name + masked value. The value is stored by
        // name so the raw secret never enters the transcript.
        let credential_name_value = self.credential_name.borrow().clone();
        let credential_name = self.credential_name.clone();
        let credential_value = self.credential.borrow().clone();
        let credential = self.credential.clone();
        let credential_block = Flex::column()
            .with_spacing(4.0)
            .with_child(
                TextArea::new()
                    .with_value(credential_name_value)
                    .with_placeholder("Credential name (e.g. github_token)…")
                    .with_min_height(36.0)
                    .with_on_change(move |text| {
                        *credential_name.borrow_mut() = text;
                    })
                    .finish(),
            )
            .with_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(
                        Icon::new("key")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        TextArea::new()
                            .with_value(credential_value)
                            .with_placeholder("Credential (optional)…")
                            .with_min_height(40.0)
                            .with_masked(true)
                            .with_on_change(move |text| {
                                *credential.borrow_mut() = text;
                            })
                            .finish(),
                    )
                    .finish(),
            );
        column = column.with_child(credential_block.finish());

        // Submit.
        if let Some(cb) = on_answer {
            let selected = self.selected.clone();
            let free_text = self.free_text.clone();
            let credential_name = self.credential_name.clone();
            let credential = self.credential.clone();
            let quick_replies = self.quick_replies.clone();
            let submit = Button::new(
                Text::new("Send answer")
                    .with_theme_color(ColorToken::Bg, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || {
                let mut parts = Vec::new();
                if let Some(idx) = *selected.borrow() {
                    if let Some(label) = quick_replies.get(idx) {
                        parts.push(label.clone());
                    }
                }
                let ft = free_text.borrow().trim().to_string();
                if !ft.is_empty() {
                    parts.push(ft);
                }
                let answer = parts.join("\n");
                let cname = credential_name.borrow().trim().to_string();
                let cval = credential.borrow().trim().to_string();
                let cred = if cval.is_empty() { None } else { Some((cname, cval)) };
                (cb.borrow_mut())(answer, cred);
            })
            .finish();
            column = column.with_child(
                Container::new(
                    Flex::row()
                        .with_main_axis_alignment(MainAxisAlignment::End)
                        .with_child(submit)
                        .finish(),
                )
                .finish(),
            );
        }

        let card = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(
                Border::all(1.0).with_border_fill(Fill::Solid(app.theme.color(ColorToken::Border))),
            )
            .with_padding(EdgeInsets::uniform(md))
            .with_corner_radius(8.0)
            .finish();
        self.root = Some(card);
    }
}

impl Default for AskUserCard {
    fn default() -> Self {
        Self::new(String::new(), Vec::new())
    }
}

impl Element for AskUserCard {
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
    use crate::elements::{LayoutContext, PaintContext};
    use crate::geometry::vec2f;

    #[test]
    fn ask_user_card_layouts_and_paints() {
        let app = AppContext::default();
        let mut card = AskUserCard::new(
            "Which database should I query?".to_string(),
            vec!["Postgres".to_string(), "SQLite".to_string()],
        );
        let size = card.layout(
            SizeConstraint::loose(vec2f(520.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);

        card.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
    }

    #[test]
    fn ask_user_card_composes_response_from_selection() {
        let app = AppContext::default();
        let mut card = AskUserCard::new(
            "Pick one".to_string(),
            vec!["A".to_string(), "B".to_string()],
        );
        card.layout(
            SizeConstraint::loose(vec2f(520.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        // Simulate selecting option 0 via the shared state.
        *card.selected.borrow_mut() = Some(0);
        let (answer, credential) = card.compose();
        assert_eq!(answer, "A");
        assert!(credential.is_none());
    }

    #[test]
    fn ask_user_card_threads_credential_out_of_answer() {
        let app = AppContext::default();
        let mut card = AskUserCard::new("What's the token?".to_string(), vec![]);
        card.layout(
            SizeConstraint::loose(vec2f(520.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        *card.credential_name.borrow_mut() = "github_token".to_string();
        *card.credential.borrow_mut() = "ghs_secret".to_string();
        let (answer, credential) = card.compose();
        // The secret is not embedded in the answer text.
        assert!(!answer.contains("ghs_secret"));
        assert_eq!(credential, Some(("github_token".to_string(), "ghs_secret".to_string())));
    }
}
