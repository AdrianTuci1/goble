//! Agent crons drawer: scheduled tasks list + create.

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment,
    Divider, EdgeInsets, Element, Fill, Flex, HoverRow, Icon, MainAxisSize, Rect, Scrollable,
    Spacer, Switch, Text, TopbarButton,
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

/// 349/350: the sidebar Automations popover — DSH's automation task list,
/// anchored over the shell, with a real switch per stored workflow and the
/// existing New cron action.
pub fn build_automations_popover(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let on_close = actions.on_toggle_automations.clone();
    let close = TopbarButton::new(
        Icon::new("x")
            .with_size(13.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(24.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();
    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Text::new("Automation tasks")
                .with_font_size(13.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(close)
        .finish();
    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    if state.crons.is_empty() {
        list = list.with_child(
            Text::new("No automation tasks yet.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }
    for cron in &state.crons {
        let toggle_id = cron.id.clone();
        let on_toggle = actions.on_cron_toggle.clone();
        let toggle = Switch::new()
            .with_checked(cron.enabled)
            .with_size(vec2f(34.0, 18.0))
            .with_on_change(move |enabled| (on_toggle.borrow_mut())(toggle_id.clone(), enabled))
            .finish();
        let row = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new(cron.name.clone())
                            .with_font_size(13.0)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(Spacer::new().finish())
                    .with_child(toggle)
                    .finish(),
            )
            .with_child(
                Text::new(format!("{} · last run {}", cron.schedule, cron.last_run))
                    .with_font_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish();
        list = list.with_child(
            Container::new(row)
                .with_padding(EdgeInsets::new(6.0, 4.0, 6.0, 4.0))
                .finish(),
        );
    }
    let on_create = actions.on_cron_create.clone();
    let new_row = HoverRow::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_child(
                Icon::new("plus")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Text::new("New automation")
                    .with_font_size(13.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::new(8.0, 6.0, 8.0, 6.0))
    .with_corner_radius(7.0)
    .with_on_click(move || (on_create.borrow_mut())())
    .finish();
    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing)
        .with_child(header)
        .with_child(Divider::horizontal().finish())
        .with_child(Scrollable::new(list.finish(), Axis::Vertical).finish())
        .with_child(new_row)
        .finish();
    ConstrainedBox::new(
        Container::new(column)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_border(
                goble_ui::elements::Border::all(1.0)
                    .with_border_color(app.theme.color(ColorToken::Border)),
            )
            .with_corner_radius(12.0)
            .with_padding(EdgeInsets::uniform(10.0))
            .finish(),
    )
    .with_width(300.0)
    .with_max_height(420.0)
    .finish()
}
