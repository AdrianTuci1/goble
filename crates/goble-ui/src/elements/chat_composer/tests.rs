use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::AppContext;
use crate::elements::Element;
use crate::elements::LayoutContext;
use crate::elements::PopupMenuItem;
use crate::elements::SizeConstraint;
use crate::elements::COMPOSER_CONTROL_RADIUS;
use crate::event::DispatchedEvent;
use crate::event::ModifiersState;
use crate::geometry::vec2f;
use crate::theme::ColorToken;
use goble_core::harness::CommandDecision;
use super::*;

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

/// Every control the rich input owns sits in one footer row under the editor:
/// the context pill and attach, then the model and stop, then the mode badge,
/// left to right. Nothing but the draft is above the editor.
#[test]
fn composer_puts_every_control_in_the_footer_below_the_editor() {
    use crate::elements::PaintContext;
    use crate::render::{RenderCommand, Renderer};

    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_model_label("gpt-4o")
        .with_path_label("~/Projects/goble")
        .with_on_attach(|| {})
        .with_on_select_model(|| {})
        .with_on_stop(|| {})
        .with_stop_visible(true);

    let size = composer.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);

    let mut paint_ctx = PaintContext::new(Renderer::new());
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();

    let icon_y = |name: &str| -> Option<f32> {
        commands.iter().find_map(|c| match c {
            RenderCommand::DrawIcon { origin, name: n, .. } if n == name => Some(origin.y),
            _ => None,
        })
    };
    let text_y = |text: &str| -> Option<f32> {
        commands.iter().find_map(|c| match c {
            RenderCommand::DrawText { origin, text: t, .. } if t == text => Some(origin.y),
            _ => None,
        })
    };

    let plus = icon_y("plus").expect("attach pill icon");
    let model = text_y("gpt-4o").expect("model control label");
    let editor = text_y("hi").expect("editor text");
    let folder = icon_y("folder").expect("directory pill icon");

    assert!(
        editor < folder,
        "the directory pill sits below the editor: {folder} vs {editor}"
    );
    assert!(
        editor < plus,
        "the attach button sits below the editor: {plus} vs {editor}"
    );
    assert!(
        editor < model,
        "the model control sits below the editor: {model} vs {editor}"
    );
    // One row: the directory pill, attach and the model share the footer's
    // line, within the few pixels their different heights account for.
    assert!(
        (plus - folder).abs() < 20.0,
        "attach is on the directory pill's line: {folder} vs {plus}"
    );
    assert!(
        (model - folder).abs() < 20.0,
        "the model is on the directory pill's line: {folder} vs {model}"
    );

    // The account button is gone from the rich input.
    assert!(icon_y("user").is_none(), "no account icon in the rich input");

    // Every control in the row is a card: the directory pill, attach, the model
    // and stop each outline their own box in the theme's grey, at the theme's
    // corner radius, and none of them fills it while the pointer is away.
    let borders: Vec<(crate::color::ColorU, f32, f32)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::StrokeRect {
                color,
                width,
                corner_radius,
                ..
            } => Some((*color, *width, *corner_radius)),
            _ => None,
        })
        .collect();
    assert_eq!(
        borders.len(),
        4,
        "the directory pill, attach, the model and stop are four cards, got {borders:?}"
    );
    for (color, width, radius) in &borders {
        assert_eq!(
            *color,
            app.theme.color(ColorToken::Border),
            "a card is outlined in the theme's grey"
        );
        assert_eq!(*width, 1.0, "the outline is 1px");
        assert_eq!(*radius, COMPOSER_CONTROL_RADIUS, "at the input's own corner radius");
    }
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { color, .. } if *color == app.theme.color(ColorToken::Hover)
        )),
        "a card draws no fill until the pointer is over it: {commands:?}"
    );
}

#[test]
fn composer_renders_dir_and_branch_pills_without_the_environment_pill() {
    use crate::elements::PaintContext;
    use crate::render::{RenderCommand, Renderer};

    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_harness_label("grok build")
        .with_path_label("/work/project")
        .with_branch_label("main")
        .with_harness_menu(
            vec![PopupMenuItem::new("grok build")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_dir_menu(
            vec![PopupMenuItem::new("/work/project")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_branch_menu(
            vec![PopupMenuItem::new("main")],
            Rc::new(RefCell::new(false)),
            |_| {},
        );

    let size = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);

    let mut paint_ctx = PaintContext::new(Renderer::new());
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();

    let icons: Vec<String> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    // folder + git-branch are the two context pills the rich input still draws.
    for expected in ["folder", "git-branch"] {
        assert!(icons.iter().any(|n| n == expected), "missing icon {expected}");
    }
    // The environment pill (`computer` -> the agentmode glyph) is gone, even
    // though the host still sets its label and its menu.
    assert!(
        !icons.iter().any(|n| n == "agentmode"),
        "the environment pill is not drawn: {icons:?}"
    );
    assert!(
        !drawn_texts(&commands).iter().any(|t| t == "grok build"),
        "and its label is not drawn either: {:?}",
        drawn_texts(&commands)
    );

    // The directory and branch pills are two cards: each outlines its own box
    // in the theme's grey, at the theme's corner radius.
    let borders: Vec<(crate::color::ColorU, f32, f32)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::StrokeRect {
                color,
                width,
                corner_radius,
                ..
            } => Some((*color, *width, *corner_radius)),
            _ => None,
        })
        .collect();
    assert_eq!(borders.len(), 2, "one card per pill, got {borders:?}");
    for (color, width, radius) in &borders {
        assert_eq!(
            *color,
            app.theme.color(ColorToken::Border),
            "a card is outlined in the theme's grey"
        );
        assert_eq!(*width, 1.0, "the outline is 1px");
        assert_eq!(*radius, COMPOSER_CONTROL_RADIUS, "at the input's own corner radius");
    }
}

/// A rich-input control is a card: the row's own box outlined in the theme's
/// grey border while the pointer is away, and the theme's hover fill once the
/// pointer is over it. The outline is all it draws at rest.
#[test]
fn a_rich_input_control_is_a_card_that_fills_on_hover() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let border = app.theme.color(ColorToken::Border);
    let hover = app.theme.color(ColorToken::Hover);
    let mut composer = ChatComposer::new().with_path_label("~/Projects/goble");

    let rest = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0)).1;
    let card = card_of(&rest, "folder");
    let (color, width, radius) = rest
        .iter()
        .find_map(|c| match c {
            RenderCommand::StrokeRect {
                rect,
                color,
                width,
                corner_radius,
            } if *rect == card => Some((*color, *width, *corner_radius)),
            _ => None,
        })
        .expect("the directory pill outlines its own box");
    assert_eq!(color, border, "the card's outline is the theme's grey");
    assert_eq!(width, 1.0, "the outline is 1px");
    assert_eq!(radius, COMPOSER_CONTROL_RADIUS, "at the input's own corner radius");
    assert!(
        !rest.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { color, .. } if *color == hover
        )),
        "a card fills only under the pointer: {rest:?}"
    );

    // The pointer onto the card: the same box fills with the hover grey, inside
    // the outline that is still drawn around it.
    let cursor = vec2f(
        (card.min_x() + card.max_x()) / 2.0,
        (card.min_y() + card.max_y()) / 2.0,
    );
    let over = paint_composer_with_pointer(&app, &mut composer, vec2f(600.0, 500.0), Some(cursor));
    assert!(
        over.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { rect, color, corner_radius }
                if *rect == card && *color == hover && *corner_radius == COMPOSER_CONTROL_RADIUS
        )),
        "the pointer over the card fills it with the hover grey: {over:?}"
    );
}

/// The rich input's controls carry no drop-down chevron: the card itself is the
/// trigger, and the tray still opens from a click on it. Each control is still
/// drawn by its own glyph — and the model control by none at all: it names the
/// model the draft runs on, so an icon beside the name would be decoration.
#[test]
fn the_rich_input_controls_draw_no_drop_down_chevron() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_harness_label("grok build")
        .with_path_label("~/Projects/goble")
        .with_branch_label("main")
        .with_model_label("gpt-4o")
        .with_on_attach(|| {})
        .with_harness_menu(
            vec![PopupMenuItem::new("grok build")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_dir_menu(
            vec![PopupMenuItem::new("~/Projects/goble")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_branch_menu(
            vec![PopupMenuItem::new("main")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_model_menu(
            vec![PopupMenuItem::new("gpt-4o")],
            Rc::new(RefCell::new(false)),
            |_| {},
        );

    let commands = paint_composer_in(&app, &mut composer, vec2f(760.0, 500.0)).1;
    let mut icons: Vec<String> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    icons.sort();
    assert_eq!(
        icons,
        [
            "folder",     // the working-directory pill
            "git-branch", // the branch pill
            "plus",       // attach
        ],
        "every control draws its own glyph and none of them a chevron, \
         the environment pill included"
    );
    assert!(
        !icons.iter().any(|name| name == "sparkle"),
        "the model control carries no icon: it reads as the model's own name"
    );
    assert!(
        !icons.iter().any(|name| name == "agentmode"),
        "the environment pill is gone from the rich input"
    );
}

/// The box the control that drew `text` outlines: the border around the text's
/// own origin, the way [`card_of`] finds a control's box from its icon.
fn card_of_text(commands: &[crate::render::RenderCommand], text: &str) -> crate::geometry::RectF {
    use crate::render::RenderCommand;
    let origin = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text: drawn, .. } if drawn == text => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the {text} control is drawn"));
    commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::StrokeRect { rect, .. }
                if rect.min_x() <= origin.x
                    && origin.x <= rect.max_x()
                    && rect.min_y() <= origin.y
                    && origin.y <= rect.max_y() =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the {text} control outlines its own box"))
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
    host_frame(app, composer, slash, slash_index, slash_dismissed, models, window).1
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
) -> (
    Option<Box<dyn Element>>,
    Vec<crate::render::RenderCommand>,
) {
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
            let mut menu = crate::elements::ModelMenu::new(models.items.clone(), models.index.clone())
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
        .with_model_menu(
            two_models(),
            open.clone(),
            move |row| log.borrow_mut().push(row),
        )
        .with_model_menu_index(index.clone());

    // Closed, the control is its label and the list is not drawn.
    let (_, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    let control = card_of_text(&commands, "gpt-4o");
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawText { text, .. } if text == "o3-mini")),
        "a closed model menu draws no rows: {commands:?}"
    );

    let mut ctx = EventContext::default();
    assert!(click_at(&mut composer, &app, control), "the press reaches the control");
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
        panel.min_x().abs() < 0.5 && (panel.width() - size.x).abs() < 0.5,
        "the band spans the input's whole width: {panel:?} in {size:?}"
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
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawText { text, color, .. }
                if text == "gpt-4o" && *color == app.theme.color(ColorToken::Accent)
        )),
        "the model in force is the row that reads in the accent colour: {commands:?}"
    );

    // A second press on the control puts the list away, the way Escape does.
    // The band over the input moves the composer down, so the control's box is
    // read from the frame the composer is drawn at its own origin again.
    let (_, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));
    let control = card_of_text(&commands, "gpt-4o");
    assert!(click_at(&mut composer, &app, control), "the press reaches the control");
    assert!(!*open.borrow(), "a second press puts the band away");
    assert!(click_at(&mut composer, &app, control), "a third press opens it again");
    assert!(*open.borrow(), "the control holds the list open or shut");

    // The band holds the keys that take a row from it; the editor keeps every
    // other key and its own caret.
    assert_eq!(*index.borrow(), 0, "the band opens on the model in force");
    assert!(press(&mut composer, &app, "ArrowDown"), "Down belongs to the band");
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
    assert!(press(&mut composer, &app, "Tab"), "Tab takes the selected row");
    assert_eq!(*chosen.borrow(), vec![0], "the host is told which row was taken");
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
    assert_eq!(*taking.borrow(), vec![1], "the row that was pressed is the one taken");
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
    let control = card_of_text(&closed, "gpt-4o");
    assert!(click_at(&mut composer, &app, control), "the control takes the press");
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
    assert!(model_band_drawn(&frame), "the model band is drawn: {frame:?}");
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
    assert!(command_band_drawn(&frame), "the command list is drawn: {frame:?}");
    assert!(!*open.borrow(), "the model band is put away");
    assert!(
        !model_band_drawn(&frame),
        "and it is not drawn beside the command list: {frame:?}"
    );

    // The band that is up is the one that owns the keys.
    assert!(press(&mut composer, &app, "ArrowDown"), "Down belongs to a band");
    assert_eq!(*slash_index.borrow(), 1, "it moves the command list's row");
    assert_eq!(*model_index.borrow(), 0, "and leaves the model list's row alone");

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
    assert!(command_band_drawn(&frame), "the command list is drawn: {frame:?}");
    assert!(!model_band_drawn(&frame), "and the model band is not drawn: {frame:?}");

    // Opening the models puts that list away: one band, and it is this one.
    let control = card_of_text(&frame, "gpt-4o");
    assert!(click_at(&mut composer, &app, control), "the control takes the press");
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
    assert!(model_band_drawn(&frame), "the model band is drawn: {frame:?}");
    assert!(
        !command_band_drawn(&frame),
        "and the command list is not drawn with it: {frame:?}"
    );

    assert!(press(&mut composer, &app, "ArrowDown"), "Down belongs to a band");
    assert_eq!(*model_index.borrow(), 1, "it moves the model list's row");
    assert_eq!(*slash_index.borrow(), 0, "and leaves the command list's row alone");

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
    assert_eq!(composer.value(), "/xhe", "the letter lands where the caret was");
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
    assert!(command_band_drawn(&frame), "the command list is drawn again: {frame:?}");
    assert!(!*open.borrow(), "the model band is put away");
    assert!(!model_band_drawn(&frame), "and not drawn under it: {frame:?}");
    assert!(press(&mut composer, &app, "ArrowDown"), "Down belongs to a band");
    assert_eq!(*slash_index.borrow(), 1, "it moves the command list's row");
    assert_eq!(*model_index.borrow(), 1, "and leaves the model list's row alone");
}

#[test]
fn composer_context_menu_opens_and_paints_panel() {
    use crate::elements::PaintContext;
    use crate::render::{RenderCommand, Renderer};

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(true));
    let mut composer = ChatComposer::new()
        .with_path_label("/work/project")
        .with_dir_menu(
            vec![
                PopupMenuItem::new("/work/project"),
                PopupMenuItem::new("/work/other"),
            ],
            open,
            |_| {},
        );

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

    // An opened context selector draws its dropdown panel (a bordered
    // surface), confirming the rich-input pills actually open.
    let strokes = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
        .count();
    assert!(strokes >= 1, "opened context menu should paint its panel border");
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

fn text_color(
    commands: &[crate::render::RenderCommand],
    text: &str,
) -> Vec<crate::color::ColorU> {
    commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText {
                text: drawn, color, ..
            } if drawn == text => Some(*color),
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

/// The box the control that drew `icon` outlines: the border around the icon's
/// own origin. Every rich-input control is a card of its own, so the border
/// that encloses the glyph is the control's.
fn card_of(commands: &[crate::render::RenderCommand], icon: &str) -> crate::geometry::RectF {
    use crate::render::RenderCommand;
    let origin = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawIcon { origin, name, .. } if name == icon => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the {icon} control is drawn"));
    commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::StrokeRect { rect, .. }
                if rect.min_x() <= origin.x
                    && origin.x <= rect.max_x()
                    && rect.min_y() <= origin.y
                    && origin.y <= rect.max_y() =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the {icon} control outlines its own box"))
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

/// A release of the primary button at the middle of `rect`, which is when a
/// control in this tree takes a click.
fn click_at(composer: &mut ChatComposer, app: &AppContext, rect: crate::geometry::RectF) -> bool {
    composer.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: vec2f(
                rect.min_x() + rect.width() / 2.0,
                rect.min_y() + rect.height() / 2.0,
            ),
            button: 0,
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
fn proposal_card_shows_the_candidates_and_selects_the_first() {
    let app = AppContext::default();
    let decisions = decision_log();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_proposal(proposal(&["git status", "git diff --stat"]))
        .with_on_decision(record_into(decisions.clone()));
    let commands = paint_composer(&app, &mut composer);

    let texts = drawn_texts(&commands);
    for expected in [
        "Agent proposes a command",
        "git status",
        "git diff --stat",
        "/work/project",
        "Approve",
        "Reject",
    ] {
        assert!(texts.iter().any(|t| t == expected), "missing {expected:?}");
    }
    // The proposal is a band of native rows, so it draws no border at all.
    let strokes = commands
        .iter()
        .filter(|c| matches!(c, crate::render::RenderCommand::StrokeRect { .. }))
        .count();
    assert_eq!(strokes, 0, "the proposal draws no border");

    // The first candidate is selected: its row is the accent one and the
    // editor was seeded with it, so Enter has something to run.
    assert_eq!(composer.selected_candidate().as_deref(), Some("git status"));
    assert_eq!(composer.value(), "git status");
    let accent = app.theme.color(ColorToken::Accent);
    assert!(
        text_color(&commands, "git status").contains(&accent),
        "the selected candidate should be the accent row"
    );
    let unselected = text_color(&commands, "git diff --stat");
    assert!(!unselected.contains(&accent));
}

#[test]
fn arrow_keys_cycle_the_candidates_and_follow_the_draft() {
    let app = AppContext::default();
    let changes = Rc::new(RefCell::new(Vec::new()));
    let for_cb = changes.clone();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_proposal(proposal(&["git status", "git diff --stat"]))
        .with_on_change(move |text| for_cb.borrow_mut().push(text));
    paint_composer(&app, &mut composer);

    assert!(press(&mut composer, &app, "ArrowDown"));
    assert_eq!(
        composer.selected_candidate().as_deref(),
        Some("git diff --stat")
    );
    assert_eq!(composer.value(), "git diff --stat");
    // The cycle is a draft change too, so the host's own draft (which is
    // what the editor is rebuilt from) does not undo the selection.
    assert_eq!(*changes.borrow(), vec!["git diff --stat".to_string()]);

    // Wraps past the end back to the first, and the other way from the top.
    assert!(press(&mut composer, &app, "ArrowDown"));
    assert_eq!(composer.value(), "git status");
    assert!(press(&mut composer, &app, "ArrowUp"));
    assert_eq!(composer.value(), "git diff --stat");
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

    assert_eq!(composer.value(), "git diff --stat");
    assert_eq!(
        composer.selected_candidate().as_deref(),
        Some("git diff --stat")
    );

    assert!(press(&mut composer, &app, "ArrowUp"));
    assert_eq!(*selection.borrow(), 0);
    assert_eq!(composer.value(), "git status");
}

#[test]
fn editing_a_candidate_submits_the_edit() {
    let app = AppContext::default();
    let decisions = decision_log();
    let mut composer = ChatComposer::new()
        .with_focused(true)
        .with_proposal(proposal(&["git status"]))
        .with_on_decision(record_into(decisions.clone()));
    paint_composer(&app, &mut composer);

    // Type into the editor below the card — the in-place edit.
    for ch in " --short".chars() {
        assert!(press(&mut composer, &app, &ch.to_string()));
    }
    assert_eq!(composer.value(), "git status --short");

    assert!(press(&mut composer, &app, "Enter"));
    assert_eq!(
        *decisions.borrow(),
        vec![(
            "call-1".to_string(),
            CommandDecision::Edit("git status --short".to_string())
        )]
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
    assert_eq!(*focused.borrow(), vec![true], "and it does not re-report focus");
}

/// Narrowed, the footer's controls move onto a second line instead of being
/// drawn past the input's own edge — the warp-new behaviour at resize, where
/// the input column grows downwards rather than sideways.
#[test]
fn the_footer_wraps_instead_of_overflowing_the_input() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let composer = || {
        ChatComposer::new()
            .with_path_label("~/Projects/goble")
            .with_branch_label("main")
            .with_model_label("gpt-4o")
            .with_stop_visible(true)
            .with_on_stop(|| {})
            .with_on_select_model(|| {})
            .with_vim(Rc::new(RefCell::new(crate::vim::VimState::new())))
    };

    // Wide enough for the whole footer: one line.
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

    // Narrow: the same controls, wrapped, and nothing drawn past the input's
    // own width.
    let (narrow, narrow_commands) = paint_composer_in(&app, &mut composer(), vec2f(260.0, 500.0));
    assert!(
        model_y(&narrow_commands) > model_y(&wide_commands),
        "the model label moved to a second line: {} then {}",
        model_y(&wide_commands),
        model_y(&narrow_commands)
    );
    assert!(
        narrow.y > wide.y,
        "the wrapped footer is taller: {wide:?} then {narrow:?}"
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

/// The instructions the host hands the composer are drawn in the input's own
/// bottom row, under the editor and the action row: the one entry a surface
/// keeps inside its input rather than over the separator above it. The composer
/// hugs its content either way — the strip adds its own height and nothing else.
#[test]
fn the_instructions_close_the_input_under_the_action_row() {
    use crate::elements::ShortcutHint;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let line_of = |commands: &[RenderCommand], text: &str| -> Option<f32> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text: run, origin, .. } if run == text => Some(origin.y),
            _ => None,
        })
    };
    // The shell bar: its directory pill over the editor, its one instruction
    // under it, and no model or stop control in between.
    let bare = paint_composer_in(
        &app,
        &mut ChatComposer::new()
            .with_context_above_editor(true)
            .with_path_label("~/Projects/goble"),
        vec2f(600.0, 500.0),
    );
    let with_hints = paint_composer_in(
        &app,
        &mut ChatComposer::new()
            .with_context_above_editor(true)
            .with_path_label("~/Projects/goble")
            .with_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")]),
        vec2f(600.0, 500.0),
    );

    let (bare_size, bare_commands) = bare;
    let (size, commands) = with_hints;
    assert!(
        line_of(&bare_commands, "new conversation").is_none(),
        "an input handed no instructions draws none"
    );
    let editor = line_of(&commands, "Ask anything...").expect("the editor is drawn");
    let hint = line_of(&commands, "new conversation").expect("the instruction is drawn");
    assert!(
        hint > editor,
        "the instruction is under the editor, inside the input ({hint} against {editor})"
    );
    // The strip's own height is all it costs.
    let bare_editor = line_of(&bare_commands, "Ask anything...").expect("the editor is drawn");
    assert!(
        (bare_editor - editor).abs() < 1.0,
        "the editor does not move for the strip ({editor} against {bare_editor})"
    );
    assert!(
        size.y > bare_size.y,
        "the input grows by the strip's height ({} against {})",
        bare_size.y,
        size.y
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

/// The rich input's cursor does not move the letter it sits among: the beam
/// takes no room in the editor's row, so the text after it starts on the beam
/// rather than a caret's width past it; the beam is thicker, and it is painted
/// before the character it reaches, which stays legible on the focus fill.
#[test]
fn the_editor_caret_takes_no_room_and_covers_the_letter_it_reaches() {
    use crate::elements::caret::CARET_WIDTH;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let focus = app.theme.color(ColorToken::Focus);
    let caret = Rc::new(RefCell::new(1));
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_focused(true)
        .with_caret(caret);
    let commands = paint_composer(&app, &mut composer);

    let beam_at = commands
        .iter()
        .position(|command| matches!(command, RenderCommand::FillRect { color, .. } if *color == focus))
        .expect("the focused editor paints its beam");
    let beam = match &commands[beam_at] {
        RenderCommand::FillRect { rect, .. } => *rect,
        _ => unreachable!(),
    };
    let (tail_at, tail) = commands
        .iter()
        .enumerate()
        .find_map(|(index, command)| match command {
            RenderCommand::DrawText { origin, text, .. } if text == "i" => Some((index, *origin)),
            _ => None,
        })
        .expect("the text after the beam is drawn");
    assert!(
        (tail.x - beam.min_x()).abs() < 0.5,
        "the letter after the caret starts on the beam, not past it: {tail:?} against {beam:?}"
    );
    assert!(
        tail.x < beam.max_x(),
        "the beam reaches over that letter: {beam:?} against {tail:?}"
    );
    assert!(
        (beam.width() - CARET_WIDTH).abs() < 0.5 && CARET_WIDTH > 3.0,
        "the beam is a thick one: {beam:?}"
    );
    assert!(
        beam_at < tail_at,
        "the beam is painted before the letter it covers, so the letter stays drawn"
    );
    let said: String = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(said, "hi", "the caret leaves the editor's text alone");
}

/// The rich input hugs its own rows: the card's vertical padding is `xs` and
/// the block carries no vertical margin, so the first content row is `xs` under
/// the composer's top edge and the last one is `xs` above its bottom edge —
/// about 4pt where it used to be `md` + `xs`, 16pt. The horizontal stays `md`
/// on the card, so the editor's text keeps the conversation's text column.
#[test]
fn the_rich_input_keeps_only_xs_above_and_below_its_rows() {
    use crate::render::RenderCommand;
    use crate::theme::SpacingToken;

    let app = AppContext::default();
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    // The context row rides above the editor here, so the first card drawn is
    // the top row and the model card is the bottom one. The composer paints
    // from its own origin: its top edge is 0 and its bottom edge is `size.y`.
    let mut composer = ChatComposer::new()
        .with_context_above_editor(true)
        .with_path_label("~/Projects/goble")
        .with_model_label("gpt-4o")
        .with_on_select_model(|| {});
    let (size, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));

    let cards: Vec<crate::geometry::RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::StrokeRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect();
    let top = cards
        .iter()
        .map(|rect| rect.min_y())
        .fold(f32::INFINITY, f32::min);
    let bottom = cards
        .iter()
        .map(|rect| rect.max_y())
        .fold(f32::NEG_INFINITY, f32::max);

    assert!(
        (top - xs).abs() < 0.5,
        "the first row is {xs} under the composer's top edge, got {top}"
    );
    assert!(
        (size.y - bottom - xs).abs() < 0.5,
        "the last row is {xs} above the composer's bottom edge, got {} of {}",
        size.y - bottom,
        size.y
    );
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
        assert!(local.borrow().is_empty(), "the cloud chord is not a local send");
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

/// The working-directory tray is capped and scrolls: a directory with more
/// entries than the cap draws an eight-row window that clips what it cannot
/// show, instead of a panel taller than the pane it opens into.
#[test]
fn the_directory_tray_is_capped_and_clips_the_rows_it_cannot_show() {
    use crate::elements::{PanelScroll, MENU_MAX_VISIBLE_ROWS};
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
            RenderCommand::DrawText { text, .. } if text.starts_with("dir-") => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect();
    assert!(
        labels.len() > MENU_MAX_VISIBLE_ROWS,
        "the whole list is there to scroll through: {labels:?}"
    );
    // The rows past the cap are drawn but clipped away: the panel's own window
    // is the eight-row cap, not the twenty rows a directory can hold.
    let window = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::ClipRect(rect) => Some(rect.size.height),
            _ => None,
        })
        .expect("a capped tray clips its rows");
    let cap = MENU_MAX_VISIBLE_ROWS as f32 * 32.0 + (MENU_MAX_VISIBLE_ROWS as f32 - 1.0) * 2.0;
    assert!(
        window <= cap + 1.0,
        "twenty rows are drawn in an eight-row window, not {window} px"
    );
}
