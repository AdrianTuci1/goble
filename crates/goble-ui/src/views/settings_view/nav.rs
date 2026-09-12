use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Button, ButtonVariant, Container, CrossAxisAlignment, EdgeInsets, Element, Fill,
    Flex, Text,
};
use crate::theme::{ColorToken, SpacingToken};

use super::{SettingsPage, SettingsView};

fn nav_item(
    label: impl Into<String>,
    page: SettingsPage,
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
    let label_text = Text::new(label.into()).finish();
    let on_navigate = on_navigate.clone();
    let button = Button::new(label_text)
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || {
            if let Some(cb) = on_navigate.as_ref() {
                (cb.borrow_mut())(page);
            }
        })
        .finish();

    Container::new(button)
        .with_background(bg)
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(padding))
        .finish()
}

impl SettingsView {
    pub(super) fn build_nav(&self, app: &AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        let pages = [
            ("Profile", SettingsPage::Profile),
            ("LLM", SettingsPage::Llm),
            ("Appearance", SettingsPage::Appearance),
            ("Account", SettingsPage::Account),
            ("Cluster", SettingsPage::Cluster),
            ("Workers", SettingsPage::Workers),
            ("Keys", SettingsPage::Keys),
        ];
        for (label, page) in pages {
            let selected = self.current_page == page;
            let item = nav_item(label, page, selected, app, self.on_navigate.clone());
            column = column.with_child(item);
        }

        Container::new(column.finish())
            .with_padding(EdgeInsets::uniform(spacing))
            .finish()
    }
}
