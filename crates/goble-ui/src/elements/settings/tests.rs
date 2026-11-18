//! Element tests for the settings surface: the measured geometry from the
//! parity document (rail 200, rows 32, card radius 12, label 14 / hint 12), the
//! square switch, the write-only secret mask, and the dark/light token paths.

use crate::color::ColorU;

use crate::elements::settings::{
    row::SettingsRowStatus, SettingsButton, SettingsFold, SettingsNav, SettingsNavGroup,
    SettingsNavItem, SettingsNotice, SettingsRow, SettingsSecretField, SettingsSection,
    SettingsSelect, SettingsSlider, SettingsTextField, SettingsToggle, SETTINGS_GROUP_RADIUS,
    SETTINGS_RAIL_WIDTH, SETTINGS_ROW_HINT_SIZE, SETTINGS_ROW_LABEL_SIZE, SETTINGS_TOGGLE_HEIGHT,
    SETTINGS_TOGGLE_WIDTH,
};
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint, Text};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, RectF};
use crate::render::RenderCommand;
use crate::test_util::render_element;
use crate::theme::{ColorToken, Theme};

fn app(theme: Theme) -> AppContext {
    let mut app = AppContext::default();
    app.theme = theme;
    app
}

fn dark() -> AppContext {
    app(Theme::dark())
}

fn light() -> AppContext {
    app(Theme::light())
}

fn texts(commands: &[RenderCommand]) -> Vec<(String, f32)> {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                text, font_size, ..
            } => Some((text.clone(), *font_size)),
            _ => None,
        })
        .collect()
}

fn drawn(commands: &[RenderCommand], text: &str) -> bool {
    texts(commands).iter().any(|(drawn, _)| drawn == text)
}

fn strokes(commands: &[RenderCommand]) -> Vec<(RectF, ColorU, f32, f32)> {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::StrokeRect {
                rect,
                color,
                width,
                corner_radius,
            } => Some((*rect, *color, *width, *corner_radius)),
            _ => None,
        })
        .collect()
}

fn fills(commands: &[RenderCommand]) -> Vec<(RectF, ColorU, f32)> {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } => Some((*rect, *color, *corner_radius)),
            _ => None,
        })
        .collect()
}

fn press(element: &mut Box<dyn Element>, app: &AppContext, at: crate::geometry::Vector2F) -> bool {
    let mut ctx = crate::elements::EventContext::default();
    let down = DispatchedEvent::MouseDown {
        position: at,
        button: 0,
    };
    let up = DispatchedEvent::MouseUp {
        position: at,
        button: 0,
    };
    let a = element.dispatch_event(&down, &mut ctx, app);
    let _ = element.dispatch_event(&up, &mut ctx, app);
    a
}

fn layout_and_paint(
    element: &mut Box<dyn Element>,
    app: &AppContext,
    size: crate::geometry::Vector2F,
) -> Vec<RenderCommand> {
    render_element(element, size, app)
}

/// The rail is T3's 200px: the element claims the width, the row is 32px, its
/// label is 14px, and the selected row's fill is a `Selected` band.
#[test]
fn the_rail_is_200px_with_32px_rows_and_a_14px_label() {
    let app = dark();
    let mut nav: Box<dyn Element> = Box::new(
        SettingsNav::new(vec![SettingsNavGroup::new("").with_items([
            SettingsNavItem::new("General"),
            SettingsNavItem::new("Appearance"),
        ])])
        .with_selected(0),
    );
    let commands = layout_and_paint(&mut nav, &app, vec2f(1024.0, 640.0));
    assert_eq!(
        nav.size().unwrap().x,
        SETTINGS_RAIL_WIDTH,
        "the rail claims its own 200px"
    );
    let selected = app.theme.color(ColorToken::Selected);
    let band = fills(&commands)
        .into_iter()
        .find(|(rect, color, _)| *color == selected && (rect.height() - 32.0).abs() < 0.5)
        .expect("the selected rail row is a 32px Selected fill");
    assert!((band.0.width() - (SETTINGS_RAIL_WIDTH - 16.0)).abs() < 0.5);
    let labels = texts(&commands);
    assert!(
        labels
            .iter()
            .any(|(text, size)| text == "Appearance" && (*size - 14.0).abs() < 0.01),
        "the rail row label is 14px: {labels:?}"
    );
}

/// A focused rail row carries the 2px `Focus` ring; the selected fill is still
/// underneath it (the ring is drawn over, not instead of, the fill).
#[test]
fn a_focused_rail_row_carries_a_2px_focus_ring() {
    let app = dark();
    let mut nav: Box<dyn Element> = Box::new(
        SettingsNav::new(vec![SettingsNavGroup::new("").with_items([
            SettingsNavItem::new("General"),
            SettingsNavItem::new("Appearance"),
        ])])
        .with_selected(1)
        .with_focused(Some(1)),
    );
    let commands = layout_and_paint(&mut nav, &app, vec2f(1024.0, 640.0));
    let focus = app.theme.color(ColorToken::Focus);
    let ring = strokes(&commands)
        .into_iter()
        .find(|(_, color, width, _)| *color == focus && (*width - 2.0).abs() < 0.01)
        .expect("the focused rail row is ringed in Focus at 2px");
    assert!(
        (ring.0.height() - SETTINGS_NAV_ROW_HEIGHT_PLUS_INSET).abs() < 0.5,
        "the ring is the 32px row inset by 2: {:?}",
        ring.0
    );
    let selected = app.theme.color(ColorToken::Selected);
    assert!(
        fills(&commands)
            .iter()
            .any(|(_, color, _)| *color == selected),
        "the selected fill is still painted under the ring"
    );
}

/// A row's label is 14/500 and its description 12px: T3's `text-sm
/// font-medium` / `text-xs`.
#[test]
fn a_row_has_a_14px_label_and_a_12px_description() {
    let app = dark();
    let mut row: Box<dyn Element> = Box::new(
        SettingsRow::new(&app, "Invert scrolling", "Natural scrolling for the wheel.")
            .with_control(SettingsToggle::new(false).finish()),
    );
    let commands = layout_and_paint(&mut row, &app, vec2f(700.0, 200.0));
    let labels = texts(&commands);
    assert!(labels
        .iter()
        .any(|(text, size)| text == "Invert scrolling"
            && (*size - SETTINGS_ROW_LABEL_SIZE).abs() < 0.01));
    assert!(labels
        .iter()
        .any(|(text, size)| text == "Natural scrolling for the wheel."
            && (*size - SETTINGS_ROW_HINT_SIZE).abs() < 0.01));
}

/// A section is a 12px-radius card with a 1px border, and it draws exactly
/// `n - 1` rules between `n` rows (none at the card's edges).
#[test]
fn a_section_card_has_a_12px_radius_and_one_rule_between_rows() {
    let app = dark();
    let mut section: Box<dyn Element> = Box::new(
        SettingsSection::new("Interface")
            .with_row(SettingsRow::new(&app, "First", "").finish())
            .with_row(SettingsRow::new(&app, "Second", "").finish())
            .with_row(SettingsRow::new(&app, "Third", "").finish()),
    );
    let commands = layout_and_paint(&mut section, &app, vec2f(700.0, 400.0));
    let card = strokes(&commands)
        .into_iter()
        .find(|(_, _, width, radius)| {
            (*width - 1.0).abs() < 0.01 && (*radius - SETTINGS_GROUP_RADIUS).abs() < 0.01
        })
        .expect("the group is a 1px stroke at radius 12");
    assert!(card.0.width() > 600.0);
    // The rules are 1px fills across the card's inner width; count the ones
    // that are wider than the row label and shorter than the card's top edge.
    let rules = fills(&commands)
        .into_iter()
        .filter(|(rect, _, _)| (rect.height() - 1.0).abs() < 0.01 && rect.width() > 600.0)
        .count();
    assert_eq!(rules, 2, "n - 1 rules for n rows");
}

/// The card's fill and border come from tokens: the dark card sits on a dark
/// page and the light card on a light page.
#[test]
fn the_group_card_resolves_in_both_themes() {
    let mut dark_section: Box<dyn Element> = Box::new(
        SettingsSection::new("Interface").with_row(SettingsRow::new(&dark(), "Row", "").finish()),
    );
    let dark_commands = layout_and_paint(&mut dark_section, &dark(), vec2f(700.0, 300.0));
    let mut light_section: Box<dyn Element> = Box::new(
        SettingsSection::new("Interface").with_row(SettingsRow::new(&light(), "Row", "").finish()),
    );
    let light_commands = layout_and_paint(&mut light_section, &light(), vec2f(700.0, 300.0));
    let dark_card = fills(&dark_commands)
        .into_iter()
        .find(|(rect, _, radius)| {
            (*radius - SETTINGS_GROUP_RADIUS).abs() < 0.01 && rect.width() > 600.0
        })
        .map(|(_, color, _)| color);
    let light_card = fills(&light_commands)
        .into_iter()
        .find(|(rect, _, radius)| {
            (*radius - SETTINGS_GROUP_RADIUS).abs() < 0.01 && rect.width() > 600.0
        })
        .map(|(_, color, _)| color);
    assert!(dark_card.is_some() && light_card.is_some());
    assert_ne!(dark_card, light_card, "the card fill follows the theme");
}

/// The switch is square: the track and the thumb paint with no corner radius,
/// and the checked track is the accent.
#[test]
fn the_switch_is_square() {
    let app = dark();
    let mut toggle: Box<dyn Element> = Box::new(SettingsToggle::new(false));
    let commands = layout_and_paint(&mut toggle, &app, vec2f(100.0, 40.0));
    let track = fills(&commands)
        .into_iter()
        .find(|(rect, _, _)| (rect.width() - SETTINGS_TOGGLE_WIDTH).abs() < 0.01)
        .expect("the track is 32 wide");
    assert!((track.0.height() - SETTINGS_TOGGLE_HEIGHT).abs() < 0.01);
    assert_eq!(track.2, 0.0, "the square switch has right angles");
    assert_eq!(toggle.size().unwrap(), vec2f(32.0, 18.0));

    let mut checked: Box<dyn Element> = Box::new(SettingsToggle::new(true));
    let commands = layout_and_paint(&mut checked, &app, vec2f(100.0, 40.0));
    assert!(
        fills(&commands)
            .iter()
            .any(|(_, color, _)| *color == app.theme.color(ColorToken::Accent)),
        "the checked track uses the accent token"
    );
    assert!(
        fills(&commands).iter().all(|(_, _, radius)| *radius == 0.0),
        "no part of the square switch rounds"
    );
}

/// Clicking the switch toggles it and runs the row's callback.
#[test]
fn clicking_the_switch_toggles_it() {
    let app = dark();
    let seen = std::rc::Rc::new(std::cell::Cell::new(0));
    let seen_cb = seen.clone();
    let mut toggle: Box<dyn Element> =
        Box::new(SettingsToggle::new(false).with_on_change(move || seen_cb.set(seen_cb.get() + 1)));
    let _ = layout_and_paint(&mut toggle, &app, vec2f(100.0, 40.0));
    assert!(press(&mut toggle, &app, vec2f(5.0, 5.0)));
    assert_eq!(seen.get(), 1);
}

/// The slider paints its track, its filled portion and its knob at the value's
/// fraction.
#[test]
fn the_slider_paints_a_knob_at_the_value_fraction() {
    let app = dark();
    let mut slider: Box<dyn Element> =
        Box::new(SettingsSlider::new(50.0, 0.0, 100.0).with_width(200.0));
    let commands = layout_and_paint(&mut slider, &app, vec2f(300.0, 40.0));
    let track = fills(&commands)
        .into_iter()
        .find(|(rect, _, _)| (rect.height() - 4.0).abs() < 0.01)
        .expect("a 4px track");
    assert!((track.0.width() - 200.0).abs() < 0.01);
    let knob = fills(&commands)
        .into_iter()
        .find(|(rect, _, _)| (rect.width() - 14.0).abs() < 0.01)
        .expect("a 14px knob");
    assert!(
        (knob.0.center().x - (track.0.min_x() + 100.0)).abs() < 1.0,
        "the knob sits at 50%: {knob:?}"
    );
}

/// The secret field is write-only: the draft paints as dots and the raw value
/// never reaches a text command. A configured value shows its placeholder.
#[test]
fn the_secret_field_never_paints_the_secret() {
    let app = dark();
    let mut field: Box<dyn Element> = Box::new(
        SettingsSecretField::new()
            .with_draft("sk-super-secret")
            .with_configured(true)
            .with_width(240.0),
    );
    let commands = layout_and_paint(&mut field, &app, vec2f(300.0, 40.0));
    let labels = texts(&commands);
    assert!(
        !labels
            .iter()
            .any(|(text, _)| text.contains("sk-super-secret") || text.contains("super")),
        "the value is never drawn: {labels:?}"
    );
    assert!(
        labels
            .iter()
            .any(|(text, _)| text == &"\u{2022}".repeat("sk-super-secret".len())),
        "the draft paints as one dot per character: {labels:?}"
    );

    let mut configured: Box<dyn Element> = Box::new(
        SettingsSecretField::new()
            .with_configured(true)
            .with_width(240.0),
    );
    let commands = layout_and_paint(&mut configured, &app, vec2f(300.0, 40.0));
    assert!(drawn(&commands, "Configured — type to replace"));
}

/// A text field is a 32px control with a border; its value draws at the
/// control's 13px size.
#[test]
fn a_text_field_draws_its_value_inside_a_bordered_control() {
    let app = dark();
    let mut field: Box<dyn Element> = Box::new(
        SettingsTextField::new()
            .with_value("deepseek")
            .with_width(240.0),
    );
    let commands = layout_and_paint(&mut field, &app, vec2f(300.0, 40.0));
    assert_eq!(field.size().unwrap().y, 32.0);
    assert!(drawn(&commands, "deepseek"));
    assert!(
        strokes(&commands).iter().any(
            |(_, _, width, radius)| (*width - 1.0).abs() < 0.01 && (*radius - 8.0).abs() < 0.01
        ),
        "the field is an 8px-radius 1px control"
    );
}

/// The select cycles its options on a pointer press and runs the callback.
#[test]
fn the_select_cycles_on_a_press() {
    let app = dark();
    let chosen = std::rc::Rc::new(std::cell::Cell::new(usize::MAX));
    let chosen_cb = chosen.clone();
    let mut select: Box<dyn Element> = Box::new(
        SettingsSelect::new(["Manual", "Plan", "Auto-review"])
            .with_selected(0)
            .with_on_change(move |index| chosen_cb.set(index)),
    );
    let commands = layout_and_paint(&mut select, &app, vec2f(300.0, 40.0));
    assert!(drawn(&commands, "Manual"));
    assert!(press(&mut select, &app, vec2f(20.0, 10.0)));
    assert_eq!(chosen.get(), 1);
}

/// A notice is a tinted card with its sentence, and an unavailable status line
/// says so without a colour that reads as an error.
#[test]
fn the_notice_tints_and_reads() {
    let app = dark();
    let mut notice: Box<dyn Element> = Box::new(SettingsNotice::error(
        "config.toml changed on disk — reload to continue.",
    ));
    let commands = layout_and_paint(&mut notice, &app, vec2f(700.0, 60.0));
    assert!(drawn(
        &commands,
        "config.toml changed on disk — reload to continue."
    ));
    let error = app.theme.color(ColorToken::Error);
    let tinted = fills(&commands)
        .into_iter()
        .any(|(_, color, _)| color != app.theme.color(ColorToken::Bg) && color != error);
    assert!(tinted, "the notice paints a tinted fill");
}

/// A disabled row's status line carries the honest sentence, and the row draws
/// its content dimmed.
#[test]
fn a_disabled_row_carries_its_own_explanation() {
    let app = dark();
    let mut row: Box<dyn Element> = Box::new(
        SettingsRow::new(&app, "Sign in with Codex", "Uses your Codex subscription.")
            .with_disabled(true)
            .with_status(SettingsRowStatus::unavailable(
                "Not available in this build · no OAuth sign-in.",
            )),
    );
    let commands = layout_and_paint(&mut row, &app, vec2f(700.0, 200.0));
    assert!(drawn(
        &commands,
        "Not available in this build · no OAuth sign-in."
    ));
    // The label is dimmed toward the background rather than drawn at full text
    // contrast.
    let full = app.theme.color(ColorToken::Text);
    let (_, color) = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, color, .. } if text == "Sign in with Codex" => {
                Some((text.clone(), *color))
            }
            _ => None,
        })
        .expect("the label draws");
    assert_ne!(color, full, "a disabled label is dimmed");
}

/// A fold is closed by default (no body rows drawn) and open draws them.
#[test]
fn the_fold_draws_its_rows_only_when_open() {
    let app = dark();
    let body = |app: &AppContext| SettingsRow::new(app, "Compiled with wgpu", "").finish();
    let mut closed: Box<dyn Element> = Box::new(
        SettingsFold::new("Diagnostics")
            .with_summary("local")
            .with_row(body(&app)),
    );
    let commands = layout_and_paint(&mut closed, &app, vec2f(700.0, 300.0));
    assert!(!drawn(&commands, "Compiled with wgpu"));
    let mut open: Box<dyn Element> = Box::new(
        SettingsFold::new("Diagnostics")
            .with_summary("local")
            .with_open(true)
            .with_row(body(&app)),
    );
    let commands = layout_and_paint(&mut open, &app, vec2f(700.0, 300.0));
    assert!(drawn(&commands, "Compiled with wgpu"));
}

/// A row's body (a field editor, a model table) draws under the label with the
/// tight `pt-3 pb-1` padding rather than a second row.
#[test]
fn a_row_body_draws_under_its_label() {
    let app = dark();
    let body: Box<dyn Element> = Text::new("inner body").with_font_size(12.0).finish();
    let mut row: Box<dyn Element> =
        Box::new(SettingsRow::new(&app, "Models", "Two configured.").with_body(body));
    let commands = layout_and_paint(&mut row, &app, vec2f(700.0, 300.0));
    assert!(drawn(&commands, "inner body"));
    let label = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == "Models" => Some(*origin),
            _ => None,
        })
        .expect("the label draws");
    let inner = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == "inner body" => Some(*origin),
            _ => None,
        })
        .expect("the body draws");
    assert!(inner.y > label.y + 20.0, "the body sits under the label");
}

/// A focused row draws its 2px ring at the group's radius.
#[test]
fn a_focused_row_draws_a_2px_ring() {
    let app = dark();
    let mut row: Box<dyn Element> = Box::new(SettingsRow::new(&app, "Row", "").with_focused(true));
    let commands = layout_and_paint(&mut row, &app, vec2f(700.0, 200.0));
    assert!(
        strokes(&commands).iter().any(|(_, color, width, radius)| {
            *color == app.theme.color(ColorToken::Focus)
                && (*width - 2.0).abs() < 0.01
                && (*radius - SETTINGS_GROUP_RADIUS).abs() < 0.01
        }),
        "the focused row is ringed at 2px/radius 12"
    );
}

/// Unused constant kept in the test so the 32px row geometry is pinned in one
/// place (the ring is inset 2 on a 32px row, so its own height is 28).
const SETTINGS_NAV_ROW_HEIGHT_PLUS_INSET: f32 = 28.0;

/// A row with a body is taller than a row without one (the editor's space).
#[test]
fn a_row_with_a_body_is_taller_than_a_plain_row() {
    let app = dark();
    let mut plain: Box<dyn Element> = SettingsRow::new(&app, "Plain", "").finish();
    let plain_size = {
        let mut ctx = LayoutContext::default();
        plain.layout(SizeConstraint::loose(vec2f(700.0, 300.0)), &mut ctx, &app)
    };
    let body: Box<dyn Element> = Text::new("body").with_font_size(12.0).finish();
    let mut with_body: Box<dyn Element> =
        SettingsRow::new(&app, "Body", "").with_body(body).finish();
    let body_size = {
        let mut ctx = LayoutContext::default();
        with_body.layout(SizeConstraint::loose(vec2f(700.0, 300.0)), &mut ctx, &app)
    };
    assert!(body_size.y > plain_size.y);
}

/// A fold section keeps its heading and hides its rows until opened: the
/// heading is the trigger (chevron + title + summary), pressing it runs the
/// section's toggle callback, and the open section draws the group card.
#[test]
fn a_fold_section_hides_its_rows_until_opened() {
    use std::cell::Cell;
    use std::rc::Rc;

    let app = dark();
    let opened = Rc::new(Cell::new(false));
    let flag = Rc::clone(&opened);
    let mut closed: Box<dyn Element> = Box::new(
        SettingsSection::new("Agent CLIs")
            .with_description("installed · claude")
            .with_fold(false)
            .with_on_fold_toggle(move || flag.set(true))
            .compact_header()
            .with_row(SettingsRow::new(&app, "Launch command", "claude").finish()),
    );
    let commands = render_element(&mut closed, vec2f(640.0, 300.0), &app);
    assert!(
        !drawn(&commands, "Launch command"),
        "a closed fold draws no rows"
    );
    assert!(
        drawn(&commands, "installed · claude"),
        "the summary is in the heading"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "chevron-right"
        )),
        "the closed fold's chevron points right"
    );
    assert!(
        press(&mut closed, &app, vec2f(120.0, 18.0)),
        "the heading claims the press"
    );
    assert!(opened.get(), "the heading runs the fold's toggle");

    let mut open: Box<dyn Element> = Box::new(
        SettingsSection::new("Agent CLIs")
            .with_description("installed · claude")
            .with_fold(true)
            .compact_header()
            .with_row(SettingsRow::new(&app, "Launch command", "claude").finish()),
    );
    let commands = render_element(&mut open, vec2f(640.0, 300.0), &app);
    assert!(
        drawn(&commands, "Launch command"),
        "the open fold draws its rows"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "chevron-down"
        )),
        "the open fold's chevron points down"
    );
}

/// A row's swatch is a real 14px colour chip before the label: the Appearance
/// page's channel and palette rows are readable at a glance.
#[test]
fn a_row_swatch_paints_its_colour() {
    let app = dark();
    let red = ColorU::new(255, 0, 0, 255);
    let mut row: Box<dyn Element> = SettingsRow::new(&app, "Accent", "The accent channel.")
        .with_swatch(red)
        .finish();
    let commands = render_element(&mut row, vec2f(640.0, 80.0), &app);
    let chip = fills(&commands).into_iter().find(|(rect, color, radius)| {
        *color == red && (rect.width() - 14.0).abs() < 0.5 && (rect.height() - 14.0).abs() < 0.5
    });
    let (rect, _, radius) = chip.expect("the red 14px swatch is drawn");
    assert_eq!(radius, 4.0, "the chip's corner radius");
    assert!(
        rect.min_x() < rect.min_y() + 100.0,
        "the chip is in the row: {rect:?}"
    );
}
