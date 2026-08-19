use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Button, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Fill,
    Flex, Label, LabelSize, LayoutContext, MainAxisAlignment, PaintContext, Point, Select,
    SelectOption, SizeConstraint, Switch, Text, TextInput,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Profile,
    Llm,
    Appearance,
}

fn nav_item(
    label: impl Into<String>,
    _page: SettingsPage,
    selected: bool,
    app: &AppContext,
    on_navigate: Option<Rc<RefCell<dyn FnMut(SettingsPage) + 'static>>>,
) -> Box<dyn Element> {
    let padding = app.theme.spacing_px(SpacingToken::Md);
    let bg = if selected {
        Fill::Solid(app.theme.color(ColorToken::Selected))
    } else {
        Fill::None
    };
    let root = Container::new(
        Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Text::new(label.into()).finish())
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(padding))
    .with_background(bg)
    .finish();

    // TODO: wire click handling once interactive wrappers support arbitrary elements.
    let _ = on_navigate;
    root
}

fn section(title: impl Into<String>, children: Vec<Box<dyn Element>>, app: &AppContext) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(
        Label::new(title.into())
            .with_size(LabelSize::Sm)
            .finish(),
    );
    for child in children {
        column = column.with_child(child);
    }
    Container::new(column.finish())
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

fn settings_row(
    label: impl Into<String>,
    control: Box<dyn Element>,
    app: &AppContext,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    Container::new(
        Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(Text::new(label.into()).finish())
            .with_child(control)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(spacing))
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .finish()
}

pub struct SettingsView {
    current_page: SettingsPage,
    profile_name: String,
    profile_email: String,
    llm_provider: String,
    llm_model: String,
    dark_mode: bool,
    on_navigate: Option<Rc<RefCell<dyn FnMut(SettingsPage) + 'static>>>,
    on_save_profile: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_save_llm: Option<Rc<RefCell<dyn FnMut(String, String) + 'static>>>,
    on_toggle_dark_mode: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsView {
    pub fn new(current_page: SettingsPage) -> Self {
        Self {
            current_page,
            profile_name: String::new(),
            profile_email: String::new(),
            llm_provider: String::new(),
            llm_model: String::new(),
            dark_mode: false,
            on_navigate: None,
            on_save_profile: None,
            on_save_llm: None,
            on_toggle_dark_mode: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_profile(mut self, name: impl Into<String>, email: impl Into<String>) -> Self {
        self.profile_name = name.into();
        self.profile_email = email.into();
        self
    }

    pub fn with_llm(mut self, provider: impl Into<String>, model: impl Into<String>) -> Self {
        self.llm_provider = provider.into();
        self.llm_model = model.into();
        self
    }

    pub fn with_dark_mode(mut self, enabled: bool) -> Self {
        self.dark_mode = enabled;
        self
    }

    pub fn with_on_navigate<F: FnMut(SettingsPage) + 'static>(mut self, callback: F) -> Self {
        self.on_navigate = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_save_profile<F: FnMut(String, String) + 'static>(mut self, callback: F) -> Self {
        self.on_save_profile = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_save_llm<F: FnMut(String, String) + 'static>(mut self, callback: F) -> Self {
        self.on_save_llm = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_toggle_dark_mode<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_dark_mode = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build_nav(&self, app: &AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        let pages = [
            ("Profile", SettingsPage::Profile),
            ("LLM", SettingsPage::Llm),
            ("Appearance", SettingsPage::Appearance),
        ];
        for (label, page) in pages {
            let selected = self.current_page == page;
            let item = nav_item(
                label,
                page,
                selected,
                app,
                self.on_navigate.clone(),
            );
            column = column.with_child(item);
        }

        Container::new(column.finish())
            .with_padding(EdgeInsets::uniform(spacing))
            .finish()
    }

    fn build_pane(&self, app: &AppContext) -> Box<dyn Element> {
        match self.current_page {
            SettingsPage::Profile => self.build_profile_page(app),
            SettingsPage::Llm => self.build_llm_page(app),
            SettingsPage::Appearance => self.build_appearance_page(app),
        }
    }

    fn build_profile_page(&self, app: &AppContext) -> Box<dyn Element> {
        let name_value = self.profile_name.clone();
        let email_value = self.profile_email.clone();

        let name_input = TextInput::new()
            .with_value(self.profile_name.clone())
            .with_on_change(move |v| {
                let _ = v;
            })
            .finish();
        let email_input = TextInput::new()
            .with_value(self.profile_email.clone())
            .with_on_change(move |v| {
                let _ = v;
            })
            .finish();

        let on_save = self.on_save_profile.clone();
        let save = Button::new(Text::new("Save").finish())
            .with_on_click(move || {
                if let Some(cb) = on_save.as_ref() {
                    (cb.borrow_mut())(name_value.clone(), email_value.clone());
                }
            })
            .finish();

        section(
            "Profile",
            vec![
                settings_row("Name", name_input, app),
                settings_row("Email", email_input, app),
                save,
            ],
            app,
        )
    }

    fn build_llm_page(&self, app: &AppContext) -> Box<dyn Element> {
        let provider_options = vec![
            SelectOption::new("OpenAI", "openai"),
            SelectOption::new("Anthropic", "anthropic"),
            SelectOption::new("Ollama", "ollama"),
            SelectOption::new("DeepSeek", "deepseek"),
            SelectOption::new("OpenRouter", "openrouter"),
        ];
        let selected = provider_options
            .iter()
            .position(|o| o.value == self.llm_provider);

        let model_value = self.llm_model.clone();
        let provider_value = self.llm_provider.clone();

        let mut provider_select = Select::new(provider_options).with_on_change(move |idx| {
            let _ = idx;
        });
        if let Some(idx) = selected {
            provider_select = provider_select.with_selected_index(idx);
        }
        let provider_select = provider_select.finish();

        let model_input = TextInput::new()
            .with_value(self.llm_model.clone())
            .with_placeholder("e.g. gpt-4o")
            .with_on_change(move |v| {
                let _ = v;
            })
            .finish();

        let on_save = self.on_save_llm.clone();
        let save = Button::new(Text::new("Save").finish())
            .with_on_click(move || {
                if let Some(cb) = on_save.as_ref() {
                    (cb.borrow_mut())(provider_value.clone(), model_value.clone());
                }
            })
            .finish();

        section(
            "LLM Provider",
            vec![
                settings_row("Provider", provider_select, app),
                settings_row("Model", model_input, app),
                save,
            ],
            app,
        )
    }

    fn build_appearance_page(&self, app: &AppContext) -> Box<dyn Element> {
        let on_toggle = self.on_toggle_dark_mode.clone();
        let switch = Switch::new()
            .with_checked(self.dark_mode)
            .with_on_change(move |v| {
                if let Some(cb) = on_toggle.as_ref() {
                    (cb.borrow_mut())(v);
                }
            })
            .finish();

        section("Appearance", vec![settings_row("Dark mode", switch, app)], app)
    }

    fn rebuild(&mut self, app: &AppContext, width: f32) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let nav_width = 160.0_f32;
        let pane_width = (width - nav_width - 1.0).max(200.0);

        let nav = self.build_nav(app);
        let pane = self.build_pane(app);
        let pane = crate::elements::ConstrainedBox::new(pane)
            .with_max_width(pane_width)
            .finish();

        let row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                crate::elements::ConstrainedBox::new(nav)
                    .with_max_width(nav_width)
                    .finish(),
            )
            .with_child(Divider::vertical().finish())
            .with_child(pane);

        self.root = Some(
            Container::new(row.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
    }
}

impl Element for SettingsView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app, constraint.max.x);
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

    #[test]
    fn settings_view_layouts() {
        let app = AppContext::default();
        let mut view = SettingsView::new(SettingsPage::Profile)
            .with_profile("Ada", "ada@example.com")
            .with_llm("openai", "gpt-4o");
        let size = view.layout(
            SizeConstraint::loose(vec2f(800.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
