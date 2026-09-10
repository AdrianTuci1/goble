//! Agent crons drawer: scheduled tasks list + create.

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Fill, Flex, Icon, MainAxisSize, Rect, Scrollable, Spacer, Text, TopbarButton,
};
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{CronEntry, UiActions, UiSnapshot};

/// Right-anchored crons drawer content: header with close, scrollable list of
/// scheduled tasks, and a create button at the bottom.
pub fn build_crons_drawer(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_close_crons = actions.on_close_crons.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_close_crons.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Agent crons")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(close_button)
        .finish();

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    if state.crons.is_empty() {
        list = list.with_child(
            Text::new("No scheduled tasks yet.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for cron in &state.crons {
        list = list.with_child(build_cron_row(app, cron, actions));
    }

    let on_cron_create = actions.on_cron_create.clone();
    let create_button = Button::new(Text::new("New cron").finish())
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (on_cron_create.borrow_mut())())
        .finish();

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(Scrollable::new(list.finish(), Axis::Vertical).finish());
    column = column.with_child(create_button);

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// One scheduled task row: status dot, name + schedule, trigger and delete.
fn build_cron_row(app: &AppContext, cron: &CronEntry, actions: &UiActions) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let radius = app.theme.radius_px();

    let status_color = if cron.enabled {
        ColorToken::Success
    } else {
        ColorToken::Muted
    };
    let status = if cron.enabled { "enabled" } else { "disabled" };

    let info = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(2.0)
        .with_child(
            Text::new(cron.name.clone())
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!(
                "{} · {} · last run {}",
                cron.schedule, status, cron.last_run
            ))
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
        )
        .finish();

    let on_cron_trigger = actions.on_cron_trigger.clone();
    let trigger_button = TopbarButton::new(
        Icon::new("refresh")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click({
        let id = cron.id.clone();
        move || (on_cron_trigger.borrow_mut())(id.clone())
    })
    .finish();

    let on_cron_delete = actions.on_cron_delete.clone();
    let delete_button = TopbarButton::new(
        Icon::new("trash")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click({
        let id = cron.id.clone();
        move || (on_cron_delete.borrow_mut())(id.clone())
    })
    .finish();

    Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(
                Container::new(Rect::new().with_size(vec2f(8.0, 8.0)).finish())
                    .with_background(Fill::Solid(app.theme.color(status_color)))
                    .with_corner_radius(4.0)
                    .finish(),
            )
            .with_child(info)
            .with_child(Spacer::new().finish())
            .with_child(trigger_button)
            .with_child(delete_button)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_corner_radius(radius)
    .with_padding(EdgeInsets::uniform(spacing))
    .finish()
}
