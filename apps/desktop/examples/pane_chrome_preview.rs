//! Headless raster previews and measurements of the pane surface, the panel
//! inset and the resize affordances.
//!
//! The live native window cannot be captured in this environment, so this
//! example builds the real app tree (the same `RootView` the window draws) with
//! panes that have work in flight (so the dock renders) and rasterizes it to
//! PNGs through the shared preview rasterizer. It is a visual verification tool,
//! not a product surface. It measures:
//!
//! - the AGENT SURFACE: the pane card's whole background, the text/content
//!   column and the gutters outside it are the same darker `Bg` token, so
//!   outside == inside (dark and light are the theme's own tokens);
//! - the PANEL SURFACE INSET: the area the panes are rendered on sits 4 px
//!   inside the window/chrome on all four sides (top: below the bar block;
//!   right/bottom: the window edges; left: the sidebar), and each tab outline
//!   keeps its own inset relative to its slot;
//! - the RESIZE affordance: no thin divider rule is drawn at a split boundary
//!   or at the sidebar edge — idle or while dragging — and the only affordance
//!   is the resize cursor over the invisible grab band.
//!
//! Scenes: 1 pane (1600x900, lane dragged to 700), 2 panes (2600x900, ratio
//! 0.6), 1 narrow pane (1024x768, lane inactive), and 2 panes with the split and
//! the sidebar edge both grabbed (2600x900) — the dragging frames must draw no
//! rule either.
//!
//! Usage: cargo run -p goble-app --example pane_chrome_preview
//! Output: /tmp/goble-work/out/pane-chrome-<scene>.png, with the measured
//! geometry, colours, panel insets and cursor probes printed to stdout.

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_app::state::{PaneSession, UiState};
use goble_app::ui::shell::{TOPBAR_BOTTOM_BAND, TOPBAR_HEIGHT};
use goble_app::ui::{Pane, PaneKind, Space, SplitDir};
use goble_core::harness::ToolCallStatus;
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{
    lane_column, AppContext, Element, LayoutContext, PaintContext, SizeConstraint,
};
use goble_ui::geometry::{rectf, RectF, Vector2F};
use goble_ui::platform::CursorKind;
use goble_ui::render::{RenderCommand, Renderer};
use goble_ui::theme::ColorToken;
use goble_ui::{vec2f, ChatFragment, ChatMessage, ChatRole, ToolCall};

const OUT_DIR: &str = "/tmp/goble-work/out";
const SCALE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scene {
    OnePane,
    TwoPanes,
    Narrow,
    ResizeChrome,
}

impl Scene {
    fn all() -> [Self; 4] {
        [
            Self::OnePane,
            Self::TwoPanes,
            Self::Narrow,
            Self::ResizeChrome,
        ]
    }

    fn size(self) -> Vector2F {
        match self {
            Self::OnePane => vec2f(1600.0, 900.0),
            Self::TwoPanes | Self::ResizeChrome => vec2f(2600.0, 900.0),
            Self::Narrow => vec2f(1024.0, 768.0),
        }
    }

    fn file(self) -> &'static str {
        match self {
            Self::OnePane => "pane-chrome-1pane.png",
            Self::TwoPanes => "pane-chrome-2pane.png",
            Self::Narrow => "pane-chrome-narrow.png",
            Self::ResizeChrome => "pane-chrome-resize-chrome.png",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::OnePane => {
                "1 pane, 1600x900: uniform darker agent surface, lane dragged to 700"
            }
            Self::TwoPanes => {
                "2 panes, 2600x900 ratio 0.6: each pane's chrome spans its own slot, no divider"
            }
            Self::Narrow => {
                "1 narrow pane, 1024x768: lane inactive, uniform agent surface, panel inset"
            }
            Self::ResizeChrome => {
                "2 panes with the split grabbed and the sidebar edge grabbed: cursor-only resize"
            }
        }
    }
}

fn chat_session() -> PaneSession {
    PaneSession {
        conversation_id: String::new(),
        draft: String::new(),
        path: String::new(),
        daemon_session_id: String::new(),
    }
}

fn chat_leaf(id: u64) -> Box<Pane> {
    Box::new(Pane::Leaf {
        id,
        kind: PaneKind::Chat,
    })
}

fn install_two_panes(state: &mut UiState, ratio: f32) {
    state.spaces = vec![Space::new(
        "Chrome preview",
        Pane::Split {
            id: 9,
            dir: SplitDir::Horizontal,
            ratio,
            first: chat_leaf(7),
            second: chat_leaf(8),
        },
    )];
    state.active_space = 0;
    state.active_pane_id = 7;
    for id in [7u64, 8] {
        state.pane_sessions.insert(id, chat_session());
    }
}

fn work_call(id: &str, command: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: "run_command".to_string(),
        arguments: format!(r#"{{"command":"{command}"}}"#),
        status: ToolCallStatus::Running,
        result: None,
        diff: None,
    }
}

/// The real app tree over a fresh in-memory store, in the scene's pane layout
/// and the normal (non-first-run) chrome, with work in flight in the active
/// pane so the work dock band renders.
fn build_scene(
    app: &AppContext,
    scene: Scene,
) -> anyhow::Result<(Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir)> {
    let dir = tempfile::tempdir()?;
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory()?,
        ThreadStore::new(dir.path())?,
    ));
    let view = RootView::new(app, &desktop, None);
    let state = view.state_rc();
    let root: Box<dyn Element> = Box::new(view);
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.show_onboarding_tip = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
        s.chat_messages = vec![
            ChatMessage::new(
                ChatRole::User,
                vec![ChatFragment::text("Where does the pane surface end?")],
            ),
            ChatMessage::new(
                ChatRole::Assistant,
                vec![ChatFragment::text(
                    "The whole agent pane is one darker surface; the text column is only a width.",
                )],
            ),
        ];
        match scene {
            Scene::OnePane | Scene::Narrow => {}
            Scene::TwoPanes => install_two_panes(&mut s, 0.6),
            Scene::ResizeChrome => {
                install_two_panes(&mut s, 0.6);
                // Both resizable boundaries are grabbed: the frames must draw
                // no rule at either one.
                s.dragging_pane_id = Some(9);
                s.sidebar_dragging = true;
            }
        }
        // A pane snapshot is what makes the pane draw its own chat view (and
        // with it the work dock); every scene gives its panes one.
        let pane_id = s.active_pane_id;
        if scene == Scene::OnePane {
            // The owner's scene: agent mode with the width dragged narrower —
            // the whole surface stays the same darker background.
            s.content_lane_prefs.insert(pane_id, 700.0);
        }
        s.pane_sessions.entry(pane_id).or_insert_with(chat_session);
        let runtime = s.pane_runtime.entry(pane_id).or_default();
        runtime
            .in_flight_tools
            .insert("call-1".to_string(), work_call("call-1", "cargo test -p goble-ui"));
        runtime.in_flight_tools.insert(
            "call-2".to_string(),
            work_call("call-2", "cargo test -p goble-app"),
        );
    }
    Ok((root, state, dir))
}

fn paint_frame(app: &AppContext, root: &mut Box<dyn Element>, size: Vector2F) -> Vec<RenderCommand> {
    let _ = root.layout(SizeConstraint::loose(size), &mut LayoutContext::default(), app);
    let mut ctx = PaintContext::new(Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut ctx, app);
    ctx.renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// Run one more paint of the already-built frame with the pointer at `pointer`,
/// so the cursor requests the tree makes are observable, and return the cursor
/// the platform was asked for.
fn probe_cursor(
    app: &AppContext,
    root: &mut Box<dyn Element>,
    pointer: Vector2F,
) -> CursorKind {
    let mut ctx = PaintContext::new(Renderer::new());
    ctx.cursor_inside = true;
    ctx.cursor_position = pointer;
    root.paint(vec2f(0.0, 0.0), &mut ctx, app);
    app.window_control.cursor()
}

/// The colour of the last fill covering `point`, i.e. what the frame leaves
/// visible there.
fn topmost_fill(commands: &[RenderCommand], point: Vector2F) -> Option<goble_ui::ColorU> {
    commands.iter().rev().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. }
            if point.x >= rect.min_x()
                && point.x <= rect.max_x()
                && point.y >= rect.min_y()
                && point.y <= rect.max_y() =>
        {
            Some(*color)
        }
        _ => None,
    })
}

/// The square `Surface` chrome bands the chrome paints: the header band (bar
/// plus the pane frame's inset) and the dock band below it.
fn chrome_bands(commands: &[RenderCommand], app: &AppContext) -> Vec<RectF> {
    let surface = app.theme.color(ColorToken::Surface);
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == surface
                && *corner_radius == 0.0
                // The body's bands only: the topbar's own surface runs above
                // the body origin.
                && rect.min_y() >= TOPBAR_HEIGHT
                && rect.width() > rect.height() * 4.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect()
}

/// Every thin one-axis rule the frame paints: a fill no thicker than 1.5 px and
/// at least 40 px long. These are exactly the divider lines a resize boundary
/// must not draw; the pane card frames are strokes, not fills, and are counted
/// separately.
fn thin_rules(commands: &[RenderCommand]) -> Vec<RectF> {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, .. }
                if rect.min_y() >= TOPBAR_HEIGHT
                    && ((rect.width() <= 1.5 && rect.height() >= 40.0)
                        || (rect.height() <= 1.5 && rect.width() >= 40.0)) =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect()
}

/// The bounding rectangle of the tab slots: the tab area the tabs tile.
fn tab_area(slots: &[RectF]) -> Option<RectF> {
    let mut rects = slots.iter().copied();
    let first = rects.next()?;
    let (mut min_x, mut min_y, mut max_x, mut max_y) =
        (first.min_x(), first.min_y(), first.max_x(), first.max_y());
    for rect in rects {
        min_x = min_x.min(rect.min_x());
        min_y = min_y.min(rect.min_y());
        max_x = max_x.max(rect.max_x());
        max_y = max_y.max(rect.max_y());
    }
    Some(rectf(min_x, min_y, max_x - min_x, max_y - min_y))
}

fn pane_bounds(state: &Rc<RefCell<UiState>>) -> Vec<(u64, RectF)> {
    let s = state.borrow();
    let bounds = s.pane_bounds.borrow().clone();
    let mut entries: Vec<(u64, RectF)> = bounds.into_iter().collect();
    entries.sort_by_key(|(id, _)| *id);
    entries
}

/// A `#rrggbb` form of a theme colour, for the printed geometry.
fn color_hex(color: goble_ui::ColorU) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

fn main() -> anyhow::Result<()> {
    std::fs::create_dir_all(OUT_DIR)?;
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/goble-ui/assets")
        .canonicalize()?;
    let assets = raster::PreviewAssets::load(&assets_dir)?;
    let app = AppContext::default();

    for scene in Scene::all() {
        let (mut root, state, _dir) = build_scene(&app, scene)?;
        let size = scene.size();
        // One frame lays the app out (the snapshot needs it), the next is the
        // frame measured and painted.
        let _ = paint_frame(&app, &mut root, size);
        let commands = paint_frame(&app, &mut root, size);
        println!("\n== {:?} — {}", scene, scene.label());
        print_geometry(&state, &commands, &app, size);
        print_cursor_probes(&app, &mut root, &state, size);

        let out = Path::new(OUT_DIR).join(scene.file());
        raster::render_element_to_png(&assets, &app, &mut root, size.x, size.y, SCALE, None, &out)?;
        println!("   -> {}", out.display());
    }
    Ok(())
}

fn print_geometry(
    state: &Rc<RefCell<UiState>>,
    commands: &[RenderCommand],
    app: &AppContext,
    window: Vector2F,
) {
    let s = state.borrow();
    let bg = app.theme.color(ColorToken::Bg);
    let surface = app.theme.color(ColorToken::Surface);
    let body_top = TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND;
    let body_left = if s.sidebar_visible {
        s.sidebar_width
    } else {
        0.0
    };
    let bands = chrome_bands(commands, app);
    let mut slots: Vec<RectF> = Vec::new();
    for (id, pane) in pane_bounds(state) {
        let pref = s.content_lane_prefs.get(&id).copied();
        let (gutter, lane) = lane_column(pane.width(), pref);
        println!(
            "\n   pane {id}: card x {:.1}..{:.1} y {:.1}..{:.1} ({}x{})",
            pane.min_x(),
            pane.max_x(),
            pane.min_y(),
            pane.max_y(),
            pane.width(),
            pane.height(),
        );
        println!(
            "     lane: x {:.1}..{:.1} (w {:.1}, gutter {:.1})",
            pane.min_x() + gutter,
            pane.min_x() + gutter + lane,
            lane,
            gutter,
        );

        // The whole card's fill, whatever token it is.
        let card_color = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if (rect.width() - pane.width()).abs() < 0.5
                        && (rect.height() - pane.height()).abs() < 0.5
                        && (rect.min_x() - pane.min_x()).abs() < 0.5 =>
                {
                    Some(*color)
                }
                _ => None,
            })
            .unwrap_or(bg);
        // What is actually visible in the gutter beside the text column and
        // inside it, at the pane's mid height.
        let mid_y = pane.min_y() + pane.height() / 2.0;
        let outside_x = if gutter > 8.0 {
            pane.min_x() + gutter / 2.0
        } else {
            pane.min_x() + 2.0
        };
        let inside_x = pane.min_x() + gutter + lane / 2.0;
        let outside = topmost_fill(commands, vec2f(outside_x, mid_y));
        let inside = topmost_fill(commands, vec2f(inside_x, mid_y));
        let uniform = outside == inside && outside == Some(card_color);
        println!(
            "     agent surface: card {} | outside text column {:?} | inside text column {:?} -> uniform? {}",
            color_hex(card_color),
            outside.map(color_hex),
            inside.map(color_hex),
            uniform
        );

        // The pane's own chrome bands: the header band at the top of its slot
        // and (when work is in flight) the dock band under it.
        let over: Vec<RectF> = bands
            .iter()
            .copied()
            .filter(|band| {
                band.min_x() <= pane.min_x() + 12.0 && band.max_x() >= pane.max_x() - 12.0
            })
            .collect();
        for band in &over {
            println!(
                "     band: x {:.1}..{:.1} (w {:.1}) y {:.1}..{:.1} (h {:.1}) r=0",
                band.min_x(),
                band.max_x(),
                band.width(),
                band.min_y(),
                band.max_y(),
                band.height(),
            );
        }
        let header = over
            .iter()
            .min_by(|a, b| a.min_y().total_cmp(&b.min_y()))
            .copied();
        if let Some(header) = header {
            // The tab's slot is the header band's full-bleed rectangle: the
            // card grown by the pane frame's gutter on every side.
            let card_gutter = pane.min_x() - header.min_x();
            let slot = rectf(
                header.min_x(),
                header.min_y(),
                header.width(),
                (pane.max_y() + card_gutter) - header.min_y(),
            );
            slots.push(slot);
            println!(
                "     tab slot: x {:.1}..{:.1} y {:.1}..{:.1} | card gutter inside it: {:.1}",
                slot.min_x(),
                slot.max_x(),
                slot.min_y(),
                slot.max_y(),
                card_gutter,
            );

            // The tab outline keeps its 3 pt inset relative to its slot.
            let outline = commands.iter().find_map(|command| match command {
            RenderCommand::StrokeRect {
                rect,
                color,
                width,
                corner_radius,
            } if (*width - 1.0).abs() < 0.01
                && (*corner_radius - 6.0).abs() < 0.01
                && (*color == app.theme.color(ColorToken::Accent)
                    || *color == app.theme.color(ColorToken::Border))
                && rect.min_x() <= header.min_x() + 3.5
                && rect.max_x() >= header.max_x() - 3.5 =>
            {
                Some(*rect)
            }
            _ => None,
        });
            if let Some(outline) = outline {
                println!(
                    "     tab outline: x {:.1}..{:.1} y {:.1}..{:.1} | slot inset L {:.1} T {:.1} R {:.1} B {:.1}",
                    outline.min_x(),
                    outline.max_x(),
                    outline.min_y(),
                    outline.max_y(),
                    outline.min_x() - header.min_x(),
                    outline.min_y() - header.min_y(),
                    header.max_x() - outline.max_x(),
                    slot.max_y() - outline.max_y(),
                );
            }
        }

        // Header row extents: the controls stay on the pane's edges even with
        // an adjustable lane.
        let close = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, size, .. } if name == "x-close" => {
                    Some((origin.x + size / 2.0, origin.y + size / 2.0))
                }
                _ => None,
            })
            .filter(|(x, y)| *y >= pane.min_y() && *y <= pane.min_y() + TOPBAR_HEIGHT)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let esc = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == "esc" => Some(*origin),
                _ => None,
            })
            .filter(|origin| {
                origin.y >= pane.min_y() - 4.0
                    && origin.y <= pane.min_y() + TOPBAR_HEIGHT
                    && origin.x >= pane.min_x() - 4.0
                    && origin.x <= pane.max_x() + 4.0
            })
            .min_by(|a, b| a.x.total_cmp(&b.x));
        if let (Some(close), Some(esc)) = (close, esc) {
            println!(
                "     topbar row: esc hint x {:.1} (pane left {:.1}) | close X x {:.1} (pane right {:.1}) -> full width? {}",
                esc.x,
                pane.min_x(),
                close.0,
                pane.max_x(),
                esc.x < pane.min_x() + 60.0 && close.0 > pane.max_x() - 40.0
            );
        }
    }

    // The panel surface is the whole tab area: the bounding rectangle of the
    // tabs' slots. It is inset from the body/chrome on all four sides.
    if let (Some(first), Some(area)) = (slots.first(), tab_area(&slots)) {
        let _ = first;
        println!(
            "\n   panel surface area: x {:.1}..{:.1} y {:.1}..{:.1} | inset from body L {:.1} T {:.1} R {:.1} B {:.1} -> 4/4/4/4? {}",
            area.min_x(),
            area.max_x(),
            area.min_y(),
            area.max_y(),
            area.min_x() - body_left,
            area.min_y() - body_top,
            window.x - area.max_x(),
            window.y - area.max_y(),
            (area.min_x() - body_left - 4.0).abs() < 0.01
                && (area.min_y() - body_top - 4.0).abs() < 0.01
                && (window.x - area.max_x() - 4.0).abs() < 0.01
                && (window.y - area.max_y() - 4.0).abs() < 0.01,
        );
    }

    // No drawn rules at the resizable boundaries — neither idle nor while a
    // drag is in flight (the ResizeChrome scene grabs both).
    let rules = thin_rules(commands);
    println!(
        "\n   thin divider rules in the frame: {} (split/sidebar must be 0){}",
        rules.len(),
        if rules.is_empty() {
            String::new()
        } else {
            format!(" -> {rules:?}")
        }
    );
    let frames = commands
        .iter()
        .filter(|command| {
            matches!(
                command,
                RenderCommand::StrokeRect { width, .. } if (*width - 1.0).abs() < 0.01
            )
        })
        .count();
    println!(
        "   pane card / outline strokes (frames, not resize rules): {frames}"
    );
    let surfaces = commands
        .iter()
        .filter(|command| {
            matches!(
                command,
                RenderCommand::FillRect { color, .. } if *color == surface
            )
        })
        .count();
    println!(
        "   Surface fills: {surfaces} | Bg fills: {} (the card is Bg for agent panes)",
        commands
            .iter()
            .filter(|command| matches!(
                command,
                RenderCommand::FillRect { color, .. } if *color == bg
            ))
            .count()
    );
}

/// The resize affordance is cursor-only at the boundaries: probe the split edge,
/// the sidebar edge and a point over the pane content, and print the cursor the
/// platform was asked for.
fn print_cursor_probes(
    app: &AppContext,
    root: &mut Box<dyn Element>,
    state: &Rc<RefCell<UiState>>,
    window: Vector2F,
) {
    let (sidebar_width, dragging) = {
        let s = state.borrow();
        (
            s.sidebar_width,
            s.dragging_pane_id.is_some() || s.sidebar_dragging,
        )
    };
    let body_top = TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND;
    let mid_y = (body_top + window.y) / 2.0;

    // The sidebar's edge, in the middle of its invisible grab band.
    let sidebar_edge = vec2f(sidebar_width, mid_y);
    println!(
        "   cursor at the sidebar edge {:.1}: {:?} (want EwResize)",
        sidebar_width,
        probe_cursor(app, root, sidebar_edge)
    );

    // The split boundary: the panes are side by side, so it is a vertical edge
    // half-way between the neighbouring cards.
    let panes = pane_bounds(state);
    if let (Some(first), Some(second)) = (panes.first(), panes.get(1)) {
        let boundary = (first.1.max_x() + second.1.min_x()) / 2.0;
        println!(
            "   cursor at the split boundary {:.1}: {:?} (want EwResize)",
            boundary,
            probe_cursor(app, root, vec2f(boundary, mid_y))
        );
        // Off the grab band, over the first pane's content: the arrow, unless a
        // drag is in flight (then the captured pointer keeps the resize arrow).
        let over_content = vec2f(
            first.1.min_x() + first.1.width() / 2.0,
            first.1.min_y() + first.1.height() / 2.0,
        );
        println!(
            "   cursor over the pane content (off the band): {:?} (want {})",
            probe_cursor(app, root, over_content),
            if dragging { "EwResize" } else { "Default" }
        );
    }
}
