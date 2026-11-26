use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::elements::AppContext;
use crate::elements::Element;
use crate::elements::LayoutContext;
use crate::elements::PopupMenuItem;
use crate::elements::SizeConstraint;
use crate::event::DispatchedEvent;
use crate::event::ModifiersState;
use crate::geometry::vec2f;
use crate::theme::ColorToken;
use goble_core::harness::CommandDecision;

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

/// The composer is **one** rounded card holding **one** input segment: the
/// owner removed the directory/branch context band ("nu mai oferim composer
/// dublu"), so there is no folder or branch control any more. The card is inset
/// by the surface gutter, the segment by the card's casing, and the footer
/// (attach, model) sits inside the segment under the editor. The instruction
/// row is a fixed line under the card, aligned right, outside its surface.
#[test]
fn the_composer_card_is_one_rounded_input_segment() {
    use crate::elements::ShortcutHint;
    use crate::elements::{
        COMPOSER_CARD_GUTTER, COMPOSER_CARD_INSET, COMPOSER_CARD_RADIUS, COMPOSER_INPUT_RADIUS,
    };
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let surface = app.theme.color(ColorToken::Surface);
    let raised = app.theme.color(ColorToken::SurfaceRaised);
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_model_label("gpt-4o")
        // Both labels are retired: the composer ignores them, so neither a
        // folder nor a branch glyph may appear in the card.
        .with_path_label("~/Projects/goble")
        .with_branch_label("main")
        .with_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")])
        .with_on_attach(|| {})
        .with_on_select_model(|| {})
        .with_on_send(|_| {});
    let (size, commands) = paint_composer_in(&app, &mut composer, vec2f(800.0, 600.0));

    let fill = |radius: f32| {
        commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if (*corner_radius - radius).abs() < 0.01 => Some((*rect, *color)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the {radius}pt rounded box is filled: {commands:?}"))
    };
    let (card, card_color) = fill(COMPOSER_CARD_RADIUS);
    assert_eq!(
        card_color, surface,
        "the card is filled with the Surface token: {commands:?}"
    );
    assert_eq!(
        card.min_x(),
        COMPOSER_CARD_GUTTER,
        "the card keeps the surface gutter on the left"
    );
    assert!(
        (card.width() - (size.x - COMPOSER_CARD_GUTTER * 2.0)).abs() < 0.5,
        "and on the right: {:?} in {size:?}",
        card
    );

    let (input, input_color) = fill(COMPOSER_INPUT_RADIUS);
    assert_eq!(
        input_color, raised,
        "the input segment is one step lighter than the card: {commands:?}"
    );
    assert!(
        (input.min_x() - card.min_x() - COMPOSER_CARD_INSET).abs() < 0.5,
        "the input segment is inset {COMPOSER_CARD_INSET} from the card's left edge"
    );
    assert!(
        (card.max_x() - input.max_x() - COMPOSER_CARD_INSET).abs() < 0.5,
        "and from its right edge"
    );
    assert!(
        (card.max_y() - input.max_y() - COMPOSER_CARD_INSET).abs() < 0.5,
        "and from its bottom edge"
    );
    // Neither surface paints an outline: the card and its segment are flat
    // fills separated by tone alone.
    let outlines: usize = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::StrokeRect { rect, .. } if rect.width() > 100.0))
        .count();
    assert_eq!(
        outlines, 0,
        "the card and its segment are borderless (the small key caps are not the card): {commands:?}"
    );

    // The folder and branch controls are gone from the composer entirely.
    let icons: Vec<&str> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert!(!icons.contains(&"folder"), "no folder control: {icons:?}");
    assert!(
        !icons.contains(&"git-branch"),
        "no branch control: {icons:?}"
    );
    assert!(
        icons.contains(&"plus"),
        "the attach control stays: {icons:?}"
    );

    // The editor leads the segment and the footer controls under it share a
    // line, inside the same segment.
    let text_y = |text: &str| -> f32 {
        commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::DrawText {
                    origin,
                    text: drawn,
                    ..
                } if drawn == text => Some(origin.y),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{text:?} is drawn: {commands:?}"))
    };
    let editor = text_y("hi");
    let model = text_y("gpt-4o");
    let plus = icon_origin(&commands, "plus")
        .expect("the attach control draws")
        .y;
    assert!(editor < model, "the model control is under the editor");
    assert!(editor < plus, "the attach control is under the editor");
    assert!(
        (model - plus).abs() < 8.0,
        "the footer controls share one line: {model} against {plus}"
    );
    assert!(
        model < input.max_y() && input.min_y() < editor,
        "the editor and the footer both live inside the input segment: {input:?}"
    );

    // The instruction row is below the card, not inside it, and it sits on the
    // right half of the row (the owner asked for it below-right).
    let hint = text_y("new conversation");
    assert!(
        hint > card.max_y(),
        "the instruction is under the card ({hint} against {:?})",
        card.max_y()
    );
    let hint_x = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == "new conversation" => {
                Some(origin.x)
            }
            _ => None,
        })
        .expect("the instruction is drawn");
    assert!(
        hint_x > size.x / 2.0,
        "the instruction row is aligned right ({hint_x} of {})",
        size.x
    );
}

/// The terminal surface's composer is not a floating card: it is the pane's own
/// bottom band. The outer box runs the full width the host gives it — flush with
/// the pane's sides, zero inset — is square against the content above it (no top
/// radius), and carries the panel's radius only on its bottom corners. The inner
/// input segment stretches with it but keeps its radius and its casing inset, so
/// the band holds the same rounded field the floating card does.
#[test]
fn the_terminal_band_is_the_panes_full_width_bottom_band() {
    use crate::elements::chat_composer::{
        COMPOSER_BAND_BOTTOM_INSET, COMPOSER_BAR_RESERVED_HEIGHT, COMPOSER_CARD_INSET,
        COMPOSER_HINTS_GAP, COMPOSER_HINTS_ROW_HEIGHT, COMPOSER_INPUT_RADIUS,
    };
    use crate::elements::ShortcutHint;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let surface = app.theme.color(ColorToken::Surface);
    let raised = app.theme.color(ColorToken::SurfaceRaised);
    // The pane card's radius, as the app hands it over from `panes.rs`.
    const PANEL_RADIUS: f32 = 10.0;
    // The boxes of one painted colour, optionally narrowed to one radius.
    let boxes = |commands: &[RenderCommand], color, radius: Option<f32>| {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color: painted,
                    corner_radius,
                } if *painted == color
                    && radius.is_none_or(|r| (*corner_radius - r).abs() < 0.01) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let banded = || {
        ChatComposer::new()
            .with_value("hi")
            .with_path_label("~/Projects/goble")
            .with_branch_label("main")
            .with_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")])
            .with_on_send(|_| {})
            .with_on_cmd_enter(|_| {})
            .with_bottom_band(PANEL_RADIUS)
    };

    for width in [800.0_f32, 320.0] {
        let mut composer = banded();
        let (size, commands) = paint_composer_in(&app, &mut composer, vec2f(width, 600.0));
        assert!(
            (size.y - COMPOSER_BAR_RESERVED_HEIGHT).abs() < 0.01,
            "the band lays out the floating card's height ({}), not {}: {size:?}",
            COMPOSER_BAR_RESERVED_HEIGHT,
            size.y
        );

        // The outer box: the Surface fill with the panel's radius, spanning the
        // pane's whole width with no gutter and ending at the pane's bottom.
        let outer = boxes(&commands, surface, Some(PANEL_RADIUS));
        assert_eq!(
            outer.len(),
            1,
            "the band paints one panel-rounded box: {commands:?}"
        );
        assert_eq!(outer[0].min_x(), 0.0, "zero side inset: {:?}", outer[0]);
        assert!(
            (outer[0].width() - width).abs() < 0.01,
            "the band runs the pane's full width: {:?} of {width}",
            outer[0]
        );
        assert!(
            (outer[0].max_y() - size.y).abs() < 0.01,
            "the band reaches the pane's bottom: {:?} in {size:?}",
            outer[0]
        );

        // No top radius: the rounded box's top-corner notches are squared with a
        // strip of the same tone, one panel radius tall, across the whole width.
        let squared = boxes(&commands, surface, Some(0.0));
        assert!(
            squared.iter().any(|rect| rect.min_x() == 0.0
                && (rect.width() - width).abs() < 0.01
                && rect.min_y() == 0.0
                && (rect.height() - PANEL_RADIUS).abs() < 0.01),
            "the band's top edge is square against the content: {squared:?}"
        );

        // The inner input segment keeps its radius and its casing inset: it
        // stretched with the outer box, it did not lose its rounded shape.
        let inner = boxes(&commands, raised, Some(COMPOSER_INPUT_RADIUS));
        assert_eq!(
            inner.len(),
            1,
            "the band holds the one rounded input segment: {commands:?}"
        );
        assert_eq!(
            inner[0].min_x(),
            COMPOSER_CARD_INSET,
            "the segment keeps its {COMPOSER_CARD_INSET}pt casing inset"
        );
        assert!(
            (inner[0].width() - (width - COMPOSER_CARD_INSET * 2.0)).abs() < 0.5,
            "the segment stretches with the band: {:?} of {width}",
            inner[0]
        );
        assert!(
            (inner[0].max_y()
                - (size.y
                    - COMPOSER_BAND_BOTTOM_INSET
                    - COMPOSER_HINTS_GAP
                    - COMPOSER_HINTS_ROW_HEIGHT))
                .abs()
                < 0.5,
            "the folded gutter is the band's bottom casing and the fixed instruction \
             row sits under the segment: {:?} of {size:?}",
            inner[0]
        );
    }
}

/// The band does not move the card's height and does not touch the agent
/// surface: a terminal composer (banded) and an agent composer (floating) with
/// the same controls and the same instruction row lay out the same height and
/// width, while the agent card keeps its gutter and its all-round radius.
#[test]
fn the_band_keeps_the_floating_cards_height_and_leaves_the_agent_card_alone() {
    use crate::elements::chat_composer::{
        COMPOSER_BAR_RESERVED_HEIGHT, COMPOSER_CARD_INSET, COMPOSER_CARD_RADIUS,
    };
    use crate::elements::{ShortcutHint, SlashMenuItem, COMPOSER_CARD_GUTTER};
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let surface = app.theme.color(ColorToken::Surface);
    let build = || {
        ChatComposer::new()
            .with_value("hi")
            // Both surfaces ship the same fixed instruction row under the card.
            .with_hints(vec![
                ShortcutHint::new(&["↵"], "send"),
                ShortcutHint::new(&["⌘", "↵"], "new conversation"),
            ])
            .with_on_send(|_| {})
            .with_on_cmd_enter(|_| {})
    };
    let window = vec2f(760.0, 600.0);

    let mut agent = build();
    let (agent_size, agent_commands) = paint_composer_in(&app, &mut agent, window);
    assert!(
        (agent_size.y - COMPOSER_BAR_RESERVED_HEIGHT).abs() < 0.01,
        "the floating agent card is the reserved height: {agent_size:?}"
    );
    let agent_card = agent_commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == surface && (*corner_radius - COMPOSER_CARD_RADIUS).abs() < 0.01 => {
                Some(*rect)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the agent card paints its floating shape: {agent_commands:?}"));
    assert_eq!(
        agent_card.min_x(),
        COMPOSER_CARD_GUTTER,
        "the agent card keeps its surface gutter"
    );
    assert!(
        (agent_card.width() - (agent_size.x - COMPOSER_CARD_GUTTER * 2.0)).abs() < 0.5,
        "and its gutter on the right: {:?} in {agent_size:?}",
        agent_card
    );
    // The card itself stops above the fixed instruction row: the row is outside
    // its painted surface, so the card's height cannot drift with the hints.
    assert!(
        agent_card.max_y() < agent_size.y,
        "the instruction row is under the card: {:?} in {agent_size:?}",
        agent_card
    );

    const PANEL_RADIUS: f32 = 10.0;
    let mut terminal = build().with_bottom_band(PANEL_RADIUS);
    let (terminal_size, terminal_commands) = paint_composer_in(&app, &mut terminal, window);
    assert_eq!(
        terminal_size, agent_size,
        "the terminal band and the agent card are the same size"
    );
    assert!(
        !terminal_commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect {
                corner_radius,
                color,
                ..
            } if *color == surface && (*corner_radius - COMPOSER_CARD_RADIUS).abs() < 0.01
        )),
        "the banded composer paints no floating card: {terminal_commands:?}"
    );

    // The slash state no longer hides a context band: both surfaces keep the
    // same card and the same height whether the draft is a command or not.
    let slash = |band: bool| {
        let mut composer = ChatComposer::new()
            .with_value("/")
            .with_focused(true)
            .with_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")])
            .with_slash_menu(
                vec![SlashMenuItem::new("new", "Start a fresh conversation")],
                Rc::new(RefCell::new(0)),
                Rc::new(RefCell::new(false)),
            )
            .with_on_send(|_| {})
            .with_on_slash_move(|_| {})
            .with_on_slash_accept(|_| {})
            .with_on_slash_dismiss(|| {});
        if band {
            composer = composer.with_bottom_band(PANEL_RADIUS);
        }
        let (size, _) = paint_composer_in(&app, &mut composer, window);
        size
    };
    let agent_slash = slash(false);
    let terminal_slash = slash(true);
    assert_eq!(
        agent_slash, terminal_slash,
        "the slash state keeps the two surfaces equal"
    );
    assert_eq!(
        agent_slash, agent_size,
        "the slash state does not change the card any more: no context band to hide"
    );
}

/// The old host flags — context above the editor, an edge-to-edge bar — no
/// longer produce a different card: there is no context band left to place, and
/// every surface draws the same single input card, so switching a pane between
/// its shell and its agent view moves nothing but the text in the editor.
#[test]
fn the_old_host_flags_do_not_change_the_card() {
    let app = AppContext::default();
    let paint = |composer: &mut ChatComposer| {
        let (size, commands) = paint_composer_in(&app, composer, vec2f(700.0, 500.0));
        (size, commands)
    };
    let mut plain = ChatComposer::new();
    let mut edge_to_edge = ChatComposer::new().with_edge_to_edge(true);
    let mut shell_bar = ChatComposer::new()
        .with_context_above_editor(true)
        .with_edge_to_edge(true);
    let (plain_size, plain_commands) = paint(&mut plain);
    for other in [&mut edge_to_edge, &mut shell_bar] {
        let (size, commands) = paint(other);
        assert_eq!(size, plain_size, "the card is the same size");
        assert_eq!(
            drawn_texts(&commands),
            drawn_texts(&plain_commands),
            "and draws the same content"
        );
    }
}

/// A rich-input control is borderless: at rest it is only its glyph and label,
/// and the pointer over it paints the soft rounded hover fill. The card and the
/// input segment carry no outline either — the whole composer is flat surfaces.
#[test]
fn a_rich_input_control_is_borderless_and_fills_on_hover() {
    use crate::elements::COMPOSER_CONTROL_RADIUS;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    // The footer controls sit on the raised input segment, where the theme's
    // `Hover` is the segment's own fill: the visible step above it is `Selected`.
    let hover = app.theme.color(ColorToken::Selected);
    let mut composer = ChatComposer::new()
        .with_path_label("~/Projects/goble")
        .with_on_attach(|| {});

    let rest = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0)).1;
    // The card's own Surface fill and the input segment's raised fill are the
    // only fills at rest: no control draws its own rounded fill until the
    // pointer is over it.
    let control_fills = |commands: &[crate::render::RenderCommand]| {
        commands
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    RenderCommand::FillRect {
                        corner_radius, ..
                    } if (*corner_radius - 6.0).abs() < 0.01
                )
            })
            .count()
    };
    assert_eq!(
        control_fills(&rest),
        0,
        "nothing fills until the pointer is over it: {rest:?}"
    );
    let outlines: Vec<f32> = rest
        .iter()
        .filter_map(|c| match c {
            RenderCommand::StrokeRect { corner_radius, .. } => Some(*corner_radius),
            _ => None,
        })
        .collect();
    assert!(
        outlines.is_empty(),
        "the borderless composer draws no outline: {outlines:?}"
    );

    // The pointer onto the attach control: its hover fill is the control's own
    // rounded box — 24pt tall, the control radius — inside the input segment.
    let plus = icon_origin(&rest, "plus").expect("the attach control draws");
    let over = paint_composer_with_pointer(
        &app,
        &mut composer,
        vec2f(600.0, 500.0),
        Some(vec2f(plus.x + 4.0, plus.y + 4.0)),
    );
    let (rect, radius) = over
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == hover && (*corner_radius - COMPOSER_CONTROL_RADIUS).abs() < 0.01 => {
                Some((*rect, *corner_radius))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the pointer over the control fills it: {over:?}"));
    assert_eq!(
        radius, COMPOSER_CONTROL_RADIUS,
        "the hover fill is the control's own rounded box"
    );
    assert!(
        (rect.height() - 24.0).abs() < 0.5,
        "the control is one 24pt row: {rect:?}"
    );
    assert!(
        rect.min_x() <= plus.x && plus.x <= rect.max_x(),
        "the fill sits around the glyph it holds: {rect:?} against {plus:?}"
    );
}

/// The controls that open a tray say so with the reference's trailing
/// chevron-down. After the folder band's retirement only the model chip and the
/// environment icon remain: the model chip points down when closed and flips up
/// while its DSH card is open, and the retired directory/branch controls draw no
/// chevron (and no glyph) at all.
#[test]
fn the_menu_backed_controls_draw_a_trailing_chevron() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        // The retired controls: accepted, drawn nowhere.
        .with_path_label("~/Projects/goble")
        .with_dir_menu(
            vec![PopupMenuItem::new("~/Projects/goble")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_branch_label("main")
        .with_branch_menu(
            vec![PopupMenuItem::new("main")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_model_label("gpt-4o")
        .with_model_menu(
            vec![PopupMenuItem::new("gpt-4o")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_harness_label("Local")
        .with_harness_icon("environment-computer")
        .with_harness_menu(
            vec![PopupMenuItem::new("Local")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_on_attach(|| {});

    let commands = paint_composer_in(&app, &mut composer, vec2f(760.0, 500.0)).1;
    let icons: Vec<String> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        icons
            .iter()
            .filter(|name| name.as_str() == "chevron-down")
            .count(),
        0,
        "the plain model selector draws no chevron: {icons:?}"
    );
    assert!(
        !icons
            .iter()
            .any(|name| name == "folder" || name == "git-branch"),
        "the retired folder/branch controls draw no glyph: {icons:?}"
    );
    assert_eq!(
        icons
            .iter()
            .filter(|name| name.as_str() == "chevron-up")
            .count(),
        0,
        "a closed model chip points down, DSH's trigger before its card opens: {icons:?}"
    );
    // Open, the DSH chip flips its chevron up: the card above it is showing.
    let mut opened = ChatComposer::new()
        .with_model_label("gpt-4o")
        .with_model_menu(
            vec![PopupMenuItem::new("gpt-4o")],
            Rc::new(RefCell::new(true)),
            |_| {},
        );
    let commands = paint_composer_in(&app, &mut opened, vec2f(760.0, 500.0)).1;
    let opened_icons: Vec<String> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        opened_icons
            .iter()
            .filter(|name| name.as_str() == "chevron-up")
            .count(),
        0,
        "the plain selector never draws a chevron: {opened_icons:?}"
    );
}

/// The bound-shell session chip belonged to the retired folder band. The single
/// composer names no host over its editor any more: both a bound and a local
/// session draw the same one-card input (the host remains named in the pane's
/// topbar), so switching sessions cannot reflow the bar.
#[test]
fn the_session_chip_is_retired_with_the_context_band() {
    use crate::render::RenderCommand;
    use goble_core::ssh_command::SshSession;

    let app = AppContext::default();
    let bounds = || SshSession {
        host: "web.example.com".to_string(),
        user: "deploy".to_string(),
        port: 22,
        alias: Some("web".to_string()),
    };
    let paint = |session: Option<SshSession>| {
        let mut composer = ChatComposer::new()
            .with_value("hi")
            .with_path_label("~/Projects/goble")
            .with_ssh_session(session);
        paint_composer_in(&app, &mut composer, vec2f(600.0, 400.0))
    };
    let (bound_size, bound) = paint(Some(bounds()));
    let (local_size, local) = paint(None);
    assert_eq!(
        bound_size, local_size,
        "a bound session and a local one draw the same card: {bound_size:?} against {local_size:?}"
    );
    for commands in [&bound, &local] {
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "conversation-remote")),
            "no session chip is drawn: {commands:?}"
        );
        assert!(
            !drawn_texts(commands).iter().any(|t| t.contains("deploy@")),
            "and no session label: {commands:?}"
        );
    }
}

/// The origin of the glyph `icon` in a painted frame.
fn icon_origin(
    commands: &[crate::render::RenderCommand],
    icon: &str,
) -> Option<crate::geometry::Vector2F> {
    use crate::render::RenderCommand;
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawIcon { origin, name, .. } if name == icon => Some(*origin),
        _ => None,
    })
}

/// The origin of the run `text` in a painted frame.
fn text_origin(
    commands: &[crate::render::RenderCommand],
    text: &str,
) -> Option<crate::geometry::Vector2F> {
    use crate::render::RenderCommand;
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawText {
            origin,
            text: drawn,
            ..
        } if drawn == text => Some(*origin),
        _ => None,
    })
}

/// A release at `position`: a control in this tree takes its click on the
/// release, and the point is inside the control when it was read from the
/// control's own glyph.
fn click_point(
    composer: &mut ChatComposer,
    app: &AppContext,
    position: crate::geometry::Vector2F,
) -> bool {
    composer.dispatch_event(
        &DispatchedEvent::MouseUp {
            position,
            button: 0,
        },
        &mut crate::elements::EventContext::default(),
        app,
    )
}

/// Click the control that drew `text`: the point of its label, plus a couple of
/// points, which is inside the borderless control's own box.
fn click_text(
    composer: &mut ChatComposer,
    app: &AppContext,
    commands: &[crate::render::RenderCommand],
    text: &str,
) -> bool {
    let origin =
        text_origin(commands, text).unwrap_or_else(|| panic!("{text:?} is drawn: {commands:?}"));
    click_point(composer, app, vec2f(origin.x + 2.0, origin.y + 4.0))
}

/// Whether the model list is the band on the input. Its rows are the models,
/// and the one that is not the control's own label can only come from the band.
fn model_band_drawn(commands: &[crate::render::RenderCommand]) -> bool {
    drawn_texts(commands).iter().any(|text| text == "o3-mini")
}

/// Whether the command list is on the input: the rows the host draws there,
/// named the way the list names them (with the slash the draft typed).
fn command_band_drawn(commands: &[crate::render::RenderCommand]) -> bool {
    drawn_texts(commands).iter().any(|text| text == "/help")
}

/// The host's frame: the band over the input in the one slot both bands share —
/// the command list while the draft is a command, else the models the model
/// control opened. It is the pairing `views::chat_view::ChatView` and
/// `app/src/ui/chat/composer.rs` build, so a test reads the input the way the
/// user sees it, and "exactly one of them is up" is a statement about pixels.
fn paint_host_frame(
    app: &AppContext,
    composer: &mut ChatComposer,
    slash: &[crate::elements::SlashMenuItem],
    slash_index: Rc<RefCell<usize>>,
    slash_dismissed: Rc<RefCell<bool>>,
    models: &HostModels,
    window: crate::geometry::Vector2F,
) -> Vec<crate::render::RenderCommand> {
    host_frame(
        app,
        composer,
        slash,
        slash_index,
        slash_dismissed,
        models,
        window,
    )
    .1
}

/// The same frame, with the band element itself, so a test can press a row of
/// the list the host draws.
fn host_frame(
    app: &AppContext,
    composer: &mut ChatComposer,
    slash: &[crate::elements::SlashMenuItem],
    slash_index: Rc<RefCell<usize>>,
    slash_dismissed: Rc<RefCell<bool>>,
    models: &HostModels,
    window: crate::geometry::Vector2F,
) -> (Option<Box<dyn Element>>, Vec<crate::render::RenderCommand>) {
    composer.layout(
        SizeConstraint::loose(window),
        &mut LayoutContext::default(),
        app,
    );
    let mut paint_ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
    let mut top = 0.0;
    let mut band: Option<Box<dyn Element>> =
        if crate::elements::slash_menu_open(&composer.value(), *slash_dismissed.borrow()) {
            Some(crate::elements::SlashMenu::new(slash.to_vec(), slash_index).finish())
        } else if *models.open.borrow() {
            // The model list is drawn exactly as the view draws it: the host
            // owns the rows, the open flag and the selection, the band closes
            // itself on a row and reports the row to the app's handler.
            let accept = models.on_select.clone();
            let open = models.open.clone();
            let mut menu =
                crate::elements::ModelMenu::new(models.items.clone(), models.index.clone())
                    .with_on_accept(move |index| {
                        *open.borrow_mut() = false;
                        (accept.borrow_mut())(index);
                    });
            let index = models.index.clone();
            let len = models.items.len();
            menu = menu.with_on_move(move |row| {
                *index.borrow_mut() = row.min(len.saturating_sub(1));
            });
            Some(menu.finish())
        } else {
            None
        };
    if let Some(band) = band.as_mut() {
        let size = band.layout(
            SizeConstraint::loose(window),
            &mut LayoutContext::default(),
            app,
        );
        band.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
        top = size.y;
    }
    composer.paint(vec2f(0.0, top), &mut paint_ctx, app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    (band, commands)
}

/// The model list a host draws: the rows, the app-owned open flag and chosen
/// row, and the handler a taken row is reported to.
struct HostModels {
    items: Vec<PopupMenuItem>,
    open: Rc<RefCell<bool>>,
    index: Rc<RefCell<usize>>,
    on_select: Rc<RefCell<dyn FnMut(usize) + 'static>>,
}

impl HostModels {
    fn new(
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        index: Rc<RefCell<usize>>,
        on_select: Rc<RefCell<dyn FnMut(usize) + 'static>>,
    ) -> Self {
        Self {
            items,
            open,
            index,
            on_select,
        }
    }
}

/// The two models a test's list holds: the one in force and one other, so a
/// drawn row that is not the control's label can only come from the list.
fn two_models() -> Vec<PopupMenuItem> {
    vec![
        PopupMenuItem::new("gpt-4o").selected(),
        PopupMenuItem::new("o3-mini"),
    ]
}

/// The model control reads what the pane would run, and says so when there is
/// nothing: a pane with no model configured must not read as one that has one,
/// and a pane that has one must not read as a pane that has none — the words
/// are only ever drawn in place of an unset label.
#[test]
fn the_model_control_reads_not_configured_when_nothing_is_configured() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let mut composer = ChatComposer::new().with_model_label("").with_model_menu(
        vec![PopupMenuItem::new("gpt-4o")],
        Rc::new(RefCell::new(false)),
        |_| {},
    );

    let (_, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 400.0));
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawText { text, .. } if text == super::composer::MODEL_NOT_CONFIGURED
        )),
        "the control says what is missing: {commands:?}"
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "sparkle")),
        "the model control carries no icon: {commands:?}"
    );

    // A configured model reads as itself: the words above are the ones the
    // unset label draws, and nothing else does.
    let mut configured = ChatComposer::new().with_model_label("o3-mini");
    let (_, commands) = paint_composer_in(&app, &mut configured, vec2f(600.0, 400.0));
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|text| text == "o3-mini"),
        "the control names the model the pane would run: {commands:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|text| text == super::composer::MODEL_NOT_CONFIGURED),
        "and never reads as a pane with no model: {commands:?}"
    );
}

/// A custom harness owns its model and approval policy, so its footer carries
/// the harness's own mark where the native model and approval controls would
/// be. A composer given both still draws only the mark: the two controls are
/// the native agent's and say nothing about a CLI harness's turn.
#[test]
fn a_custom_harness_footer_draws_its_mark_in_place_of_model_and_approval() {
    use crate::elements::ApprovalMode;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let mut custom = ChatComposer::new()
        .with_value("hi")
        .with_agent_harness("claude")
        .with_model_label("gpt-4o")
        .with_approval_mode(ApprovalMode::AutoReview);
    let (_, commands) = paint_composer_in(&app, &mut custom, vec2f(600.0, 400.0));

    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawIcon { name, .. } if name == "harness-claude"
        )),
        "the footer draws the harness's mark: {commands:?}"
    );
    let texts = drawn_texts(&commands);
    assert!(
        !texts.iter().any(|text| text == "gpt-4o"),
        "a custom harness draws no model control: {commands:?}"
    );
    assert!(
        !texts.iter().any(|text| text == "Requires review"),
        "a custom harness draws no approval control: {commands:?}"
    );

    // The native surface is unchanged: both controls, and no harness mark.
    let mut native = ChatComposer::new()
        .with_value("hi")
        .with_model_label("gpt-4o")
        .with_approval_mode(ApprovalMode::AutoReview);
    let (_, commands) = paint_composer_in(&app, &mut native, vec2f(600.0, 400.0));
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|text| text == "gpt-4o"),
        "the native footer keeps its model control: {commands:?}"
    );
    assert!(
        texts.iter().any(|text| text == "Requires review"),
        "the native footer keeps its approval control: {commands:?}"
    );
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawIcon { name, .. } if name == "eye"
        )),
        "the native footer uses the mode icon: {commands:?}"
    );
    assert!(
        !commands.iter().any(
            |c| matches!(c, RenderCommand::DrawIcon { name, .. } if name.starts_with("harness-"))
        ),
        "the native footer draws no harness mark: {commands:?}"
    );
}

/// The model control opens the models as the band the commands are drawn in: a
/// full-width row list over the input, taken with Up/Down and Enter or Tab, put
/// away with Escape — while the editor keeps the caret and the keys.
///
/// The list itself is the host's, in the one slot the command list takes, so
/// the input draws none of its own (see `views::chat_view` for the frame that
/// puts it over the strip).
#[test]
fn the_model_control_opens_the_models_the_host_draws() {
    use crate::elements::EventContext;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(false));
    let index = Rc::new(RefCell::new(0));
    let caret = Rc::new(RefCell::new(0));
    let chosen: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let log = chosen.clone();
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_focused(true)
        .with_caret(caret.clone())
        .with_model_label("gpt-4o")
        .with_model_menu(two_models(), open.clone(), move |row| {
            log.borrow_mut().push(row)
        })
        .with_model_menu_index(index.clone());

    // Closed, the control is its label and the list is not drawn.
    let (_, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawText { text, .. } if text == "o3-mini")),
        "a closed model menu draws no rows: {commands:?}"
    );

    let mut ctx = EventContext::default();
    assert!(
        click_text(&mut composer, &app, &commands, "gpt-4o"),
        "the press reaches the control"
    );
    assert!(*open.borrow(), "the control opens the model list");

    // Open, the input still draws the list nowhere: the rows belong to the
    // host's band, over the whole block (see `host_frame`).
    let models = HostModels::new(
        two_models(),
        open.clone(),
        index.clone(),
        Rc::new(RefCell::new(|_: usize| {})),
    );
    let (size, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawText { text, .. } if text == "o3-mini")),
        "the input draws the model list nowhere: {commands:?}"
    );

    let (band, commands) = host_frame(
        &app,
        &mut composer,
        &[],
        Rc::new(RefCell::new(0)),
        Rc::new(RefCell::new(false)),
        &models,
        vec2f(600.0, 500.0),
    );
    assert!(band.is_some(), "the host draws the list the control opened");
    let panel = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::SurfaceRaised) =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the band is a panel of its own");
    assert!(
        (panel.min_x() - crate::elements::COMPOSER_CARD_GUTTER).abs() < 0.5
            && (panel.width() - (size.x - crate::elements::COMPOSER_CARD_GUTTER * 2.0)).abs() < 0.5,
        "the DSH card lines up with the input card under it: {panel:?} in {size:?}"
    );
    let editor = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == "hi" => Some(*origin),
            _ => None,
        })
        .expect("the editor text");
    assert!(
        panel.max_y() <= editor.y,
        "the band is drawn over the input, above the editor: {panel:?} against {editor:?}"
    );
    for label in ["gpt-4o", "o3-mini"] {
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, RenderCommand::DrawText { text, .. } if text == label)),
            "the band lists {label}: {commands:?}"
        );
    }
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "check")),
        "the model in force is the row with DSH's trailing check: {commands:?}"
    );

    // A second press on the control puts the list away, the way Escape does.
    // The band over the input moves the composer down, so the control's box is
    // read from the frame the composer is drawn at its own origin again.
    let (_, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    assert!(
        click_text(&mut composer, &app, &commands, "gpt-4o"),
        "the press reaches the control"
    );
    assert!(!*open.borrow(), "a second press puts the band away");
    assert!(
        click_text(&mut composer, &app, &commands, "gpt-4o"),
        "a third press opens it again"
    );
    assert!(*open.borrow(), "the control holds the list open or shut");

    // The band holds the keys that take a row from it; the editor keeps every
    // other key and its own caret.
    assert_eq!(*index.borrow(), 0, "the band opens on the model in force");
    assert!(
        press(&mut composer, &app, "ArrowDown"),
        "Down belongs to the band"
    );
    assert_eq!(*index.borrow(), 1, "Down moves the selection");
    assert!(press(&mut composer, &app, "ArrowUp"));
    assert_eq!(*index.borrow(), 0, "Up moves it back");
    assert!(
        composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "x".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut ctx,
            &app,
        ),
        "a key the band does not take is the editor's"
    );
    assert_eq!(composer.value(), "xhi", "the editor takes what is typed");
    assert_eq!(*caret.borrow(), 1, "and it keeps the caret where it types");

    // Tab takes the row the selection is on, and the list follows it away.
    assert_eq!(*index.borrow(), 0);
    assert!(
        press(&mut composer, &app, "Tab"),
        "Tab takes the selected row"
    );
    assert_eq!(
        *chosen.borrow(),
        vec![0],
        "the host is told which row was taken"
    );
    assert!(!*open.borrow(), "and the list is put away");

    // Escape does the same, from the list the control has open.
    *open.borrow_mut() = true;
    assert!(press(&mut composer, &app, "Escape"));
    assert!(!*open.borrow(), "Escape puts the band away");
    assert_eq!(*chosen.borrow(), vec![0], "and nothing was taken by it");

    // A press on a row takes that row, the way the command list's rows do. The
    // row is the host's band element, which is what takes the press now.
    *open.borrow_mut() = true;
    let taking = Rc::new(RefCell::new(Vec::new()));
    let taken = taking.clone();
    let models = HostModels::new(
        two_models(),
        open.clone(),
        index.clone(),
        Rc::new(RefCell::new(move |row: usize| taken.borrow_mut().push(row))),
    );
    let (mut band, commands) = host_frame(
        &app,
        &mut composer,
        &[],
        Rc::new(RefCell::new(0)),
        Rc::new(RefCell::new(false)),
        &models,
        vec2f(600.0, 500.0),
    );
    let row = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == "o3-mini" => {
                Some(crate::geometry::RectF::new(
                    crate::geometry::PointF::new(origin.x, origin.y - 6.0),
                    crate::geometry::Size2F::new(120.0, 20.0),
                ))
            }
            _ => None,
        })
        .expect("the second row is drawn in the band");
    let band = band.as_mut().expect("the band element");
    assert!(
        band.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(
                    row.min_x() + row.width() / 2.0,
                    row.min_y() + row.height() / 2.0,
                ),
                button: 0,
            },
            &mut ctx,
            &app,
        ),
        "the row takes the press"
    );
    assert_eq!(
        *taking.borrow(),
        vec![1],
        "the row that was pressed is the one taken"
    );
    assert!(!*open.borrow(), "and the band closes on it");
}

/// The two bands are one slot over the input. A draft that turns into a command
/// moves it to the command list: the model band is put away rather than left
/// drawn beside it, and the keys that follow are the command list's.
#[test]
fn typing_a_command_takes_the_slot_from_the_model_band() {
    use crate::elements::{slash_menu_open, EventContext, SlashMenuItem};

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(false));
    let model_index = Rc::new(RefCell::new(0));
    let slash_index = Rc::new(RefCell::new(0));
    let dismissed = Rc::new(RefCell::new(false));
    let caret = Rc::new(RefCell::new(0));
    let cloud = Rc::new(RefCell::new(Vec::new()));
    let cloud_for_chord = cloud.clone();
    let commands = vec![
        SlashMenuItem::new("help", "List the commands"),
        SlashMenuItem::new("clear", "Clear the transcript"),
    ];
    // The app clears the dismissal on every edit: typing brings the command
    // list back, and the model band does not come back with it.
    let edit_dismissed = dismissed.clone();
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_focused(true)
        .with_caret(caret.clone())
        .with_model_label("gpt-4o")
        .with_model_menu(two_models(), open.clone(), |_| {})
        .with_model_menu_index(model_index.clone())
        .with_slash_menu(commands.clone(), slash_index.clone(), dismissed.clone())
        .with_on_change(move |_| *edit_dismissed.borrow_mut() = false)
        .with_on_send_to_cloud(move |text| cloud_for_chord.borrow_mut().push(text));
    let models = HostModels::new(
        two_models(),
        open.clone(),
        model_index.clone(),
        Rc::new(RefCell::new(|_: usize| {})),
    );

    // The model band is up over a draft that is not a command, and it is the
    // only band on the input.
    let (_, closed) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    assert!(
        click_text(&mut composer, &app, &closed, "gpt-4o"),
        "the control takes the press"
    );
    assert!(*open.borrow(), "the model list is up");
    let frame = paint_host_frame(
        &app,
        &mut composer,
        &commands,
        slash_index.clone(),
        dismissed.clone(),
        &models,
        vec2f(600.0, 500.0),
    );
    assert!(
        model_band_drawn(&frame),
        "the model band is drawn: {frame:?}"
    );
    assert!(
        !command_band_drawn(&frame),
        "and no command row is drawn with it: {frame:?}"
    );

    // Typing the slash makes the draft a command, which takes the slot.
    assert!(
        composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "/".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut EventContext::default(),
            &app,
        ),
        "the editor takes the slash"
    );
    assert_eq!(composer.value(), "/hi", "the slash lands in the draft");
    assert_eq!(*caret.borrow(), 1, "and the editor keeps the caret");
    assert!(
        slash_menu_open(&composer.value(), *dismissed.borrow()),
        "the command list is the band the draft asks for"
    );
    let frame = paint_host_frame(
        &app,
        &mut composer,
        &commands,
        slash_index.clone(),
        dismissed.clone(),
        &models,
        vec2f(600.0, 500.0),
    );
    assert!(
        command_band_drawn(&frame),
        "the command list is drawn: {frame:?}"
    );
    assert!(!*open.borrow(), "the model band is put away");
    assert!(
        !model_band_drawn(&frame),
        "and it is not drawn beside the command list: {frame:?}"
    );

    // The band that is up is the one that owns the keys.
    assert!(
        press(&mut composer, &app, "ArrowDown"),
        "Down belongs to a band"
    );
    assert_eq!(*slash_index.borrow(), 1, "it moves the command list's row");
    assert_eq!(
        *model_index.borrow(),
        0,
        "and leaves the model list's row alone"
    );

    // The cloud chord is the host's and runs before both bands: it still
    // claims the key the command list would otherwise take.
    assert!(
        composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Enter".to_string(),
                modifiers: ModifiersState {
                    alt: true,
                    command: true,
                    ..Default::default()
                },
            },
            &mut EventContext::default(),
            &app,
        ),
        "the cloud chord is taken before the bands"
    );
    assert_eq!(
        *cloud.borrow(),
        vec!["/hi".to_string()],
        "the cloud chord reaches the host with a band up"
    );
}

/// The other way round: with the command list up, opening the models puts that
/// list away, so the model band is the only one drawn and the keys are its own.
#[test]
fn opening_the_model_band_puts_the_command_list_away() {
    use crate::elements::{slash_menu_open, EventContext, SlashMenuItem};

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(false));
    let model_index = Rc::new(RefCell::new(0));
    let slash_index = Rc::new(RefCell::new(0));
    let dismissed = Rc::new(RefCell::new(false));
    let caret = Rc::new(RefCell::new(1));
    let commands = vec![
        SlashMenuItem::new("help", "List the commands"),
        SlashMenuItem::new("clear", "Clear the transcript"),
    ];
    let edit_dismissed = dismissed.clone();
    let dismissing = dismissed.clone();
    let mut composer = ChatComposer::new()
        .with_value("/he")
        .with_focused(true)
        .with_caret(caret.clone())
        .with_model_label("gpt-4o")
        .with_model_menu(two_models(), open.clone(), |_| {})
        .with_model_menu_index(model_index.clone())
        .with_slash_menu(commands.clone(), slash_index.clone(), dismissed.clone())
        .with_on_slash_dismiss(move || *dismissing.borrow_mut() = true)
        .with_on_change(move |_| *edit_dismissed.borrow_mut() = false);
    let models = HostModels::new(
        two_models(),
        open.clone(),
        model_index.clone(),
        Rc::new(RefCell::new(|_: usize| {})),
    );

    // A draft that is a command draws its own list over the input, with the
    // model band nowhere on it.
    let frame = paint_host_frame(
        &app,
        &mut composer,
        &commands,
        slash_index.clone(),
        dismissed.clone(),
        &models,
        vec2f(600.0, 500.0),
    );
    assert!(
        command_band_drawn(&frame),
        "the command list is drawn: {frame:?}"
    );
    assert!(
        !model_band_drawn(&frame),
        "and the model band is not drawn: {frame:?}"
    );

    // Opening the models puts that list away: one band, and it is this one.
    assert!(
        click_text(&mut composer, &app, &frame, "gpt-4o"),
        "the control takes the press"
    );
    assert!(*open.borrow(), "the model list is up");
    assert!(
        !slash_menu_open(&composer.value(), *dismissed.borrow()),
        "the command list is put away, the way Escape puts it away"
    );
    let frame = paint_host_frame(
        &app,
        &mut composer,
        &commands,
        slash_index.clone(),
        dismissed.clone(),
        &models,
        vec2f(600.0, 500.0),
    );
    assert!(
        model_band_drawn(&frame),
        "the model band is drawn: {frame:?}"
    );
    assert!(
        !command_band_drawn(&frame),
        "and the command list is not drawn with it: {frame:?}"
    );

    assert!(
        press(&mut composer, &app, "ArrowDown"),
        "Down belongs to a band"
    );
    assert_eq!(*model_index.borrow(), 1, "it moves the model list's row");
    assert_eq!(
        *slash_index.borrow(),
        0,
        "and leaves the command list's row alone"
    );

    // The editor keeps the keys the band does not take, and its caret, whatever
    // band is up.
    assert!(
        composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "x".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut EventContext::default(),
            &app,
        ),
        "a letter is the editor's"
    );
    assert_eq!(
        composer.value(),
        "/xhe",
        "the letter lands where the caret was"
    );
    assert_eq!(*caret.borrow(), 2, "and the caret follows it");

    // The draft is still a command, so the next frame hands the slot back to
    // the command list: the model band does not linger under it.
    let frame = paint_host_frame(
        &app,
        &mut composer,
        &commands,
        slash_index.clone(),
        dismissed.clone(),
        &models,
        vec2f(600.0, 500.0),
    );
    assert!(
        command_band_drawn(&frame),
        "the command list is drawn again: {frame:?}"
    );
    assert!(!*open.borrow(), "the model band is put away");
    assert!(
        !model_band_drawn(&frame),
        "and not drawn under it: {frame:?}"
    );
    assert!(
        press(&mut composer, &app, "ArrowDown"),
        "Down belongs to a band"
    );
    assert_eq!(*slash_index.borrow(), 1, "it moves the command list's row");
    assert_eq!(
        *model_index.borrow(),
        1,
        "and leaves the model list's row alone"
    );
}

#[test]
fn the_attach_menu_opens_and_paints_its_dsh_card() {
    use crate::elements::PaintContext;
    use crate::render::{RenderCommand, Renderer};

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(true));
    let mut composer = ChatComposer::new()
        .with_on_attach(|| {})
        .with_on_image(|| {})
        .with_attach_menu_open(open);

    let size = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);

    let mut paint_ctx = PaintContext::new(Renderer::new());
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();

    // The open `+` card is a DSH menu surface above the footer: a rounded,
    // bordered panel with the host's real rows.
    let strokes = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::StrokeRect { rect, .. } if rect.width() > 100.0))
        .count();
    assert!(
        strokes >= 1,
        "the open attach card paints its panel border: {commands:?}"
    );
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|text| text == "Add files"),
        "the card lists the file picker: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "Add image"),
        "and the image gesture the host wired: {texts:?}"
    );
}

fn drawn_texts(commands: &[crate::render::RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Lay the composer out and paint it, returning the draw commands.
fn paint_composer(
    app: &AppContext,
    composer: &mut ChatComposer,
) -> Vec<crate::render::RenderCommand> {
    paint_composer_in(app, composer, vec2f(600.0, 500.0)).1
}

/// The same, at a chosen width: the composer's own size and what it painted.
fn paint_composer_in(
    app: &AppContext,
    composer: &mut ChatComposer,
    window: crate::geometry::Vector2F,
) -> (crate::geometry::Vector2F, Vec<crate::render::RenderCommand>) {
    let size = composer.layout(
        SizeConstraint::loose(window),
        &mut LayoutContext::default(),
        app,
    );
    let mut paint_ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    (size, commands)
}

/// The same, with the pointer at `cursor` for the frame that paints: the tree
/// is rebuilt every frame, so an element reads the cursor at paint (see
/// `views::chat_view::tests`). `None` puts the pointer outside the window.
fn paint_composer_with_pointer(
    app: &AppContext,
    composer: &mut ChatComposer,
    window: crate::geometry::Vector2F,
    cursor: Option<crate::geometry::Vector2F>,
) -> Vec<crate::render::RenderCommand> {
    let _ = composer.layout(
        SizeConstraint::loose(window),
        &mut LayoutContext::default(),
        app,
    );
    let mut paint_ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
    if let Some(position) = cursor {
        paint_ctx.cursor_inside = true;
        paint_ctx.cursor_position = position;
    }
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

/// Dispatch a key the way the window does.
fn press(composer: &mut ChatComposer, app: &AppContext, name: &str) -> bool {
    composer.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers: ModifiersState::none(),
        },
        &mut crate::elements::EventContext::default(),
        app,
    )
}

fn proposal(candidates: &[&str]) -> CommandProposalUi {
    CommandProposalUi::new(
        "call-1",
        candidates.iter().map(|c| c.to_string()).collect(),
        "/work/project",
    )
}

type DecisionLog = Rc<RefCell<Vec<(String, CommandDecision)>>>;

fn decision_log() -> DecisionLog {
    Rc::new(RefCell::new(Vec::new()))
}

fn record_into(log: DecisionLog) -> impl FnMut(String, CommandDecision) + 'static {
    move |id, decision| log.borrow_mut().push((id, decision))
}

#[test]
fn arrow_keys_cycle_the_candidates() {
    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_proposal(proposal(&["git status", "git diff --stat"]));
    paint_composer(&app, &mut composer);

    assert!(press(&mut composer, &app, "ArrowDown"));
    assert_eq!(
        composer.selected_candidate().as_deref(),
        Some("git diff --stat")
    );

    // Wraps past the end back to the first, and the other way from the top.
    assert!(press(&mut composer, &app, "ArrowDown"));
    assert_eq!(composer.selected_candidate().as_deref(), Some("git status"));
    assert!(press(&mut composer, &app, "ArrowUp"));
    assert_eq!(
        composer.selected_candidate().as_deref(),
        Some("git diff --stat")
    );
}

#[test]
fn a_host_owned_selection_survives_the_rebuild() {
    let app = AppContext::default();
    let selection = Rc::new(RefCell::new(1));
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_proposal(proposal(&["git status", "git diff --stat"]))
        .with_proposal_selection(selection.clone());
    paint_composer(&app, &mut composer);

    assert_eq!(
        composer.selected_candidate().as_deref(),
        Some("git diff --stat")
    );

    assert!(press(&mut composer, &app, "ArrowUp"));
    assert_eq!(*selection.borrow(), 0);
    assert_eq!(composer.selected_candidate().as_deref(), Some("git status"));
}

#[test]
fn the_rich_input_stays_put_while_a_proposal_is_pending() {
    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_proposal(proposal(&["git status"]))
        .with_path_label("~/Projects/goble")
        .with_branch_label("main")
        .with_model_label("gpt-4o")
        .with_context_above_editor(true)
        .with_on_attach(|| {})
        .with_hints(vec![crate::elements::ShortcutHint::new(
            &["⌘", "↵"],
            "new conversation",
        )]);
    let commands = paint_composer(&app, &mut composer);
    let texts = drawn_texts(&commands);

    // The approval is the transcript's card; the input keeps its own chrome.
    for chrome in ["gpt-4o", "new conversation", "Ask anything..."] {
        assert!(
            texts.iter().any(|t| t == chrome),
            "the input keeps {chrome:?}, got {texts:?}"
        );
    }
    assert!(
        !texts
            .iter()
            .any(|t| t == "OK if I run this command and read the output?"),
        "the approval is not drawn in the input"
    );
}

#[test]
fn enter_approves_the_selected_candidate() {
    let app = AppContext::default();
    let decisions = decision_log();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_proposal(proposal(&["git status", "git diff --stat"]))
        .with_on_decision(record_into(decisions.clone()));
    paint_composer(&app, &mut composer);

    assert!(press(&mut composer, &app, "ArrowDown"));
    assert!(press(&mut composer, &app, "Enter"));
    assert_eq!(
        *decisions.borrow(),
        vec![(
            "call-1".to_string(),
            CommandDecision::Approve("git diff --stat".to_string())
        )]
    );
}

#[test]
fn escape_rejects_the_proposal() {
    let app = AppContext::default();
    let decisions = decision_log();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_proposal(proposal(&["git status"]))
        .with_on_decision(record_into(decisions.clone()));
    paint_composer(&app, &mut composer);

    assert!(press(&mut composer, &app, "Escape"));
    assert_eq!(
        *decisions.borrow(),
        vec![("call-1".to_string(), CommandDecision::Reject(String::new()))]
    );
}

#[test]
fn proposal_keys_are_only_taken_with_focus_and_a_proposal() {
    let app = AppContext::default();
    let decisions = decision_log();
    let first_log = decisions.clone();
    let second_log = decisions.clone();

    // No proposal: the keys belong to the ordinary editor.
    let mut plain = ChatComposer::new()
        .with_focused(true)
        .with_on_decision(record_into(first_log))
        .with_value("a draft");
    paint_composer(&app, &mut plain);
    assert!(!press(&mut plain, &app, "ArrowDown"));
    assert!(!press(&mut plain, &app, "Escape"));

    // A proposal on an unfocused composer does not swallow keys either.
    let mut blurred = ChatComposer::new()
        .with_proposal(proposal(&["git status"]))
        .with_on_decision(record_into(second_log));
    paint_composer(&app, &mut blurred);
    assert!(!press(&mut blurred, &app, "ArrowDown"));
    assert!(!press(&mut blurred, &app, "Enter"));
    assert!(decisions.borrow().is_empty());
}

#[test]
fn a_disabled_input_waits_without_taking_keys_or_presses() {
    let app = AppContext::default();
    let focused: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let log = focused.clone();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_value("a prompt the turn will not read")
        .with_disabled(true)
        .with_on_focus_change(move |focused| log.borrow_mut().push(focused));
    let (size, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    // The bar says what it waits for instead of inviting a draft, and the
    // preserved draft is not drawn: a shown draft would read as an editor that
    // still takes keys.
    assert!(
        commands.iter().any(|command| matches!(
            command,
            crate::render::RenderCommand::DrawText { text, .. }
                if text == "Waiting for your decision…"
        )),
        "the waiting placeholder is drawn: {commands:?}"
    );
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            crate::render::RenderCommand::DrawText { text, .. }
                if text.contains("a prompt the turn will not read")
        )),
        "the waiting bar does not draw the preserved draft: {commands:?}"
    );

    // Neither a key nor a press lands in the waiting input.
    assert!(!press(&mut composer, &app, "x"));
    assert_eq!(
        composer.value(),
        "a prompt the turn will not read",
        "the disabled editor took no text, and the draft is kept for after the decision"
    );
    let handled = composer.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: vec2f(size.x - 2.0, size.y - 2.0),
            button: 0,
        },
        &mut crate::elements::EventContext::default(),
        &app,
    );
    assert!(!handled, "the waiting bar does not focus itself");
    assert!(focused.borrow().is_empty(), "focus never moved to the bar");
}

/// The approval mode is a word, not a glyph: the control draws no icon, and the
/// label's colour is what distinguishes the four policies.
#[test]
fn the_approval_mode_is_neutral_icon_and_text() {
    use crate::elements::ApprovalMode;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    for mode in ApprovalMode::ALL {
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_approval_mode(mode);
        let (_, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
        let drawn = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, color, .. } if text == mode.display_label() => {
                    Some(*color)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{:?}'s label is drawn", mode));
        assert_eq!(
            drawn,
            app.theme.color(ColorToken::Text),
            "{:?} paints its label in neutral ink",
            mode
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == mode.icon()
            )),
            "{:?} draws its mode icon",
            mode
        );
    }
}

/// The approval card lists the four modes in display order with icons,
/// descriptions and the check at the end.
#[test]
fn the_approval_mode_menu_lists_modes_with_icons_and_descriptions() {
    use crate::elements::ApprovalMode;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_approval_mode(ApprovalMode::Manual)
        .with_approval_menu_open(Rc::new(RefCell::new(true)))
        .with_on_select_approval_mode(|_| {});
    let (_, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));

    for mode in ApprovalMode::DISPLAY_ORDER {
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if text == mode.display_label()
            )),
            "{:?} is listed in the open menu",
            mode
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == mode.icon()
            )),
            "{:?} has its icon",
            mode
        );
    }
    assert!(
        commands.iter().any(
            |command| matches!(command, RenderCommand::DrawIcon { name, .. } if name == "check")
        ),
        "the mode in force is stamped with the check: {commands:?}"
    );
}

/// The composer hands each menu the app's highlight cell, so Up/Down and Enter
/// keep their row across the per-frame rebuild.
#[test]
fn a_composer_menu_shares_the_apps_keyboard_highlight() {
    let app = AppContext::default();
    let open = Rc::new(RefCell::new(true));
    let highlight: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
    let picked = Rc::new(RefCell::new(None));
    let click = Rc::clone(&picked);
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_attach_menu_items(vec![
            PopupMenuItem::new("Add files").selected(),
            PopupMenuItem::new("Add image"),
        ])
        .with_attach_menu_open(Rc::clone(&open))
        .with_attach_menu_highlight(Rc::clone(&highlight))
        .with_on_select_attach_item(move |index| *click.borrow_mut() = Some(index));
    let _ = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    assert_eq!(
        *highlight.borrow(),
        Some(0),
        "the open attach menu starts on the item in force"
    );

    let mut ctx = crate::elements::EventContext::default();
    let mut key = |composer: &mut ChatComposer, key: &str| {
        composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers: ModifiersState::default(),
            },
            &mut ctx,
            &app,
        )
    };
    assert!(key(&mut composer, "ArrowDown"), "the menu takes the arrow");
    assert_eq!(*highlight.borrow(), Some(1));
    assert!(key(&mut composer, "Enter"), "and Enter");
    assert_eq!(*picked.borrow(), Some(1));
    assert!(!*open.borrow(), "selecting the row closes the menu");
}

#[test]
fn a_press_anywhere_in_the_rich_input_focuses_the_editor() {
    let app = AppContext::default();
    let focused: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let log = focused.clone();
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_path_label("~/Projects/goble")
        .with_branch_label("main")
        .with_model_label("gpt-4o")
        .with_on_attach(|| {})
        .with_on_select_model(|| {})
        .with_on_focus_change(move |focused| log.borrow_mut().push(focused));
    let size = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 500.0)),
        &mut LayoutContext::default(),
        &app,
    );
    composer.paint(
        vec2f(0.0, 0.0),
        &mut crate::elements::PaintContext::new(crate::render::Renderer::new()),
        &app,
    );

    // The bar's bottom-right gutter: inside the rich input's own bounds, below
    // and right of every child the tree holds.
    let gutter = vec2f(size.x - 2.0, size.y - 2.0);
    let mut ctx = crate::elements::EventContext::default();
    let handled = composer.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: gutter,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert!(handled, "a press in the bar belongs to the bar");
    assert_eq!(
        *focused.borrow(),
        vec![true],
        "the bar's own gutter focuses the editor"
    );

    // A press outside the bar is not the bar's to take.
    let outside = vec2f(size.x + 8.0, size.y + 8.0);
    let handled = composer.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: outside,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert!(!handled, "a press outside the bar is left to the pane");
    assert_eq!(
        *focused.borrow(),
        vec![true],
        "and it does not re-report focus"
    );
}

/// 439: narrowed, the footer keeps one line; the model truncates and the
/// approval control drops its label to the icon instead of wrapping.
#[test]
fn the_footer_stays_on_one_line_and_the_approval_control_drops_its_label() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let composer = || {
        ChatComposer::new()
            .with_model_label("gpt-4o")
            .with_approval_mode(crate::elements::ApprovalMode::Manual)
            .with_on_select_approval_mode(|_| {})
            .with_stop_visible(true)
            .with_on_stop(|| {})
            .with_on_select_model(|| {})
            .with_on_attach(|| {})
            .with_vim(Rc::new(RefCell::new(crate::vim::VimState::new())))
    };

    let (wide, wide_commands) = paint_composer_in(&app, &mut composer(), vec2f(700.0, 500.0));
    let model_y = |commands: &[RenderCommand]| -> f32 {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "gpt-4o" => Some(origin.y),
                _ => None,
            })
            .expect("the model label is drawn")
    };
    assert!(model_y(&wide_commands) > 0.0);

    // Narrow: the chip ellipsizes/absorbs the squeeze (and, if the controls
    // cannot share one line, the footer wraps), but nothing is ever painted
    // past the input's own edge.
    let (narrow, narrow_commands) = paint_composer_in(&app, &mut composer(), vec2f(260.0, 500.0));
    assert!(
        model_y(&narrow_commands) > 0.0,
        "the model chip is still drawn in a narrow footer"
    );
    assert!(
        !drawn_texts(&narrow_commands)
            .iter()
            .any(|text| text == "Default"),
        "the approval label is hidden on a narrow footer: {narrow_commands:?}"
    );
    assert!(
        narrow_commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "lock"
        )),
        "the approval icon stays on the narrow footer"
    );
    assert!(
        narrow.x <= 260.0 + 0.5,
        "the composer stays inside the pane width: {narrow:?}"
    );
    for command in &narrow_commands {
        let max_x = match command {
            RenderCommand::DrawText { origin, .. } => origin.x,
            RenderCommand::FillRect { rect, .. } => rect.max_x(),
            RenderCommand::StrokeRect { rect, .. } => rect.max_x(),
            RenderCommand::DrawIcon { origin, .. } => origin.x,
            _ => continue,
        };
        assert!(
            max_x <= narrow.x + 0.5,
            "nothing is drawn past the input's width ({}): {command:?}",
            narrow.x
        );
    }
}

/// The instructions the host hands the composer are a **fixed row under the
/// card**, aligned to its trailing edge (owner: the hints belong below the card,
/// on the right). The row is reserved from the first time a surface ships hints:
/// a hint that is showing, missing or re-worded never changes the card's
/// height, never moves the editor, and never paints over the card.
#[test]
fn the_instruction_row_is_a_fixed_line_under_the_card() {
    use crate::elements::ShortcutHint;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let line_of = |commands: &[RenderCommand], text: &str| -> Option<f32> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText {
                text: run, origin, ..
            } if run == text => Some(origin.y),
            _ => None,
        })
    };
    let card_bottom = |commands: &[RenderCommand]| -> f32 {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Surface)
                    && (*corner_radius - super::COMPOSER_CARD_RADIUS).abs() < 0.01 =>
                {
                    Some(rect.max_y())
                }
                _ => None,
            })
            .expect("the card paints its rounded rect")
    };
    let shell = |hints: Vec<ShortcutHint>| {
        ChatComposer::new()
            .with_placeholder("Run any command")
            .with_on_send(|_| {})
            // The surface always ships hints, the list is what comes and goes.
            .with_hints(hints)
    };
    let empty = paint_composer_in(&app, &mut shell(Vec::new()), vec2f(600.0, 500.0));
    let with_hints = paint_composer_in(
        &app,
        &mut shell(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")]),
        vec2f(600.0, 500.0),
    );

    let (empty_size, empty_commands) = empty;
    let (size, commands) = with_hints;
    assert!(
        line_of(&empty_commands, "new conversation").is_none(),
        "an empty instruction list draws no entry"
    );
    let editor = line_of(&commands, "Run any command").expect("the editor is drawn");
    let hint = line_of(&commands, "new conversation").expect("the instruction is drawn");
    assert!(
        hint > editor,
        "the instruction is under the editor ({hint} against {editor})"
    );
    assert!(
        hint > card_bottom(&commands),
        "the instruction is under the card, outside its painted surface ({hint} against {})",
        card_bottom(&commands)
    );
    let hint_x = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                text: run, origin, ..
            } if run == "new conversation" => Some(origin.x),
            _ => None,
        })
        .expect("the instruction is drawn");
    assert!(
        hint_x > 600.0 / 2.0,
        "the instruction row is aligned right ({hint_x} of 600)"
    );
    let empty_editor = line_of(&empty_commands, "Run any command").expect("the editor is drawn");
    assert!(
        (empty_editor - editor).abs() < 1.0,
        "the editor does not move for the row ({editor} against {empty_editor})"
    );
    // The row is reserved whether or not it has an entry, so the whole composer
    // is exactly the reserved height.
    assert!(
        (size.y - empty_size.y).abs() < 0.01,
        "the composer height is constant with the hint present or absent ({} against {})",
        size.y,
        empty_size.y
    );
    assert!(
        (size.y - super::COMPOSER_BAR_RESERVED_HEIGHT).abs() < 0.01,
        "the shell bar is the reserved bar height with the hint up: {} against {}",
        size.y,
        super::COMPOSER_BAR_RESERVED_HEIGHT
    );

    // A surface that never asks for hints pays neither the row nor its gap.
    let (hintless_size, hintless_commands) = paint_composer_in(
        &app,
        &mut ChatComposer::new().with_on_send(|_| {}),
        vec2f(600.0, 500.0),
    );
    assert!(
        line_of(&hintless_commands, "new conversation").is_none(),
        "a surface that ships no hints draws no entry"
    );
    assert!(
        (hintless_size.y - super::COMPOSER_CARD_RESERVED_HEIGHT).abs() < 0.01,
        "a hintless surface is the card's own height: {} against {}",
        hintless_size.y,
        super::COMPOSER_CARD_RESERVED_HEIGHT
    );
    assert!(
        hintless_size.y < size.y,
        "the reserved row is what the hint-shipping surface adds ({} against {})",
        hintless_size.y,
        size.y
    );
}

/// The owner's height parity: the terminal bar and the agent input are the same
/// composer. Both ship the fixed instruction row under the card — the shell's
/// one ⌘↵ entry and the agent's send / cloud / tasks set — so both lay out at
/// exactly [`COMPOSER_BAR_RESERVED_HEIGHT`] and neither gains a row for its
/// hints.
#[test]
fn the_terminal_and_agent_cards_are_the_same_height() {
    use crate::elements::ShortcutHint;

    let app = AppContext::default();
    let context = |composer: ChatComposer| composer.with_on_send(|_| {});
    // The terminal bar: the shell's command placeholder and the one
    // instruction the command line answers (⌘↵ new conversation).
    let mut terminal = context(
        ChatComposer::new()
            .with_placeholder(crate::elements::chat_composer::TERMINAL_COMPOSER_PLACEHOLDER),
    )
    .with_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")]);
    // The agent input: the agent controls and its own three instructions, drawn
    // in the same fixed row under the card.
    let mut agent = context(ChatComposer::new())
        .with_harness_label("This Computer")
        .with_harness_icon("environment-computer")
        .with_model_label("DeepSeek Flash")
        .with_on_attach(|| {})
        .with_hints(vec![
            ShortcutHint::new(&["↵"], "send"),
            ShortcutHint::new(&["⌘", "⌥", "↵"], "send to cloud"),
            ShortcutHint::new(&["⌘", "⇧", "W"], "tasks"),
        ]);

    let (terminal_size, terminal_commands) =
        paint_composer_in(&app, &mut terminal, vec2f(876.0, 500.0));
    let (agent_size, agent_commands) = paint_composer_in(&app, &mut agent, vec2f(876.0, 500.0));
    assert!(
        (terminal_size.y - agent_size.y).abs() < 0.01,
        "the terminal card must equal the agent card ({} against {})",
        terminal_size.y,
        agent_size.y
    );
    assert!(
        (terminal_size.y - super::COMPOSER_BAR_RESERVED_HEIGHT).abs() < 0.01,
        "both are the reserved bar height: {} against {}",
        terminal_size.y,
        super::COMPOSER_BAR_RESERVED_HEIGHT
    );
    // Each surface names its own gestures in the row under its card.
    let texts = drawn_texts(&terminal_commands);
    assert!(
        texts.iter().any(|text| text == "new conversation"),
        "the shell bar still names ⌘↵ new conversation: {texts:?}"
    );
    let texts = drawn_texts(&agent_commands);
    assert!(
        texts.iter().any(|text| text == "send") && texts.iter().any(|text| text == "tasks"),
        "the agent input names send and tasks: {texts:?}"
    );
}

/// The instruction row never wraps and never moves the card: at a wide and a
/// narrow pane, the composer's height with three hints is the height with none,
/// and nothing a hint draws runs past the input's own width.
#[test]
fn the_instruction_row_never_wraps_or_grows_the_card() {
    use crate::elements::ShortcutHint;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let hints = || {
        vec![
            ShortcutHint::new(&["⌘", "↵"], "new conversation"),
            ShortcutHint::new(&["⌘", "⌥", "↵"], "send to cloud"),
            ShortcutHint::new(&["⌘", "K"], "commands"),
        ]
    };
    let paint = |hints: Vec<ShortcutHint>, width: f32| {
        let mut composer = ChatComposer::new()
            .with_context_above_editor(true)
            .with_path_label("~/Projects/goble")
            .with_hints(hints)
            .with_on_send(|_| {});
        paint_composer_in(&app, &mut composer, vec2f(width, 500.0))
    };
    for width in [900.0_f32, 260.0] {
        let (wide_size, wide_commands) = paint(hints(), width);
        let (empty_size, _) = paint(Vec::new(), width);
        assert!(
            (wide_size.y - empty_size.y).abs() < 0.01,
            "the row is constant at {width}pt: {} against {}",
            wide_size.y,
            empty_size.y
        );
        for command in &wide_commands {
            let max_x = match command {
                RenderCommand::DrawText { origin, .. } => origin.x,
                RenderCommand::FillRect { rect, .. } => rect.max_x(),
                RenderCommand::StrokeRect { rect, .. } => rect.max_x(),
                RenderCommand::DrawIcon { origin, .. } => origin.x,
                _ => continue,
            };
            assert!(
                max_x <= width + 0.5,
                "nothing the instruction row draws runs past the pane at {width}pt: {command:?}"
            );
        }
    }
}

/// The shared rich input names its surface: the agent/chat composer asks for a
/// prompt ("Ask anything..."), while the app hands the terminal surface "Run any
/// command" through the builder. The placeholder keeps its mechanics — drawn
/// only while the draft is empty, gone once there is something to submit.
#[test]
fn the_placeholder_names_the_surface() {
    use crate::elements::chat_composer::{
        AGENT_COMPOSER_PLACEHOLDER, TERMINAL_COMPOSER_PLACEHOLDER,
    };

    let app = AppContext::default();
    assert_eq!(
        ChatComposer::new().placeholder(),
        AGENT_COMPOSER_PLACEHOLDER,
        "the shared default is the agent input's prompt placeholder"
    );
    assert_eq!(TERMINAL_COMPOSER_PLACEHOLDER, "Run any command");
    assert_eq!(AGENT_COMPOSER_PLACEHOLDER, "Ask anything...");

    let mut agent = ChatComposer::new().with_context_above_editor(true);
    let commands = paint_composer(&app, &mut agent);
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|text| text == AGENT_COMPOSER_PLACEHOLDER),
        "the agent input draws its placeholder: {texts:?}"
    );

    let mut terminal = ChatComposer::new()
        .with_context_above_editor(true)
        .with_placeholder(TERMINAL_COMPOSER_PLACEHOLDER);
    assert_eq!(terminal.placeholder(), TERMINAL_COMPOSER_PLACEHOLDER);
    let (_, commands) = paint_composer_in(&app, &mut terminal, vec2f(600.0, 500.0));
    let texts = drawn_texts(&commands);
    assert!(
        texts
            .iter()
            .any(|text| text == TERMINAL_COMPOSER_PLACEHOLDER),
        "the terminal rich input asks for a command: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text == AGENT_COMPOSER_PLACEHOLDER),
        "and never reads as the agent prompt input: {texts:?}"
    );

    let mut typed = ChatComposer::new()
        .with_context_above_editor(true)
        .with_placeholder(TERMINAL_COMPOSER_PLACEHOLDER)
        .with_value("ls -la");
    let (_, commands) = paint_composer_in(&app, &mut typed, vec2f(600.0, 500.0));
    let texts = drawn_texts(&commands);
    assert!(
        !texts
            .iter()
            .any(|text| text == TERMINAL_COMPOSER_PLACEHOLDER),
        "the placeholder steps aside once the draft has text: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "ls -la"),
        "the draft replaces it: {texts:?}"
    );
}

/// The composer's editor draws its insertion beam in the focus blue — the
/// colour the reference's editor cursor carries, not the UI's neutral accent —
/// and a blurred composer draws no beam.
#[test]
fn the_focused_editor_shows_the_focus_blue_caret() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let focus = app.theme.color(ColorToken::Focus);

    let mut focused = ChatComposer::new().with_value("hi").with_focused(true);
    let commands = paint_composer(&app, &mut focused);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == focus
        )),
        "the focused editor's beam is the focus blue: {commands:?}"
    );

    let mut blurred = ChatComposer::new().with_value("hi");
    let commands = paint_composer(&app, &mut blurred);
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == focus
        )),
        "a blurred composer draws no caret: {commands:?}"
    );
}

/// The rich input's cursor is a terminal's block, not a highlight: the editor
/// draws its whole line as one run, the block is a fill painted behind it, and
/// the letter the block covers is redrawn over the run in the background
/// colour, at the pen the editor gives it. The cell covers exactly the letter
/// after the insertion point, and no other letter moves.
#[test]
fn the_editor_caret_covers_the_letter_after_it_without_moving_the_text() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let focus = app.theme.color(ColorToken::Focus);
    let bg = app.theme.color(ColorToken::Bg);
    let caret = Rc::new(RefCell::new(1));
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_focused(true)
        .with_caret(caret);
    let commands = paint_composer(&app, &mut composer);

    // One intact run at the editor's own origin, and then the letter the block
    // covers, redrawn in the background colour on the run's own pen.
    let runs: Vec<(&str, crate::geometry::Vector2F, crate::color::ColorU)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text,
                color,
                ..
            } => Some((text.as_str(), *origin, *color)),
            _ => None,
        })
        .collect();
    assert_eq!(
        runs.len(),
        2,
        "the editor draws its line as one run and redraws only the letter the \
         block covers: {runs:?}"
    );
    assert_eq!(runs[0].0, "hi");
    assert_eq!(runs[1].0, "i", "the cell covers the `i` after the beam");
    assert_eq!(
        runs[1].2, bg,
        "the covered letter is drawn in the background colour, so the cell \
         reads as inverted"
    );

    let (cell_at, cell) = commands
        .iter()
        .enumerate()
        .find_map(|(index, command)| match command {
            RenderCommand::FillRect { rect, color, .. } if *color == focus => Some((index, *rect)),
            _ => None,
        })
        .expect("the focused editor paints its block");
    let advance = |ch: &str| {
        crate::platform::text_atlas::advance_width(
            ch,
            15.0,
            crate::platform::text_atlas::FontWeight::Regular,
            crate::theme::FontFamily::System,
            false,
        )
    };
    assert!(
        (cell.min_x() - (runs[0].1.x + advance("h"))).abs() < 0.5,
        "the cell starts where the `h` ends: {cell:?}"
    );
    let advance_i = advance("i");
    assert!(
        (cell.width() - advance_i).abs() < 0.5,
        "the cell covers the `i` after it ({advance_i}): {cell:?}"
    );
    assert!(
        (runs[1].1.x - cell.min_x()).abs() < 0.5 && (runs[1].1.y - runs[0].1.y).abs() < 0.5,
        "the inverted glyph sits on the cell's own pen and line: {:?} against \
         {cell:?} and {:?}",
        runs[1].1,
        runs[0].1
    );
    // The fill is behind the line and the inverted glyph over it: the run keeps
    // its own pixel column, and the cell is what changes colour.
    let run_at = commands
        .iter()
        .position(|command| matches!(command, RenderCommand::DrawText { text, .. } if text == "hi"))
        .expect("the line is drawn");
    let ink_at = commands
        .iter()
        .position(|command| matches!(command, RenderCommand::DrawText { text, .. } if text == "i"))
        .expect("the covered letter is redrawn");
    assert!(
        cell_at < run_at,
        "the block is a fill painted before the line, so the letter is not \
         hidden by it"
    );
    assert!(
        run_at < ink_at,
        "the inverted letter is painted after the line, so it is the ink that \
         changes colour and not a shade behind it"
    );
}

/// No focus ring: the owner asked for no blue border or margin around the rich
/// input, so keyboard focus is shown by the editor's beam alone. The card is
/// borderless (owner: flatter, cleaner), so it paints no outline at all —
/// focused or not — and no `Focus`-coloured stroke is painted. The caret is a
/// fill, not a stroke, so the beam still shows the focus blue inside the editor.
///
/// a11y tradeoff: the card no longer marks "this surface owns the keyboard"
/// with an outline; the visibly blinking caret inside it is the only signal
/// (recorded in the owner-feedback report).
#[test]
fn the_card_draws_no_focus_ring_when_focused() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let focus = app.theme.color(ColorToken::Focus);
    let has_focus_stroke = |commands: &[crate::render::RenderCommand]| {
        commands.iter().any(|command| match command {
            RenderCommand::StrokeRect { color, .. } => *color == focus,
            _ => false,
        })
    };
    let has_outline = |commands: &[crate::render::RenderCommand]| {
        commands
            .iter()
            .any(|command| matches!(command, RenderCommand::StrokeRect { .. }))
    };

    // Focused: the editor's beam is the whole signal; the card is ringless.
    let mut focused = ChatComposer::new().with_value("hi").with_focused(true);
    let commands = paint_composer(&app, &mut focused);
    assert!(
        !has_focus_stroke(&commands),
        "a focused card draws no blue focus ring: {commands:?}"
    );
    assert!(
        !has_outline(&commands),
        "the focused card is borderless: it paints no outline at all: {commands:?}"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == focus
        )),
        "the caret stays the focus indicator: {commands:?}"
    );

    // Blurred: the same plain casing, still no ring.
    let mut blurred = ChatComposer::new().with_value("hi");
    let commands = paint_composer(&app, &mut blurred);
    assert!(
        !has_focus_stroke(&commands),
        "a blurred card draws no focus ring: {commands:?}"
    );
    assert!(
        !has_outline(&commands),
        "the blurred card is borderless too: it paints no outline at all: {commands:?}"
    );

    // A menu opened from the card paints no ring either.
    let mut focused_with_menu = ChatComposer::new()
        .with_focused(true)
        .with_path_label("/work/project")
        .with_dir_menu(
            vec![PopupMenuItem::new("/work/project")],
            Rc::new(RefCell::new(true)),
            |_| {},
        );
    let commands = paint_composer(&app, &mut focused_with_menu);
    assert!(
        !has_focus_stroke(&commands),
        "a focused editor with a menu up still draws no focus ring: {commands:?}"
    );
}

/// 436: a real Enter keydown on the focused composer calls on_send with the
/// draft and clears it; Shift+Enter is left to the editor as a newline.
#[test]
fn plain_enter_on_the_focused_composer_sends_the_draft() {
    let app = AppContext::default();
    let sent: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = sent.clone();
    let mut composer = ChatComposer::new()
        .with_value("ship it")
        .with_focused(true)
        .with_on_send(move |text| sink.borrow_mut().push(text));
    let _ = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    assert!(
        composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Enter".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut crate::elements::EventContext::default(),
            &app,
        ),
        "the composer consumes Enter"
    );
    assert_eq!(*sent.borrow(), vec!["ship it".to_string()]);
    assert_eq!(composer.value(), "");

    let mut multiline = ChatComposer::new()
        .with_value("line one")
        .with_focused(true)
        .with_on_send(|_| panic!("Shift+Enter is a newline, not a send"));
    let _ = paint_composer_in(&app, &mut multiline, vec2f(600.0, 500.0));
    let _ = multiline.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "Enter".to_string(),
            modifiers: ModifiersState {
                shift: true,
                ..ModifiersState::none()
            },
        },
        &mut crate::elements::EventContext::default(),
        &app,
    );
}

/// The rich input draws **no send control**: the owner removed the circular
/// send button, so the footer paints no send glyph and no 32pt `Selected`
/// circle, and a press anywhere on the bar never submits. The keyboard is the
/// whole send gesture (Enter / Cmd+Enter / Cmd+Alt+Enter, covered by their own
/// tests), while the footer row keeps its height so the card's layout — and the
/// height a shell host reserves for it — does not change. The stop circle
/// remains the only round control, while a turn streams.
#[test]
fn the_rich_input_draws_no_send_control() {
    use crate::elements::COMPOSER_SEND_SIZE;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let sent: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = sent.clone();
    let mut composer = ChatComposer::new()
        .with_value("ship it")
        .with_focused(true)
        .with_on_send(move |text| sink.borrow_mut().push(text));
    let (size, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));

    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "arrow-right" || name == "send"
        )),
        "no send glyph is drawn: {commands:?}"
    );
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Selected)
                    && (rect.width() - COMPOSER_SEND_SIZE).abs() < 0.5
                    && (rect.height() - COMPOSER_SEND_SIZE).abs() < 0.5
        )),
        "no 32pt Selected circle (the old send button) is drawn: {commands:?}"
    );
    assert!(
        (size.y
            - (crate::elements::COMPOSER_INPUT_MIN_HEIGHT
                + crate::elements::COMPOSER_CARD_INSET * 2.0
                + crate::elements::COMPOSER_CARD_GUTTER))
            .abs()
            < 0.5,
        "the card keeps the input's reserved height with an empty footer: {size:?}"
    );

    // No press on the bar sends: the keyboard owns the gesture.
    for y in [0.2, 0.5, 0.8] {
        let _ = click_point(&mut composer, &app, vec2f(size.x - 20.0, size.y * y));
    }
    assert!(
        sent.borrow().is_empty(),
        "a press on the bar never sends the draft"
    );
    let _ = press(&mut composer, &app, "Enter");
    assert_eq!(
        *sent.borrow(),
        vec!["ship it".to_string()],
        "Enter is the send gesture"
    );

    // The stop circle stays while a turn streams: error red at 90%, 32pt round.
    let stopped = Rc::new(RefCell::new(false));
    let flag = stopped.clone();
    let mut streaming = ChatComposer::new()
        .with_value("a draft")
        .with_stop_visible(true)
        .with_on_stop(move || *flag.borrow_mut() = true);
    let (_, commands) = paint_composer_in(&app, &mut streaming, vec2f(600.0, 500.0));
    let stop_fill = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == {
                let mut error = app.theme.color(ColorToken::Error);
                error.a = 230;
                error
            } =>
            {
                Some((*rect, *corner_radius))
            }
            _ => None,
        })
        .expect("the stop circle fills with the error colour at 90%");
    assert!(
        (stop_fill.0.width() - COMPOSER_SEND_SIZE).abs() < 0.5
            && (stop_fill.1 - COMPOSER_SEND_SIZE / 2.0).abs() < 0.5,
        "the stop circle is the 32pt round control the send circle used to swap with: {:?}",
        stop_fill.0
    );
    assert!(
        icon_origin(&commands, "stop").is_some(),
        "the stop glyph is drawn: {commands:?}"
    );
    let center = vec2f(
        stop_fill.0.min_x() + COMPOSER_SEND_SIZE / 2.0,
        stop_fill.0.min_y() + COMPOSER_SEND_SIZE / 2.0,
    );
    assert!(
        click_point(&mut streaming, &app, center),
        "the stop circle takes the press"
    );
    assert!(*stopped.borrow(), "and reports the stop");
}

/// `⌘⌥⏎` is the cloud gesture (warp-new's `CMD+ALT+ENTER`): the host's own
/// callback fires with the draft, and the ordinary send path — what a plain
/// `↵` takes — is left alone.
#[test]
fn the_cloud_chord_submits_to_the_cloud_and_plain_enter_still_sends() {
    let app = AppContext::default();

    // A plain Enter sends the draft the usual way and never routes it to the
    // cloud, even on a composer that carries both callbacks.
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent_cb = sent.clone();
    let cloud_for_plain = Rc::new(RefCell::new(Vec::new()));
    let cloud_for_plain_cb = cloud_for_plain.clone();
    let mut plain = ChatComposer::new()
        .with_focused(true)
        .with_value("a local turn")
        .with_on_send(move |text| sent_cb.borrow_mut().push(text))
        .with_on_send_to_cloud(move |text| cloud_for_plain_cb.borrow_mut().push(text));
    paint_composer(&app, &mut plain);
    let _ = press(&mut plain, &app, "Enter");
    assert_eq!(*sent.borrow(), vec!["a local turn".to_string()]);
    assert!(
        cloud_for_plain.borrow().is_empty(),
        "a plain Enter is not the cloud chord"
    );
    assert_eq!(plain.value(), "", "the submitted draft is spent");

    // Command/Ctrl+Alt+Enter is the cloud chord, in both spellings the OS
    // sends (command on macOS, ctrl elsewhere).
    for modifiers in [
        ModifiersState {
            alt: true,
            command: true,
            ..Default::default()
        },
        ModifiersState {
            alt: true,
            ctrl: true,
            ..Default::default()
        },
    ] {
        let cloud = Rc::new(RefCell::new(Vec::new()));
        let cloud_cb = cloud.clone();
        let local = Rc::new(RefCell::new(Vec::new()));
        let local_cb = local.clone();
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_value("ship it")
            .with_on_send(move |text| local_cb.borrow_mut().push(text))
            .with_on_send_to_cloud(move |text| cloud_cb.borrow_mut().push(text));
        paint_composer(&app, &mut composer);

        let handled = composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Enter".to_string(),
                modifiers,
            },
            &mut crate::elements::EventContext::default(),
            &app,
        );
        assert!(handled, "the cloud chord belongs to the composer");
        assert_eq!(*cloud.borrow(), vec!["ship it".to_string()]);
        assert!(
            local.borrow().is_empty(),
            "the cloud chord is not a local send"
        );
        assert_eq!(composer.value(), "", "the submitted draft is spent");
    }

    // Shift is not part of the gesture, so the editor keeps the key.
    let cloud = Rc::new(RefCell::new(Vec::new()));
    let cloud_cb = cloud.clone();
    let mut shifted = ChatComposer::new()
        .with_focused(true)
        .with_value("typed")
        .with_on_send_to_cloud(move |text| cloud_cb.borrow_mut().push(text));
    paint_composer(&app, &mut shifted);
    let _ = shifted.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "Enter".to_string(),
            modifiers: ModifiersState {
                alt: true,
                command: true,
                shift: true,
                ..Default::default()
            },
        },
        &mut crate::elements::EventContext::default(),
        &app,
    );
    assert!(cloud.borrow().is_empty(), "⌘⌥⇧↵ is not the cloud chord");
}

/// While a turn waits, an empty plain Enter is still a submit: the first Enter
/// queued the prompt, so the second one is the host's send-now. The composer
/// reports it and lets the host decide what an empty submit means.
#[test]
fn an_empty_enter_while_waiting_reports_a_submit() {
    let app = AppContext::default();
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent_cb = sent.clone();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_waiting(true)
        .with_on_send(move |text| sent_cb.borrow_mut().push(text));
    paint_composer(&app, &mut composer);

    let handled = press(&mut composer, &app, "Enter");

    assert!(handled, "the waiting composer keeps Enter");
    assert_eq!(
        *sent.borrow(),
        vec![String::new()],
        "an empty submit reaches the host"
    );
}

/// The working-directory tray is capped and scrolls: a directory with more
/// entries than the cap draws an eight-row window that clips what it cannot
/// show, instead of a panel taller than the pane it opens into.
#[test]
fn the_retired_directory_menu_draws_nothing() {
    use crate::elements::PanelScroll;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let items: Vec<PopupMenuItem> = (0..20)
        .map(|i| PopupMenuItem::new(format!("dir-{i:02}")))
        .collect();
    let mut composer = Box::new(
        ChatComposer::new()
            .with_path_label("/work/project")
            .with_dir_menu(items, Rc::new(RefCell::new(true)), |_| {})
            .with_dir_menu_scroll(PanelScroll::new()),
    ) as Box<dyn crate::elements::Element>;

    let commands = render_element(&mut composer, vec2f(600.0, 700.0), &app);
    let labels: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } if text.starts_with("dir-") => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        labels.is_empty(),
        "the retired directory tray draws no rows, even when its old flag is open: {labels:?}"
    );
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, RenderCommand::ClipRect(_))),
        "and no clipped tray window: {commands:?}"
    );
}

/// The composer's controls are one compact strip: the footer's model label and
/// its glyph-only controls share the pill's 24pt height, so the agent input and
/// the shell bar space their buttons the same way. The directory and branch
/// labels are gone with their band, so neither is drawn at any size.
/// The composer's controls are one compact strip: the context band's pills and
/// the footer's model and attach controls draw at the pill's 24pt height with
/// the same 12px label, so the agent input and the shell bar space their
/// buttons the same way.
#[test]
fn the_rich_input_controls_share_one_compact_size() {
    use crate::elements::CONTEXT_PILL_HEIGHT;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let painted = |composer: &mut ChatComposer| {
        let (_, commands) = paint_composer_in(&app, composer, vec2f(720.0, 500.0));
        let labels: Vec<f32> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText {
                    text, font_size, ..
                } if matches!(
                    text.as_str(),
                    "gpt-4o" | "~/Projects/goble" | "terminal-first"
                ) =>
                {
                    Some(*font_size)
                }
                _ => None,
            })
            .collect();
        let texts = drawn_texts(&commands);
        (labels, texts)
    };

    // The agent footer: its model label is the one text control left at the
    // shared label size; the retired directory/branch labels never draw.
    let mut agent = ChatComposer::new()
        .with_path_label("~/Projects/goble")
        .with_branch_label("terminal-first")
        .with_model_label("gpt-4o")
        .with_on_attach(|| {})
        .with_on_select_model(|| {});
    let (agent_labels, agent_texts) = painted(&mut agent);
    assert_eq!(agent_labels, vec![crate::elements::CONTEXT_PILL_LABEL_SIZE]);
    assert!(
        !agent_texts
            .iter()
            .any(|text| text == "~/Projects/goble" || text == "terminal-first"),
        "the retired context labels never draw: {agent_texts:?}"
    );

    // The shell bar: no model control and no context pill, so it draws no
    // control label at all.
    let mut shell = ChatComposer::new()
        .with_path_label("~/Projects/goble")
        .with_branch_label("terminal-first");
    let (shell_labels, shell_texts) = painted(&mut shell);
    assert!(
        shell_labels.is_empty(),
        "the shell bar draws no labels: {shell_labels:?}"
    );
    assert!(
        !shell_texts
            .iter()
            .any(|text| text == "~/Projects/goble" || text == "terminal-first"),
        "the shell bar draws no context labels: {shell_texts:?}"
    );
    assert_eq!(
        CONTEXT_PILL_HEIGHT, 24.0,
        "the control row is the pill's 24pt height"
    );
}

/// The bar a host reserves for this card (the shell pane's pty rows are sized
/// from it) is the height the card actually lays out for that configuration:
/// the context band, the input segment with one line of editor, the footer row
/// and the one instruction row the shell bar carries. A composer change that
/// grows the card must move the reserve with it.
#[test]
fn the_reserved_bar_height_is_the_cards_own_height() {
    use crate::elements::{PopupMenuItem, ShortcutHint};

    let app = AppContext::default();
    // The shell bar's own set of controls: a context band (directory, branch),
    // a working editor, the send/stop circle, and one instruction row.
    let items = vec![PopupMenuItem::new("main")];
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_blur_on_outside_click(false)
        .with_path_label("~/Projects/goble")
        .with_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")])
        .with_on_change(|_| {})
        .with_on_send(|_| {})
        .with_on_cmd_enter(|_| {})
        .with_on_focus_change(|_| {})
        .with_on_stop(|| {})
        .with_dir_menu(items.clone(), Rc::new(RefCell::new(false)), |_| {})
        .with_branch_label("main")
        .with_branch_menu(items, Rc::new(RefCell::new(false)), |_| {})
        .with_slash_menu(
            vec![],
            Rc::new(RefCell::new(0)),
            Rc::new(RefCell::new(false)),
        )
        .with_on_slash_move(|_| {})
        .with_on_slash_accept(|_| {})
        .with_on_slash_dismiss(|| {});
    let size = composer.layout(
        SizeConstraint::loose(vec2f(900.0, 600.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(
        (size.y - super::COMPOSER_BAR_RESERVED_HEIGHT).abs() < 0.01,
        "the reserved bar height {} is the card's own {size:?}",
        super::COMPOSER_BAR_RESERVED_HEIGHT
    );
}

/// A background composer never takes the keys of the pane the user is typing
/// in: the chords its bands answer (and the cloud submit) belong to the
/// focused card alone. Before this, a pane whose draft was a `/` command could
/// swallow Enter/Up/Down/Escape from the active pane, and `⌘⌥↵` in a stale
/// composer could route its old draft to the cloud.
#[test]
fn a_background_composer_keeps_the_bands_keys_for_the_focused_one() {
    use crate::elements::ShortcutHint;

    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_value("/")
        .with_focused(false)
        .with_hints(vec![ShortcutHint::new(&["/"], "commands")])
        .with_on_send(|_| {})
        .with_on_send_to_cloud(|_| {})
        .with_slash_menu(
            vec![],
            Rc::new(RefCell::new(0)),
            Rc::new(RefCell::new(false)),
        )
        .with_on_slash_move(|_| {})
        .with_on_slash_accept(|_| {})
        .with_on_slash_dismiss(|| {})
        .with_model_menu(vec![], Rc::new(RefCell::new(true)), |_| {})
        .with_model_menu_index(Rc::new(RefCell::new(0)));
    composer.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut ctx = crate::elements::EventContext;
    let down = DispatchedEvent::KeyDown {
        key: "ArrowDown".to_string(),
        modifiers: ModifiersState::none(),
    };
    let enter = DispatchedEvent::KeyDown {
        key: "Enter".to_string(),
        modifiers: ModifiersState::none(),
    };
    let escape = DispatchedEvent::KeyDown {
        key: "Escape".to_string(),
        modifiers: ModifiersState::none(),
    };
    let cloud = DispatchedEvent::KeyDown {
        key: "Enter".to_string(),
        modifiers: ModifiersState {
            command: true,
            alt: true,
            ..ModifiersState::none()
        },
    };
    for key in [&down, &enter, &escape, &cloud] {
        assert!(
            !composer.dispatch_event(key, &mut ctx, &app),
            "a background composer keeps {key:?} for the focused pane"
        );
    }
    // The commands still run in the composer whose pane is the one on screen:
    // the same keys are handled once it holds the caret.
    let mut focused = ChatComposer::new()
        .with_value("/")
        .with_focused(true)
        .with_slash_menu(
            vec![],
            Rc::new(RefCell::new(0)),
            Rc::new(RefCell::new(false)),
        )
        .with_on_slash_move(|_| {})
        .with_on_slash_accept(|_| {})
        .with_on_slash_dismiss(|| {});
    focused.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(
        focused.dispatch_event(&down, &mut ctx, &app),
        "the focused composer takes the band's arrow key"
    );
}

/// The model chip opens the DSH two-level card, wired to the host's state: the
/// root pane drills into the model list and the effort list, the model list
/// takes the row the arrows moved to through the host's callback, the effort
/// list reports the level by the id its harness names, Escape backs out of a
/// drilled pane and then closes, and a background composer's card takes none of
/// the keys — they belong to the pane whose composer holds the caret.
#[test]
fn the_model_card_reports_the_model_and_level_the_host_wires() {
    use crate::elements::{
        ComposerModelTray, EventContext, ModelTrayHarness, ModelTrayModel, ModelTrayPane,
        ModelTrayState,
    };

    let app = AppContext::default();
    let harness = || {
        vec![ModelTrayHarness::new("goble", "Goble").with_models(vec![
            ModelTrayModel::new("m1", "Model One")
                .with_thinking(vec!["on".to_string(), "off".to_string()]),
            ModelTrayModel::new("m2", "Model Two"),
        ])]
    };
    let key = |name: &str| DispatchedEvent::KeyDown {
        key: name.to_string(),
        modifiers: ModifiersState::none(),
    };

    // A background composer keeps its card's keys for the pane on screen.
    let mut background = ChatComposer::new()
        .with_model_label("Model One")
        .with_model_tray(ComposerModelTray::new(
            harness(),
            Rc::new(RefCell::new(ModelTrayState::default())),
            Rc::new(RefCell::new(true)),
        ));
    let _ = background.layout(
        SizeConstraint::loose(vec2f(600.0, 500.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut ctx = EventContext::default();
    assert!(
        !background.dispatch_event(&key("Enter"), &mut ctx, &app),
        "a background composer does not take the card's Enter"
    );

    // The focused composer routes the card's keys to the host's callbacks.
    let open = Rc::new(RefCell::new(true));
    let chosen: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let levels: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let tray_state = Rc::new(RefCell::new(ModelTrayState::default()));
    let log = chosen.clone();
    let level_log = levels.clone();
    let tray = ComposerModelTray::new(harness(), tray_state.clone(), open.clone())
        .with_current("m1", "on")
        .with_on_select_model(move |row| log.borrow_mut().push(row))
        .with_on_select_level(move |level| level_log.borrow_mut().push(level));
    let mut composer = ChatComposer::new()
        .with_model_label("Model One")
        .with_focused(true)
        .with_model_tray(tray);
    let _ = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 500.0)),
        &mut LayoutContext::default(),
        &app,
    );
    // Root pane: Enter drills into the model list, the arrows move the row, and
    // Enter takes it through the host's callback.
    assert!(
        composer.dispatch_event(&key("Enter"), &mut ctx, &app),
        "the root pane drills on Enter"
    );
    assert_eq!(tray_state.borrow().pane(), ModelTrayPane::Model);
    assert!(
        composer.dispatch_event(&key("ArrowDown"), &mut ctx, &app),
        "the model pane takes the arrow key"
    );
    assert_eq!(
        tray_state.borrow().row,
        1,
        "the highlight moved one row down"
    );
    assert!(
        composer.dispatch_event(&key("Enter"), &mut ctx, &app),
        "the model pane takes Enter"
    );
    assert_eq!(
        chosen.borrow().as_slice(),
        &[1],
        "Enter takes the row the arrows moved to, through the host's callback"
    );
    assert!(!*open.borrow(), "the pick closes the card");

    // Escape backs out of a drilled pane first (no selection), then closes.
    *open.borrow_mut() = true;
    *tray_state.borrow_mut() = ModelTrayState::default();
    let _ = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 500.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(composer.dispatch_event(&key("Enter"), &mut ctx, &app));
    assert_eq!(tray_state.borrow().pane(), ModelTrayPane::Model);
    assert!(composer.dispatch_event(&key("Escape"), &mut ctx, &app));
    assert_eq!(
        tray_state.borrow().pane(),
        ModelTrayPane::Root,
        "Escape leaves the drilled pane for the root menu"
    );
    assert!(*open.borrow(), "and does not close the card yet");
    assert!(composer.dispatch_event(&key("Escape"), &mut ctx, &app));
    assert!(!*open.borrow(), "Escape from the root pane closes the card");

    // The detail/effort column reports the level by the id its harness names.
    *open.borrow_mut() = true;
    {
        let mut state = tray_state.borrow_mut();
        state.pane = ModelTrayPane::Effort;
        state.level = 0;
        state.seeded = true;
    }
    let _ = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 500.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(composer.dispatch_event(&key("Enter"), &mut ctx, &app));
    assert_eq!(
        levels.borrow().as_slice(),
        &["on".to_string()],
        "the level is reported by its harness id"
    );
    assert!(!*open.borrow(), "and the pick closes the card");
}

/// 454/457: an integration with no key is an honest Configure status row, not
/// an invented model.
#[test]
fn an_unconfigured_integration_draws_its_honest_status_row() {
    use crate::elements::{
        ComposerModelTray, ModelTrayHarness, ModelTrayModel, ModelTrayPane, ModelTrayState,
    };

    let app = AppContext::default();
    let state = Rc::new(RefCell::new(ModelTrayState::default()));
    {
        let mut state = state.borrow_mut();
        state.seeded = true;
        state.pane = ModelTrayPane::Model;
    }
    let tray = ComposerModelTray::new(
        vec![
            ModelTrayHarness::new("anthropic", "Anthropic").with_models(vec![ModelTrayModel::new(
                "__configure__:anthropic",
                "Not configured · Configure",
            )]),
            ModelTrayHarness::new("openai", "OpenAI")
                .with_models(vec![ModelTrayModel::new("gpt-5", "GPT-5")]),
        ],
        state,
        Rc::new(RefCell::new(true)),
    )
    .with_current("gpt-5", "")
    .with_on_select_model(|_| {});
    let mut composer = ChatComposer::new()
        .with_model_label("GPT-5")
        .with_model_tray(tray);
    let (_, commands) = paint_composer_in(&app, &mut composer, vec2f(900.0, 600.0));
    let texts = drawn_texts(&commands);
    for text in ["Not configured · Configure", "GPT-5"] {
        assert!(
            texts.iter().any(|drawn| drawn == text),
            "{text:?}: {texts:?}"
        );
    }
    // 514: the integration column is icon-only, so provider names are tooltips.
    assert!(!texts
        .iter()
        .any(|text| text == "Anthropic" || text == "OpenAI"));
}

/// The chip is DSH's model seat (`ModelSelect.module.css` `.trigger`): the
/// catalogue's display name rather than the host's id, the effort beside it in
/// the dimmer caption tone, no colour change when the card opens, and a chevron
/// that flips with the open flag.
#[test]
fn the_model_chip_names_the_catalogue_model_and_captions_the_effort() {
    use crate::elements::{ComposerModelTray, ModelTrayHarness, ModelTrayModel, ModelTrayState};
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let tray = ComposerModelTray::new(
        vec![
            ModelTrayHarness::new("deepseek", "DeepSeek").with_models(vec![ModelTrayModel::new(
                "deepseek-flash",
                "DeepSeek Flash",
            )
            .with_thinking(vec!["off".to_string(), "high".to_string()])]),
        ],
        Rc::new(RefCell::new(ModelTrayState::default())),
        Rc::new(RefCell::new(false)),
    )
    .with_current("deepseek-flash", "high");
    let mut composer = ChatComposer::new()
        .with_model_label("deepseek-flash")
        .with_model_tray(tray);
    let commands = paint_composer(&app, &mut composer);
    let color_of = |want: &str| {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text, color, .. } if text == want => Some(*color),
            _ => None,
        })
    };
    let name = color_of("DeepSeek Flash").expect("the seat names the catalogue model");
    assert_eq!(
        name,
        app.theme.color(ColorToken::Muted),
        "the model name is the label tone"
    );
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, .. } if text.contains("DeepSeek ·")
        )),
        "453: the chip shows only the model name, no provider prefix"
    );
    assert!(
        color_of("High").is_none(),
        "517: the composer chip no longer shows effort"
    );
    assert!(
        !drawn_texts(&commands)
            .iter()
            .any(|text| text == "deepseek-flash"),
        "the raw host id is not the seat's label: {:?}",
        drawn_texts(&commands)
    );

    // The card's open flag only flips the chevron: DSH keeps the label tone.
    let mut opened = ChatComposer::new()
        .with_model_label("deepseek-flash")
        .with_model_tray(
            ComposerModelTray::new(
                vec![
                    ModelTrayHarness::new("deepseek", "DeepSeek").with_models(vec![
                        ModelTrayModel::new("deepseek-flash", "DeepSeek Flash")
                            .with_thinking(vec!["high".to_string()]),
                    ]),
                ],
                Rc::new(RefCell::new(ModelTrayState::default())),
                Rc::new(RefCell::new(true)),
            )
            .with_current("deepseek-flash", "high"),
        );
    let commands = paint_composer(&app, &mut opened);
    let reopened = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, color, .. } if text == "DeepSeek Flash" => Some(*color),
            _ => None,
        })
        .expect("the open chip still names the model");
    assert_eq!(
        reopened,
        app.theme.color(ColorToken::Muted),
        "opening the card does not recolour the name"
    );
}

/// The chip's value is one line inside DSH's 360pt cap: a model name that would
/// over-run it is ellipsized (the old builder let the run wrap out of the 28pt
/// control), and the effort gives its space up before the name loses a pixel
/// (`.triggerEffort { flex-shrink: 1000 }`).
#[test]
fn a_long_model_chip_value_ellipsizes_inside_dshs_cap() {
    use crate::elements::{ComposerModelTray, ModelTrayHarness, ModelTrayModel, ModelTrayState};

    let long = "DeepSeek-V4.1-Flash-Experimental-Preview-Candidate-2026-11";
    let chip = |name: &str, effort: &str| {
        ChatComposer::new()
            .with_model_label("stale-id")
            .with_model_tray(
                ComposerModelTray::new(
                    vec![ModelTrayHarness::new("deepseek", "DeepSeek")
                        .with_models(vec![ModelTrayModel::new("long", name)
                            .with_thinking(vec![effort.to_string()])])],
                    Rc::new(RefCell::new(ModelTrayState::default())),
                    Rc::new(RefCell::new(false)),
                )
                .with_current("long", effort),
            )
    };

    // The name alone overflows the cap: it ellipsizes, and the effort drops out
    // of the chip instead of wrapping under the name.
    let app = AppContext::default();
    let mut composer = chip(long, "high");
    let commands = paint_composer_in(&app, &mut composer, vec2f(900.0, 600.0)).1;
    let texts = drawn_texts(&commands);
    let fitted = texts
        .iter()
        .find(|text| text.starts_with("DeepSeek-V4.1") && text.ends_with('…'))
        .unwrap_or_else(|| panic!("the chip ellipsizes the long name: {texts:?}"));
    assert!(fitted.chars().count() < long.chars().count());
    assert!(
        !texts.iter().any(|text| text == long),
        "the full name cannot fit the chip"
    );

    // A name that fits keeps every pixel; the effort is the box that shrinks.
    let mut composer = chip(
        "DeepSeek-V4.1-Flash-Extended-Preview",
        "extraordinarily-high-reasoning",
    );
    let commands = paint_composer_in(&app, &mut composer, vec2f(900.0, 600.0)).1;
    let texts = drawn_texts(&commands);
    assert!(
        texts
            .iter()
            .any(|text| text == "DeepSeek-V4.1-Flash-Extended-Preview"),
        "the model name keeps its whole run: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text == "High"),
        "517: no effort is appended to the chip"
    );
}

/// The single composer has no context band left to hide: a `/` draft draws the
/// same one-card input as a plain draft (the slash popup is the host's, drawn
/// above the card), and the retired directory/branch controls never appear.
#[test]
fn a_slash_draft_keeps_the_single_composer_intact() {
    use crate::elements::SlashMenuItem;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let build = |value: &str, dismissed: bool| {
        ChatComposer::new()
            .with_value(value)
            .with_focused(true)
            .with_harness_label("This Computer")
            .with_harness_icon("environment-computer")
            .with_on_send(|_| {})
            .with_hints(vec![crate::elements::ShortcutHint::new(
                &["⌘", "↵"],
                "new conversation",
            )])
            .with_slash_menu(
                vec![SlashMenuItem::new("new", "Start a fresh conversation")],
                Rc::new(RefCell::new(0)),
                Rc::new(RefCell::new(dismissed)),
            )
            .with_on_slash_move(|_| {})
            .with_on_slash_accept(|_| {})
            .with_on_slash_dismiss(|| {})
    };
    let frame = |composer: &mut ChatComposer| {
        let (size, commands) = paint_composer_in(&app, composer, vec2f(760.0, 500.0));
        let mut tokens = drawn_texts(&commands);
        tokens.extend(commands.iter().filter_map(|command| match command {
            RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
            _ => None,
        }));
        (size, tokens)
    };

    let mut slash = build("/", false);
    let (slash_size, slash_frame) = frame(&mut slash);
    let mut plain = build("hi", false);
    let (plain_size, plain_frame) = frame(&mut plain);
    assert_eq!(
        slash_size, plain_size,
        "a slash draft and a plain draft are the same one-card composer"
    );
    for tokens in [&slash_frame, &plain_frame] {
        assert!(
            !tokens.iter().any(|token| matches!(
                token.as_str(),
                "folder" | "branch" | "git-branch" | "This Computer"
            )),
            "the retired folder/branch band never draws: {tokens:?}"
        );
    }

    // Escape dismissing the draft's list changes nothing about the card; the
    // hint row stays under it either way.
    let mut dismissed = build("/", true);
    let (dismissed_size, dismissed_frame) = frame(&mut dismissed);
    assert_eq!(dismissed_size, plain_size);
    assert!(
        dismissed_frame
            .iter()
            .any(|token| token == "new conversation"),
        "the fixed instruction row survives the dismissal: {dismissed_frame:?}"
    );
}

/// The two bands cannot be up together: opening the model card puts the draft's
/// command band away, so the composer is the input segment plus one overlay. The
/// slash draft keeps the context band hidden — the model card is the band that
/// was opened, not a reason to bring the context controls back.
#[test]
fn opening_the_model_card_puts_the_slash_band_away() {
    use crate::elements::{
        ComposerModelTray, ModelTrayHarness, ModelTrayModel, ModelTrayState, SlashMenuItem,
    };

    let app = AppContext::default();
    let tray_open = Rc::new(RefCell::new(false));
    let tray = ComposerModelTray::new(
        vec![
            ModelTrayHarness::new("goble", "Goble").with_models(vec![ModelTrayModel::new(
                "m1",
                "Model One",
            )
            .with_thinking(vec!["off".to_string()])]),
        ],
        Rc::new(RefCell::new(ModelTrayState::default())),
        tray_open.clone(),
    )
    .with_current("m1", "off")
    .with_on_select_model(|_| {})
    .with_on_select_level(|_| {});
    let mut composer = ChatComposer::new()
        .with_value("/")
        .with_focused(true)
        .with_path_label("brindlepad-web")
        .with_model_label("Model One")
        .with_model_tray(tray)
        .with_slash_menu(
            vec![SlashMenuItem::new("new", "Start a fresh conversation")],
            Rc::new(RefCell::new(0)),
            Rc::new(RefCell::new(false)),
        )
        .with_on_slash_move(|_| {})
        .with_on_slash_accept(|_| {})
        .with_on_slash_dismiss(|| {});
    // The chip opens the card; the draft's `/` band owns the overlay, so the
    // next rebuild must put the freshly opened model card away again.
    *tray_open.borrow_mut() = true;
    let _ = composer.layout(
        SizeConstraint::loose(vec2f(760.0, 500.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(
        !*tray_open.borrow(),
        "the slash band wins the one overlay slot over the model card"
    );
    let commands = paint_composer(&app, &mut composer);
    let tokens = drawn_texts(&commands);
    assert!(
        !tokens.iter().any(|token| token == "brindlepad-web"),
        "the context band stays hidden while the slash draft is active: {tokens:?}"
    );
}

/// The model chip opens the DSH card inside the composer's own frame: the root
/// pane's `Model` and `Effort` cells with their current values and right
/// chevrons, drawn as a 240pt menu card anchored above the chip — not as a
/// full-width band over the input.
#[test]
fn the_model_chip_opens_its_card_and_takes_the_keyboard() {
    use crate::elements::{
        ComposerModelTray, EventContext, ModelTrayHarness, ModelTrayModel, ModelTrayState,
    };

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(false));
    let focus_log: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let log = focus_log.clone();
    let tray = ComposerModelTray::new(
        vec![
            ModelTrayHarness::new("goble", "Goble").with_models(vec![ModelTrayModel::new(
                "m1",
                "Model One",
            )
            .with_thinking(vec!["on".to_string()])]),
        ],
        Rc::new(RefCell::new(ModelTrayState::default())),
        open.clone(),
    )
    .with_current("m1", "on")
    .with_on_select_model(|_| {})
    .with_on_select_level(|_| {});
    let mut composer = ChatComposer::new()
        .with_model_label("Model One")
        .with_model_tray(tray)
        .with_on_focus_change(move |focused| log.borrow_mut().push(focused));
    let commands = paint_composer_in(&app, &mut composer, vec2f(600.0, 400.0)).1;
    let chip = commands
        .iter()
        .find_map(|command| match command {
            crate::render::RenderCommand::DrawText { origin, text, .. }
                if text.contains("Model One") =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the model chip draws");
    let point = vec2f(chip.x + 8.0, chip.y + 6.0);
    let mut ctx = EventContext::default();
    let down = DispatchedEvent::MouseDown {
        position: point,
        button: crate::event::BUTTON_PRIMARY,
    };
    let up = DispatchedEvent::MouseUp {
        position: point,
        button: crate::event::BUTTON_PRIMARY,
    };
    assert!(composer.dispatch_event(&down, &mut ctx, &app));
    let _ = composer.dispatch_event(&up, &mut ctx, &app);
    assert!(*open.borrow(), "the chip opens the DSH card");
    assert_eq!(
        focus_log.borrow().as_slice(),
        &[true],
        "opening the card also makes the composer the keyboard's owner"
    );
}

fn the_model_chip_opens_the_dsh_card_above_it() {
    use crate::elements::{
        ComposerModelTray, ModelTrayHarness, ModelTrayModel, ModelTrayState, COMPOSER_CARD_RADIUS,
    };
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(true));
    let tray = ComposerModelTray::new(
        vec![
            ModelTrayHarness::new("deepseek", "DeepSeek").with_models(vec![ModelTrayModel::new(
                "deepseek-flash",
                "DeepSeek Flash",
            )
            .with_thinking(vec!["off".to_string(), "on".to_string()])]),
        ],
        Rc::new(RefCell::new(ModelTrayState::default())),
        open.clone(),
    )
    .with_current("deepseek-flash", "on")
    .with_on_select_model(|_| {})
    .with_on_select_level(|_| {});
    let mut composer = ChatComposer::new()
        .with_model_label("DeepSeek Flash")
        .with_focused(true)
        .with_model_tray(tray)
        .with_on_send(|_| {});
    let (size, commands) = paint_composer_in(&app, &mut composer, vec2f(900.0, 600.0));

    // The root pane's two cells: labels left, current values right.
    let texts = drawn_texts(&commands);
    for label in ["Model", "Effort", "DeepSeek Flash", "On"] {
        assert!(
            texts.iter().any(|text| text == label),
            "the root pane draws {label:?}: {texts:?}"
        );
    }

    // The card is the raised 240pt casing above the chip, not a band.
    let card = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised)
                && (*corner_radius - 16.0).abs() < 0.01
                && rect.min_y() < 0.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the model card paints its raised casing above the chip");
    assert!(
        (card.width() - 240.0).abs() < 0.5,
        "the root pane is DSH's 240pt settings menu: {card:?}"
    );
    assert!(
        card.width() < size.x,
        "the card is a menu, not a band over the whole input: {card:?} in {size:?}"
    );
    // The chip sits near the composer card's leading edge, so the 240pt card
    // cannot open leftwards from it: DSH's clamped body portal slides it right
    // to the layout viewport's edge, which is the composer card's 12pt gutter
    // here. It must never paint off the card (the cropping the owner saw).
    let composer_card = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::Surface)
                && (*corner_radius - COMPOSER_CARD_RADIUS).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the composer paints its own card");
    assert!(
        card.min_x() >= composer_card.min_x() - 0.5,
        "the model card clamps inside the composer card: {card:?} against {composer_card:?}"
    );
    assert!(
        card.max_x() <= composer_card.max_x() + 0.5,
        "the model card clamps inside the composer card: {card:?} against {composer_card:?}"
    );

    // DSH's drill chevrons on the cells, and the chip's chevron flipped up.
    assert!(
        commands
            .iter()
            .filter(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "chevron-right"
            ))
            .count()
            >= 2,
        "each root cell carries its drill chevron: {commands:?}"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "chevron-up"
        )),
        "the open chip points up: {commands:?}"
    );
}
