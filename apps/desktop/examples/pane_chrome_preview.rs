//! Headless raster previews and measurements of the pane surface, the panel
//! inset and the resize affordances.
//!
//! The live native window cannot be captured in this environment, so this
//! example builds the real app tree (the same `RootView` the window draws) with
//! panes that have work in flight (so the dock renders) and rasterizes it to
//! PNGs through the shared preview rasterizer. It is a visual verification tool,
//! not a product surface. It measures:
//!
//! - the LAYERING: the panel surface the panes are rendered on — the WHOLE pane
//!   area, the 4 px `PANE_AREA_INSET` frame itself included, and the gutters
//!   between/around the tab slots — is the SIDEBAR's `Surface`; only the panel
//!   itself (each pane/tab card) is the darker `Bg`. An agent card also matches
//!   its own content column (both `Bg`), so outside the text column reads like
//!   inside it, while the backdrop matches the sidebar. The measured pixels:
//!   the area's top/bottom/left/right edges, the gutter between two
//!   neighbouring tab slots, and the sidebar reference are all the same grey —
//!   no dark strip (not even the owner's leftover 3 px) on any edge;
//! - the TAB-SLOT INSET: the tab slots are inset 4 px inside the window/chrome
//!   on all four sides (top: below the bar block; right/bottom: the window
//!   edges; left: the sidebar), and each tab outline keeps its own inset
//!   relative to its slot. The SURFACE, unlike the slots, paints that whole
//!   frame: its fill starts on the body origin, not 4 px inside it;
//! - the CARD CORNERS: every pane card is cut to its 6 pt radius on all four
//!   corners — the four corner spandrels read as the panel surface, not as
//!   square card pixels — and the focused pane's accent triangle is clipped to
//!   the card's rounded top-left corner (no accent pixel outside the radius).
//!   Read off the rendered pixels and saved as 10x corner zooms for each card
//!   of the 1- and 2-pane scenes, in the dark and light themes;
//! - the RESIZE affordance: no thin divider rule is drawn at a split boundary
//!   or at the sidebar edge — idle or while dragging — and the only affordance
//!   is the resize cursor over the invisible grab band;
//! - the RESPONSIVE steps at 1600/1100/900/760: the sidebar auto-collapses at
//!   the width the pane tree needs (the 1024 floor earlier for a wide tree),
//!   the next breakpoint hides the agent panel; the panes themselves stop at
//!   the owner's 360 pt minimum.
//!
//! Scenes: 1 pane (1600x900, lane dragged to 700), 2 panes (2600x900, ratio
//! 0.6), 3 panes (2600x900, thirds), 1 narrow pane (1024x768, lane inactive), 2
//! panes with the split and the sidebar edge both grabbed (2600x900) — the
//! dragging frames must draw no rule either — and the same two-pane layout at
//! the responsive widths 1600/1100/900/760: the sidebar auto-collapses at its
//! measured breakpoint and the agent panel leaves the view below 768, mirroring
//! DeepSeek Harness's breakpoint behaviour.
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
use goble_app::terminal::TerminalSession;
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
    ThreePanes,
    Narrow,
    ResizeChrome,
    /// The same two-pane layout at the responsive widths: 1600 (all shown),
    /// 1100 (sidebar shown, still above the measured breakpoint), 900 (the
    /// sidebar auto-collapsed) and 760 (secondary panels leave the view).
    Responsive1600,
    Responsive1100,
    Responsive900,
    Responsive760,
    /// The agent panel (a secondary surface) open at a wide and at a narrow
    /// width: shown above 768, gone below it (the responsive second step).
    Panel1600,
    Panel760,
    /// The agent pane with its bottom terminal dock open (T3 Code's drawer).
    Dock,
}

impl Scene {
    fn all() -> [Self; 12] {
        [
            Self::OnePane,
            Self::TwoPanes,
            Self::ThreePanes,
            Self::Narrow,
            Self::ResizeChrome,
            Self::Responsive1600,
            Self::Responsive1100,
            Self::Responsive900,
            Self::Responsive760,
            Self::Panel1600,
            Self::Panel760,
            Self::Dock,
        ]
    }

    fn size(self) -> Vector2F {
        match self {
            Self::OnePane => vec2f(1600.0, 900.0),
            Self::TwoPanes | Self::ThreePanes | Self::ResizeChrome => vec2f(2600.0, 900.0),
            Self::Narrow => vec2f(1024.0, 768.0),
            Self::Responsive1600 => vec2f(1600.0, 900.0),
            Self::Responsive1100 => vec2f(1100.0, 900.0),
            Self::Responsive900 => vec2f(900.0, 900.0),
            Self::Responsive760 | Self::Panel760 => vec2f(760.0, 900.0),
            Self::Panel1600 => vec2f(1600.0, 900.0),
            Self::Dock => vec2f(1600.0, 900.0),
        }
    }

    fn file(self) -> &'static str {
        match self {
            Self::OnePane => "pane-chrome-1pane.png",
            Self::TwoPanes => "pane-chrome-2pane.png",
            Self::ThreePanes => "pane-chrome-3pane.png",
            Self::Narrow => "pane-chrome-narrow.png",
            Self::ResizeChrome => "pane-chrome-resize-chrome.png",
            Self::Responsive1600 => "responsive-1600.png",
            Self::Responsive1100 => "responsive-1100.png",
            Self::Responsive900 => "responsive-900.png",
            Self::Responsive760 => "responsive-760.png",
            Self::Panel1600 => "panel-1600.png",
            Self::Panel760 => "panel-760.png",
            Self::Dock => "panel-chrome-dock.png",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::OnePane => {
                "1 pane, 1600x900: panel card darker, backdrop = sidebar, lane dragged to 700"
            }
            Self::TwoPanes => {
                "2 panes, 2600x900 ratio 0.6: darker panels on the sidebar-coloured panel surface"
            }
            Self::ThreePanes => {
                "3 panes, 2600x900 thirds: the surface stays sidebar grey around all three"
            }
            Self::Narrow => {
                "1 narrow pane, 1024x768: lane inactive, darker panel on the sidebar-coloured backdrop"
            }
            Self::ResizeChrome => {
                "2 panes with the split grabbed and the sidebar edge grabbed: cursor-only resize"
            }
            Self::Responsive1600 => "responsive 1600: sidebar + two panes, everything shown",
            Self::Responsive1100 => "responsive 1100: sidebar still fits beside two panes",
            Self::Responsive900 => "responsive 900: sidebar auto-collapsed, panes keep width",
            Self::Responsive760 => {
                "responsive 760: sidebar collapsed, two panes at their 360 pt minimum"
            }
            Self::Panel1600 => "agent panel open at 1600: the secondary surface is shown",
            Self::Panel760 => "agent panel open at 760: the secondary surface leaves the view",
            Self::Dock => "agent pane with its bottom terminal dock open",
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

fn install_three_panes(state: &mut UiState) {
    state.spaces = vec![Space::new(
        "Chrome preview",
        Pane::Split {
            id: 9,
            dir: SplitDir::Horizontal,
            ratio: 1.0 / 3.0,
            first: chat_leaf(7),
            second: Box::new(Pane::Split {
                id: 10,
                dir: SplitDir::Horizontal,
                ratio: 0.5,
                first: chat_leaf(8),
                second: chat_leaf(11),
            }),
        },
    )];
    state.active_space = 0;
    state.active_pane_id = 7;
    for id in [7u64, 8, 11] {
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
            Scene::ThreePanes => install_three_panes(&mut s),
            Scene::ResizeChrome => {
                install_two_panes(&mut s, 0.6);
                // Both resizable boundaries are grabbed: the frames must draw
                // no rule at either one.
                s.dragging_pane_id = Some(9);
                s.sidebar_dragging = true;
            }
            Scene::Responsive1600
            | Scene::Responsive1100
            | Scene::Responsive900
            | Scene::Responsive760 => {
                install_two_panes(&mut s, 0.5);
            }
            Scene::Panel1600 | Scene::Panel760 => {
                install_two_panes(&mut s, 0.5);
                // The agent panel is the secondary surface the responsive
                // layout hides at the second breakpoint: open it so the step is
                // visible and measurable.
                s.right_sidebar_open = true;
            }
            Scene::Dock => {}
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
        if scene == Scene::Dock {
            // The app's own path opens a real PTY; a headless example has no
            // daemon to start, so it seeds the drawer with a representative
            // screen and renders the same surface.
            assert!(
                s.toggle_terminal_dock(pane_id),
                "the agent pane offers its header terminal dock"
            );
            s.set_terminal_dock_height(pane_id, 260.0);
            s.terminal.borrow_mut().sessions.insert(
                pane_id,
                TerminalSession::preview_screen(
                    "$ cargo test -p goble-ui --lib\r\n                     test result: ok. 916 passed; 0 failed\r\n                     $ git status --short\r\n                     ?? functii.py\r\n$ ",
                ),
            );
        }
        let runtime = s.pane_runtime.entry(pane_id).or_default();
        runtime.in_flight_tools.insert(
            "call-1".to_string(),
            work_call("call-1", "cargo test -p goble-ui"),
        );
        runtime.in_flight_tools.insert(
            "call-2".to_string(),
            work_call("call-2", "cargo test -p goble-app"),
        );
    }
    Ok((root, state, dir))
}

fn paint_frame(
    app: &AppContext,
    root: &mut Box<dyn Element>,
    size: Vector2F,
) -> Vec<RenderCommand> {
    let _ = root.layout(
        SizeConstraint::loose(size),
        &mut LayoutContext::default(),
        app,
    );
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
fn probe_cursor(app: &AppContext, root: &mut Box<dyn Element>, pointer: Vector2F) -> CursorKind {
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
        render_scene(&app, &assets, scene, "")?;
    }
    // The light theme's pane cards: the rounded edge and the clipped triangle
    // follow the theme's own `Surface`/`Border` tokens, so the 1- and 2-pane
    // corner scenes are rendered in the other theme too.
    let mut light = AppContext::default();
    light.theme = goble_ui::theme::Theme::light();
    for scene in [Scene::OnePane, Scene::TwoPanes, Scene::Dock] {
        render_scene(&light, &assets, scene, "-light")?;
    }
    Ok(())
}

/// Build, measure and rasterize one scene, with `suffix` appended to its PNG
/// stem (the light-theme pass uses it so it does not overwrite the dark one).
fn render_scene(
    app: &AppContext,
    assets: &raster::PreviewAssets,
    scene: Scene,
    suffix: &str,
) -> anyhow::Result<()> {
    let (mut root, state, _dir) = build_scene(app, scene)?;
    let size = scene.size();
    // One frame lays the app out (the snapshot needs it), the next is the
    // frame measured and painted.
    let _ = paint_frame(app, &mut root, size);
    let commands = paint_frame(app, &mut root, size);
    println!("\n== {:?}{suffix} — {}", scene, scene.label());
    print_geometry(&state, &commands, app, size);
    print_panel_surface_pixels(&state, &commands, app, size);
    print_cursor_probes(app, &mut root, &state, size);

    let stem = scene.file().trim_end_matches(".png");
    let out = Path::new(OUT_DIR).join(format!("{stem}{suffix}.png"));
    raster::render_element_to_png(assets, app, &mut root, size.x, size.y, SCALE, None, &out)?;
    println!("   -> {}", out.display());
    // The card corners, read off the rendered pixels: each corner's spandrel is
    // the panel surface (not a square card pixel) and the focused pane's accent
    // triangle stays inside the rounded corner.
    print_card_corners(&state, app, &out)?;
    save_corner_zooms(scene, &out, &state)?;
    Ok(())
}

/// The pane card's radius, mirrored from `ui::panes::WINDOW_CORNER_RADIUS` (it
/// is crate-private in the app, and an example is its own crate).
const CARD_RADIUS: f32 = 6.0;

/// How many points a corner zoom shows either side of the corner, and how far
/// it magnifies them.
const CORNER_CROP: u32 = 36;
const CORNER_ZOOM: u32 = 10;

/// The RGB at the pixel a point falls in.
fn pixel_at(pixels: &image::RgbaImage, x: f32, y: f32) -> [u8; 3] {
    let px = pixels
        .get_pixel(x.floor().max(0.0) as u32, y.floor().max(0.0) as u32)
        .0;
    [px[0], px[1], px[2]]
}

/// Whether a rendered pixel is the theme colour, within the renderer's
/// anti-aliasing and linear-light blend tolerance.
fn near(pixel: [u8; 3], color: goble_ui::ColorU) -> bool {
    pixel[0].abs_diff(color.r) <= 6
        && pixel[1].abs_diff(color.g) <= 6
        && pixel[2].abs_diff(color.b) <= 6
}

/// Measure the pane cards' four corners in the scene's rendered PNG: every
/// corner spandrel must be the panel's `Surface` (a square corner would leave
/// the card's `Bg` there), the pixels just inside each rounded path must be the
/// card content, and the focused pane's accent triangle must have no pixel
/// outside its rounded top-left corner. Fails the preview if a corner is square.
fn print_card_corners(
    state: &Rc<RefCell<UiState>>,
    app: &AppContext,
    png: &Path,
) -> anyhow::Result<()> {
    // While a sheet is open its scrim covers the frame, so the colours read off
    // the pixels are the scrim's blend, not the card's corner material: the
    // scenes without a sheet are the ones measured (as for the surface pixels).
    {
        let s = state.borrow();
        if s.right_sidebar_open || s.crons_open {
            println!("   card corners: not measured — a secondary sheet's scrim covers the frame");
            return Ok(());
        }
    }
    let pixels = image::open(png)?.to_rgba8();
    let name = png
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("frame.png")
        .trim_end_matches(".png");
    let surface = app.theme.color(ColorToken::Surface);
    let bg = app.theme.color(ColorToken::Bg);
    let accent = app.theme.color(ColorToken::Accent);
    let frames = pane_bounds(state);
    let mut rounded = 0;
    let mut square = Vec::new();
    for (id, card) in &frames {
        let radius = CARD_RADIUS.min(card.width() * 0.5).min(card.height() * 0.5);
        let focused = *id == state.borrow().active_pane_id;
        // The corner spandrels: outside the rounded path, inside the card rect.
        for (corner, x, y) in [
            ("tl", card.min_x(), card.min_y()),
            ("tr", card.max_x() - 1.0, card.min_y()),
            ("bl", card.min_x(), card.max_y() - 1.0),
            ("br", card.max_x() - 1.0, card.max_y() - 1.0),
        ] {
            let at = pixel_at(&pixels, x, y);
            if near(at, surface) {
                rounded += 1;
            } else {
                square.push(format!("pane {id} {corner} {at:?}"));
            }
        }
        // Inside the rounded path at the bottom corners: still the card fill.
        let inside_bl = pixel_at(&pixels, card.min_x() + radius, card.max_y() - radius);
        let inside_br = pixel_at(&pixels, card.max_x() - radius, card.max_y() - radius);
        let inside = near(inside_bl, bg) && near(inside_br, bg);
        // The focus marker: inside the rounded top-left corner, never outside.
        let marker_inside = pixel_at(
            &pixels,
            card.min_x() + radius * 0.5,
            card.min_y() + radius * 0.5,
        );
        let outside_accent = accent_pixels_outside_corner(&pixels, *card, radius, accent);
        println!(
            "   corner zoom {name} pane {id} (card x {:.1}..{:.1} y {:.1}..{:.1}, r={radius:.1}): \
             spandrels {} ({}) | inside bottom corners {} | marker inside {:?} | accent px outside the radius {} | focused {}",
            card.min_x(),
            card.max_x(),
            card.min_y(),
            card.max_y(),
            if square.is_empty() { "surface".to_string() } else { format!("{:?}", square) },
            if square.is_empty() { "rounded" } else { "SQUARE" },
            if inside {
                "Bg".to_string()
            } else {
                format!("{inside_bl:?}/{inside_br:?}")
            },
            marker_inside,
            outside_accent,
            focused,
        );
        if focused {
            anyhow::ensure!(
                near(marker_inside, accent),
                "pane {id}: the focus marker does not paint inside the card's rounded corner"
            );
            anyhow::ensure!(
                outside_accent == 0,
                "pane {id}: {outside_accent} accent pixels sit outside the rounded corner"
            );
        }
        anyhow::ensure!(
            inside,
            "pane {id}: the bottom corners' inside pixels are not the card fill"
        );
        anyhow::ensure!(
            square.is_empty(),
            "pane {id}: square card pixels: {square:?}"
        );
    }
    if frames.is_empty() {
        anyhow::bail!("no pane card was drawn in {}", png.display());
    }
    println!(
        "   card corners ({name}): {rounded} spandrels rounded, {} square",
        square.len()
    );
    Ok(())
}

/// The accent pixels of a card's top-left corner square that lie outside its
/// rounded path.
fn accent_pixels_outside_corner(
    pixels: &image::RgbaImage,
    card: RectF,
    radius: f32,
    accent: goble_ui::ColorU,
) -> usize {
    let cx = card.min_x() + radius;
    let cy = card.min_y() + radius;
    let reach = radius.ceil() as u32 + 4;
    let mut outside = 0;
    for dy in 0..reach {
        for dx in 0..reach {
            let x = card.min_x().floor() as u32 + dx;
            let y = card.min_y().floor() as u32 + dy;
            let px = pixels.get_pixel(x, y).0;
            if !near([px[0], px[1], px[2]], accent) {
                continue;
            }
            let distance = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            if distance > radius {
                outside += 1;
            }
        }
    }
    outside
}

/// Save a magnified crop of each pane card's four corners for the 1-pane and
/// 2-pane scenes, so the rounding (and the clipped triangle corner) can be
/// inspected as an image.
fn save_corner_zooms(scene: Scene, png: &Path, state: &Rc<RefCell<UiState>>) -> anyhow::Result<()> {
    if !matches!(scene, Scene::OnePane | Scene::TwoPanes) {
        return Ok(());
    }
    let pixels = image::open(png)?.to_rgba8();
    let stem = png.file_stem().and_then(|s| s.to_str()).unwrap_or("pane");
    for (id, card) in pane_bounds(state) {
        for (corner, x, y) in [
            ("tl", card.min_x(), card.min_y()),
            ("tr", card.max_x(), card.min_y()),
            ("bl", card.min_x(), card.max_y()),
            ("br", card.max_x(), card.max_y()),
        ] {
            let x0 = (x.floor() as i64 - (CORNER_CROP as i64 / 2))
                .clamp(0, (pixels.width() - CORNER_CROP) as i64) as u32;
            let y0 = (y.floor() as i64 - (CORNER_CROP as i64 / 2))
                .clamp(0, (pixels.height() - CORNER_CROP) as i64) as u32;
            let crop =
                image::imageops::crop_imm(&pixels, x0, y0, CORNER_CROP, CORNER_CROP).to_image();
            let zoom = image::imageops::resize(
                &crop,
                CORNER_CROP * CORNER_ZOOM,
                CORNER_CROP * CORNER_ZOOM,
                image::imageops::FilterType::Nearest,
            );
            let out = Path::new(OUT_DIR).join(format!("{stem}-corner-{corner}-pane{id}.png"));
            zoom.save(&out)?;
            println!("   corner zoom -> {}", out.display());
        }
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
    // The panel surface starts right of the sidebar only while the responsive
    // layout keeps it shown; a collapsed sidebar gives its width back.
    let body_left = if s.responsive.sidebar_shown(s.sidebar_visible) {
        s.sidebar_width
    } else {
        0.0
    };
    // The responsive step this frame is in: the decisions the layout made from
    // the window's measured width, and the surfaces it keeps or hides.
    let responsive = s.responsive;
    let sidebar_drawn = commands.iter().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. }
            if *color == surface
                && rect.min_x() == 0.0
                && rect.height() > window.y / 2.0
                // The collapsed-sidebar frame paints the panel surface from
                // x=0 to the window's right edge too; the sidebar is the fill
                // that stops at the sidebar's own width.
                && rect.max_x() < window.x - 1.0 =>
        {
            Some(*rect)
        }
        _ => None,
    });
    let panel_drawn = commands.iter().any(
        |command| matches!(command, RenderCommand::DrawText { text, .. } if text == "Agent panel"),
    );
    let frames = pane_bounds(state);
    println!(
        "\n   responsive: window {:.0} | sidebar collapsed? {} | secondary hidden? {} | focused only? {}",
        window.x,
        responsive.sidebar_collapsed,
        responsive.secondary_hidden,
        responsive.focused_only,
    );
    println!(
        "   sidebar: {} | agent panel: {} | panes drawn: {} ({} leaves in the tree)",
        match sidebar_drawn {
            Some(rect) => format!("shown x {:.1}..{:.1}", rect.min_x(), rect.max_x()),
            None => "collapsed".to_string(),
        },
        if panel_drawn { "shown" } else { "hidden" },
        frames.len(),
        {
            let mut leaves = Vec::new();
            if let Some(space) = s.spaces.get(s.active_space) {
                space.root.leaves(&mut leaves);
            }
            leaves.len()
        }
    );

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
            "     panel card: {} | outside text column {:?} | inside text column {:?} -> lane matches card? {}",
            color_hex(card_color),
            outside.map(color_hex),
            inside.map(color_hex),
            uniform
        );
        // The surface the panels are rendered on is the sidebar's `Surface`.
        let backdrop = commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == surface
                && *corner_radius == 0.0
                && rect.min_y() > TOPBAR_HEIGHT
                && rect.width() > window.x / 2.0
                && rect.height() > window.y / 2.0 =>
            {
                Some((*rect, *color))
            }
            _ => None,
        });
        if let Some((backdrop_rect, backdrop_color)) = backdrop {
            // The sidebar fill stops at the sidebar's width; the collapsed-
            // sidebar frame's panel fill reaches the window's right edge, so it
            // is not mistaken for the sidebar.
            let sidebar = commands.iter().find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == surface
                        && rect.min_x() == 0.0
                        && rect.height() > window.y / 2.0
                        && rect.max_x() < window.x - 1.0 =>
                {
                    Some(*color)
                }
                _ => None,
            });
            // The surface paints the body's whole rectangle — the 4 px
            // `PANE_AREA_INSET` frame itself included. Its fill must reach the
            // body origin (below the bar block, right of the sidebar) and the
            // window's right/bottom edges: no dark strip on any side.
            let frame_painted = (backdrop_rect.min_x() - body_left).abs() < 0.01
                && (backdrop_rect.min_y() - body_top).abs() < 0.01
                && (backdrop_rect.max_x() - window.x).abs() < 0.01
                && (backdrop_rect.max_y() - window.y).abs() < 0.01;
            println!(
                "     panel surface backdrop: {} (x {:.1}..{:.1} y {:.1}..{:.1}) | sidebar: {} -> same? {} | inset frame painted? {}",
                color_hex(backdrop_color),
                backdrop_rect.min_x(),
                backdrop_rect.max_x(),
                backdrop_rect.min_y(),
                backdrop_rect.max_y(),
                sidebar.map(color_hex).unwrap_or_else(|| "collapsed".to_string()),
                sidebar.is_none() || sidebar == Some(backdrop_color),
                frame_painted,
            );
        }

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
                RenderCommand::DrawIcon {
                    origin, name, size, ..
                } if name == "x-close" => Some((origin.x + size / 2.0, origin.y + size / 2.0)),
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

    // The TAB-SLOT area: the bounding rectangle of the tabs' slots. The slots
    // are inset from the body/chrome on all four sides; the panel surface behind
    // them is not — it paints the body's whole rectangle, the inset frame
    // included (see the edge pixel probes).
    if let (Some(first), Some(area)) = (slots.first(), tab_area(&slots)) {
        let _ = first;
        println!(
            "\n   tab-slot area: x {:.1}..{:.1} y {:.1}..{:.1} | slots inset from body L {:.1} T {:.1} R {:.1} B {:.1} -> 4/4/4/4? {}",
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
    // drag is in flight (the ResizeChrome scene grabs both). A rule elsewhere
    // is another surface's own chrome (the agent panel's header seam), not a
    // resize divider.
    let frames = pane_bounds(state);
    let mut boundaries: Vec<f32> = frames
        .windows(2)
        .map(|pair| (pair[0].1.max_x() + pair[1].1.min_x()) / 2.0)
        .collect();
    if s.responsive.sidebar_shown(s.sidebar_visible) {
        boundaries.push(s.sidebar_width);
    }
    let rules = thin_rules(commands);
    let (at_boundary, elsewhere): (Vec<RectF>, Vec<RectF>) = rules.into_iter().partition(|rule| {
        boundaries.iter().any(|boundary| {
            (rule.min_x() - boundary).abs() < 2.0
                || (rule.max_x() - boundary).abs() < 2.0
                || (((rule.min_x() + rule.max_x()) / 2.0) - boundary).abs() < 2.0
        })
    });
    println!(
        "\n   thin divider rules at resizable boundaries: {} (must be 0){}",
        at_boundary.len(),
        if at_boundary.is_empty() {
            String::new()
        } else {
            format!(" -> {at_boundary:?}")
        }
    );
    println!(
        "   thin rules elsewhere in the frame (other chrome, not resize): {}{}",
        elsewhere.len(),
        if elsewhere.is_empty() {
            String::new()
        } else {
            format!(" -> {elsewhere:?}")
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
    println!("   pane card / outline strokes (frames, not resize rules): {frames}");
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
        "   Surface fills: {surfaces} | Bg fills: {} (panel cards are Bg for every pane kind)",
        commands
            .iter()
            .filter(|command| matches!(
                command,
                RenderCommand::FillRect { color, .. } if *color == bg
            ))
            .count()
    );
}

/// The panel surface's pixels, measured the way the frame leaves them: the last
/// fill covering the point. It prints the sidebar's own pixel (the reference),
/// the four edges of the pane area across the whole 4 px `PANE_AREA_INSET` frame,
/// the 3 px gutter between two neighbouring tab slots' outlines, and the pane
/// cards' pixels — so a leftover dark strip on any edge shows up as a different
/// `#rrggbb` and a false "all sidebar grey?".
fn print_panel_surface_pixels(
    state: &Rc<RefCell<UiState>>,
    commands: &[RenderCommand],
    app: &AppContext,
    window: Vector2F,
) {
    let s = state.borrow();
    let surface = app.theme.color(ColorToken::Surface);
    let bg = app.theme.color(ColorToken::Bg);
    let body_top = TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND;
    let body_left = if s.responsive.sidebar_shown(s.sidebar_visible) {
        s.sidebar_width
    } else {
        0.0
    };
    let mid_x = (body_left + window.x) / 2.0;
    let mid_y = (body_top + window.y) / 2.0;
    let pixel = |point: Vector2F| {
        topmost_fill(commands, point)
            .map(color_hex)
            .unwrap_or_else(|| "none".to_string())
    };

    // While a sheet is open its scrim covers the frame (the agent panel scenes),
    // so the pane-area pixels are not the panel surface's own: say so instead of
    // measuring the scrim.
    if s.right_sidebar_open || s.crons_open {
        println!(
            "\n   panel surface pixels: not measured — a secondary sheet's scrim covers the frame"
        );
        return;
    }

    // The sidebar is the reference grey the panel surface must match. A
    // collapsed sidebar (the responsive scenes) paints no sidebar, so the
    // reference is the theme's own `Surface` colour.
    let sidebar_shown = s.responsive.sidebar_shown(s.sidebar_visible);
    let sidebar_probe = topmost_fill(commands, vec2f(s.sidebar_width / 2.0, mid_y));
    let (sidebar_label, mut all_surface) = if sidebar_shown {
        (
            pixel(vec2f(s.sidebar_width / 2.0, mid_y)),
            sidebar_probe == Some(surface),
        )
    } else {
        ("collapsed (theme Surface reference)".to_string(), true)
    };

    // The four edges of the pane area, right across the inset frame: 1.5 px
    // inside each side, in the middle of the area.
    let mut edges = Vec::new();
    for (label, point) in [
        ("top", vec2f(mid_x, body_top + 1.5)),
        ("bottom", vec2f(mid_x, window.y - 1.5)),
        ("left", vec2f(body_left + 1.5, mid_y)),
        ("right", vec2f(window.x - 1.5, mid_y)),
    ] {
        let color = topmost_fill(commands, point);
        all_surface &= color == Some(surface);
        edges.push(format!("{label} {}", pixel(point)));
    }
    println!(
        "\n   panel surface pixels: sidebar {sidebar_label} | {} | frame + edges all sidebar grey? {}",
        edges.join(" | "),
        all_surface
    );

    // The gutter between two neighbouring tab slots: each tab outline is inset
    // 1.5 pt from the shared side, so the outlines are 3 pt apart and those
    // pixels are the panel surface. Sample the three pixel columns across that
    // 3 pt gap, at three heights of the split.
    let panes = pane_bounds(state);
    let mut gutter_surface = true;
    if let (Some(first), Some(second)) = (panes.first(), panes.get(1)) {
        let boundary = (first.1.max_x() + second.1.min_x()) / 2.0;
        let mut samples = Vec::new();
        for y in [
            (first.1.min_y() + second.1.max_y()) / 2.0,
            (first.1.min_y() + TOPBAR_HEIGHT + 20.0),
            second.1.max_y() - 20.0,
        ] {
            for dx in [-1.0, 0.0, 1.0] {
                let point = vec2f(boundary + dx, y);
                gutter_surface &= topmost_fill(commands, point) == Some(surface);
                samples.push(pixel(point));
            }
        }
        println!(
            "   tab gutter pixels (the 3 pt outline gap between two slots): {} -> panel surface? {}",
            samples.join(" "),
            gutter_surface
        );
    } else {
        println!("   tab gutter pixels: single pane, no neighbouring tab slot");
    }

    // The cards, and only the cards, are the darker `Bg`: the fill at each
    // card's own rectangle. Also count the `Bg` fills drawn over the panel
    // surface that are not inside a card — a stray dark strip anywhere in the
    // pane area — which must be zero.
    let mut cards = Vec::new();
    let mut only_cards_bg = true;
    for (id, frame) in &panes {
        let card_fill = commands.iter().find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } if rect == frame => Some(*color),
            _ => None,
        });
        only_cards_bg &= card_fill == Some(bg);
        cards.push(format!(
            "pane {id} {}",
            color_hex(card_fill.unwrap_or(surface))
        ));
    }
    let surface_index = commands.iter().position(|command| match command {
        RenderCommand::FillRect { rect, color, .. } => {
            *color == surface && rect.width() > window.x / 2.0
        }
        _ => false,
    });
    let stray: Vec<RectF> = surface_index
        .map(|index| {
            commands
                .iter()
                .skip(index + 1)
                .filter_map(|command| match command {
                    RenderCommand::FillRect { rect, color, .. } if *color == bg => {
                        let inside_card = panes.iter().any(|(_, frame)| {
                            rect.min_x() >= frame.min_x() - 0.5
                                && rect.max_x() <= frame.max_x() + 0.5
                                && rect.min_y() >= frame.min_y() - 0.5
                                && rect.max_y() <= frame.max_y() + 0.5
                        });
                        // The chrome above the pane area (the tab fillets) is
                        // not a panel-surface pixel.
                        let above_surface = rect.max_y() <= body_top + 0.5;
                        (!inside_card && !above_surface).then_some(*rect)
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    println!(
        "   pane card pixels: {} | cards (only) are Bg? {} | surface {} vs card {} | Bg fills over the surface outside cards: {} (must be 0)",
        cards.join(" | "),
        only_cards_bg && stray.is_empty(),
        color_hex(surface),
        color_hex(bg),
        stray.len(),
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
    let (sidebar_width, sidebar_shown, dragging) = {
        let s = state.borrow();
        (
            s.sidebar_width,
            s.responsive.sidebar_shown(s.sidebar_visible),
            s.dragging_pane_id.is_some() || s.sidebar_dragging,
        )
    };
    let body_top = TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND;
    let mid_y = (body_top + window.y) / 2.0;

    // The sidebar's edge, in the middle of its invisible grab band — it only
    // has one while the responsive layout keeps the sidebar shown.
    let sidebar_edge = vec2f(sidebar_width, mid_y);
    println!(
        "   cursor at the sidebar edge {:.1}: {:?} (want {})",
        sidebar_width,
        probe_cursor(app, root, sidebar_edge),
        if sidebar_shown { "EwResize" } else { "Default" }
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
