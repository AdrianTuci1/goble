use super::*;

use goble_ui::elements::{LayoutContext, PaintContext, SizeConstraint};
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::render::{RenderCommand, Renderer};
use goble_ui::theme::ColorToken;

/// The message the agents tab's chip carries.
const CHIP: &str = "Agent conversations";

/// Lay the mounted root out and paint it, with the pointer at `cursor` when
/// given (and outside the window otherwise).
fn paint_frame(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    cursor: Option<Vector2F>,
) -> Vec<RenderCommand> {
    let _ = root.layout(
        SizeConstraint::loose(vec2f(1024.0, 768.0)),
        &mut LayoutContext::default(),
        app,
    );
    let mut ctx = PaintContext::new(Renderer::new());
    if let Some(position) = cursor {
        ctx.cursor_inside = true;
        ctx.cursor_position = position;
    }
    root.paint(vec2f(0.0, 0.0), &mut ctx, app);
    ctx.renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// Where the hovered chip's message is drawn, if the frame drew it at all.
fn chip_text_index(commands: &[RenderCommand]) -> Option<usize> {
    commands.iter().position(
        |command| matches!(command, RenderCommand::DrawText { text, .. } if text == CHIP),
    )
}

/// Where the sidebar and the pane on its right stop painting: the resize divider
/// between them is the last thing both of them draw.
fn divider_index(commands: &[RenderCommand], sidebar_width: f32) -> usize {
    commands
        .iter()
        .rposition(|command| match command {
            RenderCommand::FillRect { rect, .. } => {
                rect.width() == 1.0
                    && rect.height() > 500.0
                    && (rect.min_x() - (sidebar_width - 0.5)).abs() < 0.01
            }
            _ => false,
        })
        .expect("the sidebar's divider is painted")
}

/// The chip hovered on a sidebar toolbelt tab is drawn after the pane that sits
/// to the right of the sidebar — the element that used to cover it — and after
/// everything else the frame paints.
#[test]
fn a_hovered_sidebar_tab_chip_is_the_windows_last_paint() {
    let (view, _bus, _dir) = root_with_bus();
    let state = view.state_rc();
    let sidebar_width = state.borrow().sidebar_width;
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(view);

    // The pointer away: the frame draws no chip, and the tab to hover is found
    // by the icon it draws (14 pt, inside the sidebar — the topbar's chat tab
    // draws the same glyph at 16 pt).
    let commands = paint_frame(&mut root, &app, None);
    assert!(
        chip_text_index(&commands).is_none(),
        "a frame with nothing hovered draws no chip"
    );
    let tab = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon {
                origin,
                name,
                size,
                ..
            } if name == "message-chat-square"
                && *size == 14.0
                && origin.x < crate::ui::SIDEBAR_WIDTH =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the toolbelt draws the agents tab's icon");

    // The pointer rests on the tab: the chip is queued, not drawn in the tab's
    // own pass, and the root's layer draws it after the whole tree.
    let commands = paint_frame(&mut root, &app, Some(tab + vec2f(7.0, 7.0)));
    let chip = chip_text_index(&commands).expect("hovering the tab draws its chip");
    let divider = divider_index(&commands, sidebar_width);
    assert!(
        chip > divider,
        "the chip is drawn after the pane to the right of the sidebar (chip {chip}, divider {divider})"
    );
    let box_index = commands[..chip]
        .iter()
        .rposition(|command| {
            matches!(command, RenderCommand::FillRect { color, .. }
                if *color == app.theme.color(ColorToken::SurfaceRaised))
        })
        .expect("the layer draws the chip's box");
    assert!(
        box_index > divider,
        "the chip's box is drawn after the pane to the right of the sidebar (box {box_index}, divider {divider})"
    );
    assert_eq!(
        chip,
        commands.len() - 1,
        "the chip is the window's last paint"
    );
    assert!(
        app.hover_chips.borrow().is_empty(),
        "the layer empties the frame's chips"
    );

    // The pointer away again: no chip is left over from the hovered frame.
    let commands = paint_frame(&mut root, &app, None);
    assert!(
        chip_text_index(&commands).is_none(),
        "the next frame draws no leftover chip"
    );
}
