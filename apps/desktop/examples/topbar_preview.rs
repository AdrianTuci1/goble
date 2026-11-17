//! Headless raster previews of the Chrome-style fluid topbar tab strip.
//!
//! The live native window cannot be captured in this environment, so this
//! example builds the real app tree (the same `RootView` the window draws) and
//! rasterizes the top of the window to PNGs through the shared preview
//! rasterizer. It is a visual-verification tool, not a product surface.
//!
//! The strip's states are injected the way the app injects them: the pointer
//! position and the held-button flag are handed to the paint pass, which is
//! where the tab reads hover and press.
//!
//! Usage: cargo run -p goble-app --example topbar_preview
//! Output: /tmp/goble-work/out/topbar-<theme>-<scene>.png, plus a x4
//! `-band-zoom` crop of the tab band and the strip of `Bg` under it, and an
//! x8 `-fillet-zoom` crop of the selected tab's bottom junction, so the
//! inverted (concave) corners, the band and the floating pills can be read
//! closely.

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint};
use goble_ui::geometry::{vec2f, RectF, Vector2F};
use goble_ui::render::{RenderCommand, Renderer};
use goble_ui::theme::{TabColor, Theme};

use goble_app::root_view::RootView;
use goble_app::ui::shell::TOPBAR_HEIGHT;
use goble_app::ui::{Pane, PaneKind, Space};

/// The window the app is laid out in. Wide enough for four comfortable tabs;
/// the pane area below the bar is what the selected tab's fill merges into.
const WIDTH: f32 = 1024.0;
const HEIGHT: f32 = 420.0;
const SCALE: f32 = 2.0;
const OUT_DIR: &str = "/tmp/goble-work/out";

/// The tab geometry the example reads off the frame to aim the pointer.
const TAB_EDGE_INSET: f32 = 3.0;
const TAB_CLOSE_BUTTON: f32 = 16.0;
const TAB_LABEL_PAD: f32 = 7.0;

#[derive(Clone, Copy, PartialEq)]
enum Scene {
    /// Four tabs, the second selected: the merge at rest.
    Rest,
    /// The pointer over the third (floating) tab.
    Hover,
    /// The pointer over the selected tab's close control.
    CloseHover,
    /// A held button over the third tab: the pressed fill.
    Pressed,
    /// A coloured selected tab and a coloured floating one.
    Coloured,
    /// Nine tabs: the shared, narrow strip.
    Many,
    /// The selected tab in its inline rename field.
    Rename,
    /// The selected pane has changed files: the Open-changes topbar control.
    Diff,
}

impl Scene {
    fn name(self) -> &'static str {
        match self {
            Scene::Rest => "rest",
            Scene::Hover => "hover",
            Scene::CloseHover => "close-hover",
            Scene::Pressed => "pressed",
            Scene::Coloured => "coloured",
            Scene::Many => "many",
            Scene::Rename => "rename",
            Scene::Diff => "diff",
        }
    }
}

fn tab_names(scene: Scene) -> Vec<String> {
    if scene == Scene::Many {
        return (0..9).map(|index| format!("Workspace {index}")).collect();
    }
    [
        "Topbar preview",
        "Composer polish",
        "Sidebar review",
        "Backlog triage",
    ]
    .iter()
    .map(|name| name.to_string())
    .collect()
}

/// A throwaway git repository whose one tracked file is modified (+1/-1), so
/// the Open-changes control has real data to draw. `None` when git is not
/// available: the scene then simply draws without the control.
fn dirty_repository() -> Option<tempfile::TempDir> {
    let dir = tempfile::tempdir().ok()?;
    let run = |args: &[&str]| -> bool {
        std::process::Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    };
    if !run(&["init", "-q"]) {
        return None;
    }
    run(&["config", "user.email", "preview@example.com"]);
    run(&["config", "user.name", "Preview"]);
    std::fs::write(dir.path().join("tracked.txt"), "alpha\nbeta\n").ok()?;
    run(&["add", "."]);
    if !run(&["commit", "-q", "-m", "initial"]) {
        return None;
    }
    std::fs::write(dir.path().join("tracked.txt"), "alpha\ndelta\n").ok()?;
    Some(dir)
}

/// One frame of the app: layout rebuilds the tree from app state, then paint
/// reads the pointer — the order the window itself runs every frame.
fn paint_frame(
    app: &AppContext,
    root: &mut Box<dyn Element>,
    cursor: Option<Vector2F>,
    pressed: bool,
) -> Vec<RenderCommand> {
    let _ = root.layout(
        SizeConstraint::loose(vec2f(WIDTH, HEIGHT)),
        &mut LayoutContext::default(),
        app,
    );
    app.pointer_down.set(pressed);
    let mut ctx = PaintContext::new(Renderer::new());
    if let Some(position) = cursor {
        ctx.cursor_inside = true;
        ctx.cursor_position = position;
    }
    root.paint(vec2f(0.0, 0.0), &mut ctx, app);
    app.pointer_down.set(false);
    ctx.renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// The selected tab's fill in a frame: the one `Surface` body that starts
/// inside the bar and runs past its bottom edge into the content.
fn selected_body(commands: &[RenderCommand], app: &AppContext) -> Option<RectF> {
    let surface = app.theme.color(goble_ui::theme::ColorToken::Surface);
    commands.iter().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. }
            if *color == surface
                && rect.min_y() > 0.0
                && rect.min_y() < TOPBAR_HEIGHT
                && rect.max_y() > TOPBAR_HEIGHT =>
        {
            Some(*rect)
        }
        _ => None,
    })
}

/// The commands of one scene, drawn over a fresh app state so hover and press
/// are read from this frame alone.
fn scene_commands(app: &AppContext, scene: Scene) -> anyhow::Result<(Vec<RenderCommand>, RectF)> {
    let dir = tempfile::tempdir()?;
    let desktop = Arc::new(DesktopState::new(
        Store::open_in_memory()?,
        ThreadStore::new(dir.path())?,
    ));
    let view = RootView::new(app, &desktop, None);
    let state = view.state_rc();
    let mut root: Box<dyn Element> = Box::new(view);

    let names = tab_names(scene);
    {
        let mut s = state.borrow_mut();
        // The first-run overlays would cover the strip; the preview is of the
        // normal surface.
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.show_onboarding_tip = false;
        s.right_sidebar_open = false;
        s.spaces[0].name = names[0].clone();
        s.spaces[0].named = true;
        for name in &names[1..] {
            let id = s.next_pane_id;
            s.next_pane_id += 1;
            s.spaces.push(Space::new(
                name.clone(),
                Pane::Leaf {
                    id,
                    kind: PaneKind::Terminal,
                },
            ));
        }
        // A pane outside any repository draws no Open-changes control: the
        // plain scenes show the bar as it looks on a clean working tree.
        let active = s.active_pane_id;
        s.set_pane_path(active, dir.path().display().to_string());
        // The second tab is the selected one throughout: Chrome's selected tab
        // merges into the content with a floating neighbour on each side.
        s.active_space = 1;
    }

    // Scene state that changes what the strip paints (never its own geometry),
    // set before the probe so the probe sees the same trailing controls the
    // final frame will.
    let repository = (scene == Scene::Diff).then(dirty_repository).flatten();
    // The Open-changes control changes the trailing cluster's width, so it has
    // to exist before the probe measures the strip. The colour/rename states
    // only change paint and are set after the probe, so the selected tab's
    // plain surface is what the probe's body finder sees.
    if scene == Scene::Diff {
        if let Some(repository) = repository.as_ref() {
            let mut s = state.borrow_mut();
            let pane = s.spaces[1].root.first_leaf_id();
            s.active_pane_id = pane;
            s.set_pane_path(pane, repository.path().display().to_string());
        }
    }

    // Lay the strip out once to learn its slot geometry, so the pointer can be
    // aimed at a tab (or at the selected tab's close control) in window points.
    let probe = paint_frame(app, &mut root, None, false);
    let body = selected_body(&probe, app)
        .ok_or_else(|| anyhow::anyhow!("the selected tab's fill is not painted"))?;
    let slot = body.width() + TAB_EDGE_INSET * 2.0;
    // The selected tab is the second one, so the strip starts one slot before
    // its fill (which is itself inset by the pill's edge gap).
    let slot_x = body.min_x() - TAB_EDGE_INSET - slot;

    // Paint-only scene state, set once the geometry is known.
    {
        let mut s = state.borrow_mut();
        match scene {
            Scene::Coloured => {
                s.spaces[1].color = Some(TabColor::Red);
                s.spaces[2].color = Some(TabColor::Blue);
            }
            Scene::Rename => s.space_rename_editing = true,
            _ => {}
        }
    }

    let middle = TOPBAR_HEIGHT / 2.0;
    let close_center = slot_x + slot * 2.0 - TAB_EDGE_INSET - TAB_LABEL_PAD - TAB_CLOSE_BUTTON
        + TAB_CLOSE_BUTTON / 2.0;
    let (cursor, pressed) = match scene {
        // Third tab: the second floating neighbour of the selected one.
        Scene::Hover => (Some(vec2f(slot_x + slot * 2.5, middle)), false),
        Scene::CloseHover => (Some(vec2f(close_center, middle)), false),
        Scene::Pressed => (Some(vec2f(slot_x + slot * 2.5, middle)), true),
        _ => (None, false),
    };

    Ok((paint_frame(app, &mut root, cursor, pressed), body))
}

/// The logical band a zoom crop shows around the selected tab: enough of the
/// strip to include a floating neighbour on each side, and enough of the
/// content below to see the tab cross the [`TOPBAR_BOTTOM_BAND`] into the
/// surface.
fn band_crop(body: RectF) -> (f32, f32, f32) {
    let left = (body.min_x() - 150.0).max(0.0);
    let right = (body.max_x() + 330.0).min(WIDTH);
    let height = TOPBAR_HEIGHT + goble_app::ui::shell::TOPBAR_BOTTOM_BAND + 56.0;
    (left, right - left, height)
}

/// Crop a logical rectangle out of a rendered frame, scale it up `zoom` times on
/// top of the raster pass's x2 and write it out.
fn write_zoom(
    full: &std::path::Path,
    out: &std::path::Path,
    (left, width, height): (f32, f32, f32),
    zoom: u32,
) -> anyhow::Result<()> {
    let frame = image::open(full)?.to_rgba8();
    let cropped = image::imageops::crop_imm(
        &frame,
        (left * SCALE) as u32,
        0,
        (width * SCALE) as u32,
        (height * SCALE) as u32,
    )
    .to_image();
    let zoomed = image::imageops::resize(
        &cropped,
        cropped.width() * zoom,
        cropped.height() * zoom,
        image::imageops::FilterType::Lanczos3,
    );
    zoomed.save(out)?;
    println!(
        "wrote {} ({}x{})",
        out.display(),
        zoomed.width(),
        zoomed.height()
    );
    Ok(())
}

/// The selected tab's bottom junction: both inverted corners, the band under
/// them and the first rows of the content, at a high zoom.
fn fillet_crop(body: RectF) -> (f32, f32, f32) {
    let left = (body.min_x() - 48.0).max(0.0);
    let right = (body.max_x() + 48.0).min(WIDTH);
    let height = TOPBAR_HEIGHT + goble_app::ui::shell::TOPBAR_BOTTOM_BAND + 20.0;
    (left, right - left, height)
}

fn main() -> anyhow::Result<()> {
    let assets =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/goble-ui/assets");
    let assets = raster::PreviewAssets::load(&assets)?;
    std::fs::create_dir_all(OUT_DIR)?;

    let scenes = [
        Scene::Rest,
        Scene::Hover,
        Scene::CloseHover,
        Scene::Pressed,
        Scene::Coloured,
        Scene::Many,
        Scene::Rename,
        Scene::Diff,
    ];
    for scene in scenes {
        for (theme_name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            let app = AppContext {
                theme,
                ..AppContext::default()
            };
            let (commands, body) = scene_commands(&app, scene)?;
            let out = std::path::Path::new(OUT_DIR)
                .join(format!("topbar-{theme_name}-{}.png", scene.name()));
            raster::rasterize_to_png(
                &assets,
                &app,
                &commands,
                vec2f(WIDTH, HEIGHT),
                SCALE,
                None,
                &out,
            )?;
            let zoom = std::path::Path::new(OUT_DIR).join(format!(
                "topbar-{theme_name}-{}-band-zoom.png",
                scene.name()
            ));
            write_zoom(&out, &zoom, band_crop(body), 2)?;
            let fillet = std::path::Path::new(OUT_DIR).join(format!(
                "topbar-{theme_name}-{}-fillet-zoom.png",
                scene.name()
            ));
            write_zoom(&out, &fillet, fillet_crop(body), 4)?;
            if scene == Scene::Diff {
                let cluster = std::path::Path::new(OUT_DIR)
                    .join(format!("topbar-{theme_name}-diff-control-zoom.png"));
                let box_ = (
                    WIDTH - 360.0,
                    360.0,
                    TOPBAR_HEIGHT + goble_app::ui::shell::TOPBAR_BOTTOM_BAND + 44.0,
                );
                write_zoom(&out, &cluster, box_, 3)?;
            }
        }
    }
    Ok(())
}
