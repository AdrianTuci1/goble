
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, ChatSidebar, Container, CrossAxisAlignment, Divider,
    EdgeInsets, Element, Expanded, Fill, Flex, Icon, MainAxisSize, RoutineItem, Spacer, Text, TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};


use super::super::{UiActions, UiSnapshot, WorkspaceRouting};

/// The right-hand agent panel, shown as a floating overlay (a `Sheet`) with an
/// X close button instead of the old docked `ChatSidebar`. Reuses the routines
/// list + "+" add-cron button that the sidebar used to render.
pub(crate) fn build_chat_panel_overlay(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);

    let on_close = actions.on_toggle_right_sidebar.clone();
    let header = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Agent panel")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(13.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            TopbarButton::new(
                Icon::new("x")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_on_click(move || (on_close.borrow_mut())())
            .finish(),
        )
        .finish();

    let on_add = actions.on_open_crons.clone();
    let routines = state
        .crons
        .iter()
        .map(|cron| {
            let schedule = if cron.last_run == "unknown" {
                cron.schedule.clone()
            } else {
                cron.last_run.clone()
            };
            RoutineItem::new(cron.name.clone(), schedule, cron.enabled)
        })
        .collect();
    let sidebar = ChatSidebar::new(app)
        .with_routines(routines)
        .with_on_add(move || (on_add.borrow_mut())())
        .finish();

    let body = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Container::new(header).with_padding(EdgeInsets::new(0.0, md, 0.0, md)).finish())
        .with_child(Divider::horizontal().finish())
        .with_child(sidebar)
        .finish();

    Container::new(body)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish()
}

/// The agent's "no API key" error, rendered inline at the top of the chat area
/// instead of floating over the app as a dialog.
///
/// It is a full-width band of rows in the pager idiom, like the rest of the
/// agent surface: no border and no rounded card. The heading carries the error
/// color, and the row below is two columns: what went wrong, and the button
/// that fixes it (the model-provider dialog).
pub(crate) fn build_agent_error(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_config = actions.on_config_llm_key.clone();
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let columns = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(md)
        .with_child(
            Expanded::new(
                Text::new("Failed to use agent. Configure your API keys before using agent.")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish(),
        )
        .with_child(
            Button::new(Text::new("Edit API keys").finish())
                .with_variant(ButtonVariant::Primary)
                .with_on_click(move || (on_config.borrow_mut())())
                .finish(),
        )
        .finish();
    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        .with_child(
            Text::new("No API key configured")
                .with_theme_color(ColorToken::Error, app)
                .with_font_size(13.0)
                .finish(),
        )
        .with_child(columns)
        .finish();
    Container::new(body)
        // A full-width band of rows: no border and no rounded card.
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::uniform(md))
        .finish()
}

/// First-run compact getting-started hint, shown on the chat workspace once the
/// workspace choice is made. Dismissible; the dismissal survives a restart via
/// the persisted onboarding flag.
pub(crate) fn build_onboarding_tip(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_dismiss = actions.on_dismiss_onboarding_tip.clone();
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let tip = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Icon::new("info")
                .with_size(16.0)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
        .with_child(
            Text::new("You're all set. Use Ctrl/Cmd+Arrows to move between panes, Ctrl/Cmd+Space to split, and Cmd+K for commands.")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(
            Button::new(Text::new("Got it").finish())
                .with_variant(ButtonVariant::Ghost)
                .with_on_click(move || (on_dismiss.borrow_mut())())
                .finish(),
        )
        .finish();
    Container::new(tip)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::new(md, sm, md, sm))
        .finish()
}

/// First-run choice prompt: pick where the agent should run.
pub(crate) fn build_workspace_choice(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let local = {
        let on_choose = actions.on_choose_workspace.clone();
        Button::new(Text::new("Local").finish())
            .with_variant(ButtonVariant::Primary)
            .with_on_click(move || (on_choose.borrow_mut())(WorkspaceRouting::Local))
            .finish()
    };
    let remote = {
        let on_choose = actions.on_choose_workspace.clone();
        Button::new(Text::new("Remote").finish())
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || (on_choose.borrow_mut())(WorkspaceRouting::Remote))
            .finish()
    };
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(md)
        .with_child(
            Text::new("Run the agent locally or on a remote worker?")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(local)
        .with_child(remote)
        .finish();
    Container::new(row)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(md))
        .finish()
}

#[cfg(test)]
mod error_notice_tests {
    use super::*;
    use crate::media::MediaState;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::state::UiState;
    use goble_ui::elements::AppContext;
    use goble_ui::geometry::vec2f;
    use goble_ui::platform::WindowControl;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::{command_counts, render_element};

    /// R13: the last bordered card is gone. The inline key error is a band of
    /// rows, so it paints no border and no corner radius.
    #[test]
    fn the_error_notice_is_a_band_not_a_bordered_card() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(UiState::mock()));
        let actions = crate::actions::make_actions(
            state,
            None,
            Rc::new(RefCell::new(MediaState::mock())),
            WindowControl::default(),
            Rc::new(RefCell::new(1.0)),
        );

        let mut notice = build_agent_error(&app, &actions);
        let commands = render_element(&mut notice, vec2f(600.0, 200.0), &app);
        let counts = command_counts(&commands);

        assert_eq!(
            counts.stroke_rect, 0,
            "the error notice paints a band, not a bordered card"
        );
        let band_radius = commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                color,
                corner_radius,
                ..
            } if *color == app.theme.color(ColorToken::SurfaceRaised) => Some(*corner_radius),
            _ => None,
        });
        assert_eq!(
            band_radius,
            Some(0.0),
            "the error notice band has no corner radius"
        );
    }
}
