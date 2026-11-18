//! Headless raster previews of the pane chrome: the full-width topbar band and
//! the work dock band, with their inner content bounded to the pane's content
//! lane.
//!
//! The live native window cannot be captured in this environment, so this
//! example builds the real app tree (the same `RootView` the window draws) with
//! a pane that has work in flight (so the dock renders) and rasterizes it to
//! PNGs through the shared preview rasterizer. It is a visual verification tool
//! for the pane-chrome fix, not a product surface:
//!
//! - the pane's topbar band is square and flush with the pane's slot (it covers
//!   the pane frame's inset gutter), while the pane card below keeps its rounded
//!   inset frame;
//! - the work dock's band spans the pane as a square band, and its announcement
//!   cards are bounded to the content lane the transcript is centred in;
//! - a narrow pane (no active lane) keeps the same bands with full-width inner
//!   content.
//!
//! Scenes: 1 pane (1600), 2 panes (2600, ratio 0.6), 1 narrow pane (1024).
//!
//! Usage: cargo run -p goble-app --example pane_chrome_preview
//! Output: /tmp/goble-work/out/pane-chrome-<scene>.png, with the per-pane
//! geometry (pane rect, header band rect, dock band rect, card rects, resolved
//! lane column) printed to stdout.

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_app::state::{PaneSession, UiState};
use goble_app::ui::{Pane, PaneKind, Space, SplitDir};
use goble_core::harness::ToolCallStatus;
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{
    lane_column, AppContext, Element, LayoutContext, PaintContext, SizeConstraint,
};
use goble_ui::geometry::{RectF, Vector2F};
use goble_ui::render::{RenderCommand, Renderer};
use goble_ui::{vec2f, ChatFragment, ChatMessage, ChatRole, ToolCall};

const OUT_DIR: &str = "/tmp/goble-work/out";
const SCALE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scene {
    OnePane,
    TwoPanes,
    Narrow,
}

impl Scene {
    fn all() -> [Self; 3] {
        [Self::OnePane, Self::TwoPanes, Self::Narrow]
    }

    fn size(self) -> Vector2F {
        match self {
            Self::OnePane => vec2f(1600.0, 900.0),
            Self::TwoPanes => vec2f(2600.0, 900.0),
            Self::Narrow => vec2f(1024.0, 768.0),
        }
    }

    fn file(self) -> &'static str {
        match self {
            Self::OnePane => "pane-chrome-1pane.png",
            Self::TwoPanes => "pane-chrome-2pane.png",
            Self::Narrow => "pane-chrome-narrow.png",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::OnePane => "1 pane, 1600x900: full-width square chrome bands, lane-bounded content",
            Self::TwoPanes => "2 panes, 2600x900 ratio 0.6: each pane's chrome spans its own slot",
            Self::Narrow => "1 narrow pane, 1024x768: lane inactive, chrome bands still full width",
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
                vec![ChatFragment::text("Where does the pane chrome end?")],
            ),
            ChatMessage::new(
                ChatRole::Assistant,
                vec![ChatFragment::text(
                    "The chrome bands span the whole pane; the text inside them stops at the conversation column.",
                )],
            ),
        ];
        match scene {
            Scene::OnePane | Scene::Narrow => {}
            Scene::TwoPanes => install_two_panes(&mut s, 0.6),
        }
        // A pane snapshot is what makes the pane draw its own chat view (and
        // with it the work dock); every scene gives its panes one.
        let pane_id = s.active_pane_id;
        if scene == Scene::OnePane {
            // The owner's scene: agent mode with the width dragged narrower —
            // the header stays full width while the darker column narrows.
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

/// The square `Surface` bands the chrome paints: the header band (bar plus the
/// pane frame's inset) and the dock band below it.
fn chrome_bands(commands: &[RenderCommand], app: &AppContext) -> Vec<RectF> {
    let surface = app.theme.color(goble_ui::theme::ColorToken::Surface);
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == surface && *corner_radius == 0.0 && rect.width() > rect.height() * 4.0 => {
                Some(*rect)
            }
            _ => None,
        })
        .collect()
}

fn pane_bounds(state: &Rc<RefCell<UiState>>) -> Vec<(u64, RectF)> {
    let s = state.borrow();
    let bounds = s.pane_bounds.borrow().clone();
    let mut entries: Vec<(u64, RectF)> = bounds.into_iter().collect();
    entries.sort_by_key(|(id, _)| *id);
    entries
}

/// A `#rrggbb` form of a theme colour, for the printed geometry.
fn card_color_hex(color: goble_ui::ColorU) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

/// Relative luminance, enough to state that the lane column is darker.
fn luminance(color: goble_ui::ColorU) -> f32 {
    0.2126 * color.r as f32 + 0.7152 * color.g as f32 + 0.0722 * color.b as f32
}

fn print_geometry(state: &Rc<RefCell<UiState>>, commands: &[RenderCommand], app: &AppContext) {
    let s = state.borrow();
    for (id, pane) in pane_bounds(state) {
        let pref = s.content_lane_prefs.get(&id).copied();
        let (gutter, lane) = lane_column(pane.width(), pref);
        println!(
            "\n   pane {id}: x {:.1}..{:.1} y {:.1}..{:.1} (w {:.1}); lane x {:.1}..{:.1} (w {:.1}, gutter {:.1})",
            pane.min_x(),
            pane.max_x(),
            pane.min_y(),
            pane.max_y(),
            pane.width(),
            pane.min_x() + gutter,
            pane.min_x() + gutter + lane,
            lane,
            gutter,
        );
        // The bands drawn over this pane's x range and starting under the topbar.
        let over: Vec<RectF> = chrome_bands(commands, app)
            .into_iter()
            .filter(|band| band.min_x() <= pane.min_x() + 12.0 && band.max_x() >= pane.max_x() - 12.0)
            .collect();
        for band in over {
            println!(
                "     band: x {:.1}..{:.1} (w {:.1}) y {:.1}..{:.1} (h {:.1}) r=0",
                band.min_x(),
                band.max_x(),
                band.width(),
                band.min_y(),
                band.max_y(),
                band.height()
            );
        }
        // The lane column's fill vs the pane card's surface: the column must
        // be the darker token, and the header controls must stay on the pane's
        // edges (not on the column's).
        let surface = app.theme.color(goble_ui::theme::ColorToken::Surface);
        let card = commands.iter().find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == surface
                    && (rect.width() - pane.width()).abs() < 0.5
                    && rect.height() > 300.0 =>
            {
                Some(*rect)
            }
            _ => None,
        });
        let column = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if rect.width() < pane.width() - 1.0
                        && rect.height() > 300.0
                        && rect.min_x() >= pane.min_x()
                        && rect.max_x() <= pane.max_x() =>
                {
                    Some((*rect, *color))
                }
                _ => None,
            })
            .max_by(|a, b| (a.0.width() * a.0.height()).total_cmp(&(b.0.width() * b.0.height())));
        if let (Some(card), Some((column_rect, column_color))) = (card, column) {
            println!(
                "     pane surface {:?} (x {:.1}..{:.1}) | lane column {:?} (x {:.1}..{:.1}, w {:.1}) -> darker? {}",
                card_color_hex(surface),
                card.min_x(),
                card.max_x(),
                card_color_hex(column_color),
                column_rect.min_x(),
                column_rect.max_x(),
                column_rect.width(),
                luminance(column_color) < luminance(surface)
            );
        }
        // Header row extents: the esc hint sits on the pane's leading edge, the
        // close X on its trailing edge, even with a narrower lane.
        let close = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, size, .. } if name == "x-close" => {
                    Some((origin.x + size / 2.0, origin.y + size / 2.0))
                }
                _ => None,
            })
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let esc = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == "esc" => Some(*origin),
                _ => None,
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
}

fn scene_png(scene: Scene) -> PathBuf {
    Path::new(OUT_DIR).join(scene.file())
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
        print_geometry(&state, &commands, &app);
        let out = scene_png(scene);
        raster::render_element_to_png(&assets, &app, &mut root, size.x, size.y, SCALE, None, &out)?;
        println!("   -> {}", out.display());
    }
    Ok(())
}
