//! Headless raster previews of the per-pane content lane.
//!
//! The live native window cannot be captured in this environment, so this
//! example builds the real app tree (the same `RootView` the window draws) and
//! rasterizes it to PNGs through the shared preview rasterizer. It is a visual
//! verification tool for the per-pane lane fix, not a product surface:
//!
//! - each chat pane centres its own content column inside its own pane width
//!   (the app's width axis: 64% clamped 680-920, inactive when the pane is
//!   narrow) and the pane's header spans the full pane;
//! - panes with different widths resolve different content columns;
//! - a committed per-pane preference resizes only the pane it belongs to.
//!
//! Scenes: 1 pane (1600), 2 panes of different widths (2600, ratio 0.6),
//! the same split with the left pane's lane dragged/committed pref, 3 panes
//! (3000), and 2 narrow panes (1600, both inactive/full width).
//!
//! Usage: cargo run -p goble-app --example pane_lane_preview
//! Output: /tmp/goble-work/out/pane-lane-<scene>.png, with the per-pane
//! geometry (pane rect, resolved content column, gutters) printed to stdout.

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_app::state::{PaneSession, UiState};
use goble_app::ui::{Pane, PaneKind, Space, SplitDir};
use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{lane_column, resolve_content_width, AppContext, Element, LayoutContext};
use goble_ui::geometry::Vector2F;
use goble_ui::render::Renderer;
use goble_ui::{vec2f, ChatFragment, ChatMessage, ChatRole};

const OUT_DIR: &str = "/tmp/goble-work/out";
/// One device pixel per logical point: the scenes are 1600-3000 wide, and the
/// geometry under test is read from the element tree, not the raster.
const SCALE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scene {
    OnePane,
    TwoPanes,
    TwoPanesPerPaneDrag,
    ThreePanes,
    TwoNarrow,
}

impl Scene {
    fn all() -> [Self; 5] {
        [
            Self::OnePane,
            Self::TwoPanes,
            Self::TwoPanesPerPaneDrag,
            Self::ThreePanes,
            Self::TwoNarrow,
        ]
    }

    fn size(self) -> Vector2F {
        match self {
            Self::OnePane | Self::TwoNarrow => vec2f(1600.0, 900.0),
            Self::TwoPanes | Self::TwoPanesPerPaneDrag => vec2f(2600.0, 900.0),
            Self::ThreePanes => vec2f(3000.0, 900.0),
        }
    }

    fn file(self) -> &'static str {
        match self {
            Self::OnePane => "pane-lane-1pane.png",
            Self::TwoPanes => "pane-lane-2pane.png",
            Self::TwoPanesPerPaneDrag => "pane-lane-2pane-per-pane-drag.png",
            Self::ThreePanes => "pane-lane-3pane.png",
            Self::TwoNarrow => "pane-lane-2pane-narrow.png",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::OnePane => "1 pane, 1600x900: one lane centred in the pane",
            Self::TwoPanes => "2 panes, 2600x900 ratio 0.6: pane columns 1372 / 912",
            Self::TwoPanesPerPaneDrag => {
                "2 panes, 2600x900: LEFT pane's lane dragged +100, RIGHT untouched"
            }
            Self::ThreePanes => "3 panes, 3000x900: each pane meters its own content",
            Self::TwoNarrow => "2 narrow panes, 1600x900: both inactive / full width",
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

/// Two chat panes in one space (ids 7 and 8), active on 7.
fn install_two_panes(state: &mut UiState, ratio: f32) {
    state.spaces = vec![Space::new(
        "Lane preview",
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

/// Three chat panes in equal slots (outer 2/3, inner 1/2): ids 7, 8, 11.
fn install_three_panes(state: &mut UiState) {
    state.spaces = vec![Space::new(
        "Lane preview",
        Pane::Split {
            id: 9,
            dir: SplitDir::Horizontal,
            ratio: 2.0 / 3.0,
            first: Box::new(Pane::Split {
                id: 10,
                dir: SplitDir::Horizontal,
                ratio: 0.5,
                first: chat_leaf(7),
                second: chat_leaf(8),
            }),
            second: chat_leaf(11),
        },
    )];
    state.active_space = 0;
    state.active_pane_id = 7;
    for id in [7u64, 8, 11] {
        state.pane_sessions.insert(id, chat_session());
    }
}

/// The real app tree over a fresh in-memory store, in the scene's pane layout
/// and the normal (non-first-run) chrome.
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
        // The first-run overlays would cover the workspace; the preview is of
        // the normal surface.
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.show_onboarding_tip = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
        // One short conversation in every pane, so the transcript (and with it
        // the centred content column) is what each pane shows.
        s.chat_messages = vec![
            ChatMessage::new(
                ChatRole::User,
                vec![ChatFragment::text("Centre this pane's content.")],
            ),
            ChatMessage::new(
                ChatRole::Assistant,
                vec![ChatFragment::text(
                    "Each pane meters its own column now: the header spans the pane, the content is centred inside it.",
                )],
            ),
        ];
        match scene {
            Scene::OnePane => {}
            Scene::TwoPanes | Scene::TwoPanesPerPaneDrag => install_two_panes(&mut s, 0.6),
            Scene::TwoNarrow => install_two_panes(&mut s, 0.5),
            Scene::ThreePanes => install_three_panes(&mut s),
        }
    }
    // The probe in `main` records the pane bounds; the scene needs no further
    // preparation here.
    Ok((root, state, dir))
}

/// One frame of the app: layout rebuilds the tree from app state, then paint
/// records the panes' bounds — the order the window runs every frame.
fn paint_frame(app: &AppContext, root: &mut Box<dyn Element>, size: Vector2F) {
    let _ = root.layout(
        goble_ui::elements::SizeConstraint::loose(size),
        &mut LayoutContext::default(),
        app,
    );
    let mut ctx = goble_ui::elements::PaintContext::new(Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut ctx, app);
}

/// The panes' recorded bounds, in pane-id order.
fn pane_bounds(state: &Rc<RefCell<UiState>>) -> Vec<(u64, goble_ui::geometry::RectF)> {
    let s = state.borrow();
    let bounds = s.pane_bounds.borrow().clone();
    let mut entries: Vec<(u64, goble_ui::geometry::RectF)> = bounds.into_iter().collect();
    entries.sort_by_key(|(id, _)| *id);
    entries
}

fn print_geometry(state: &Rc<RefCell<UiState>>, scene: Scene) {
    println!("\n== {:?} — {}", scene, scene.label());
    let s = state.borrow();
    for (id, bounds) in pane_bounds(state) {
        let pref = s.content_lane_prefs.get(&id).copied();
        let (gutter, width) = lane_column(bounds.width(), pref);
        println!(
            "   pane {id}: pane x {:.1}..{:.1} (w {:.1}) | pref {} | content x {:.1}..{:.1} (w {:.1}) | gutter {:.2}/side",
            bounds.min_x(),
            bounds.max_x(),
            bounds.width(),
            pref.map(|p| format!("{p:.2}")).unwrap_or_else(|| "-".to_string()),
            bounds.min_x() + gutter,
            bounds.min_x() + gutter + width,
            width,
            gutter,
        );
    }
    println!(
        "   header: each pane's topbar spans the whole pane x-range above; the content column is the inner range"
    );
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
        // The probe lays the frame out once and records each pane's bounds (the
        // per-pane test does the same before dispatching a drag; here it is how
        // the per-pane-drag scene learns the left pane's column width).
        paint_frame(&app, &mut root, size);
        if scene == Scene::TwoPanesPerPaneDrag {
            let left_width = pane_bounds(&state)
                .iter()
                .find(|(id, _)| *id == 7)
                .map(|(_, bounds)| bounds.width())
                .unwrap_or(0.0);
            let (_, ratio) = lane_column(left_width, None);
            // What a 50 px outward drag of the left pane's edge commits: the
            // ratio column + 2*50, resolved against the pane's own width.
            let dragged = resolve_content_width(left_width, Some(ratio + 100.0));
            state.borrow_mut().content_lane_prefs.insert(7, dragged);
        }
        let out = scene_png(scene);
        raster::render_element_to_png(
            &assets, &app, &mut root, size.x, size.y, SCALE, None, &out,
        )?;
        print_geometry(&state, scene);
        println!("   -> {}", out.display());
    }
    Ok(())
}
