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
use goble_ui::theme::{AccentColor, ColorToken, TabColor, Theme};

use goble_app::root_view::RootView;
use goble_app::ui::shell::TOPBAR_HEIGHT;
use goble_app::ui::{Pane, PaneKind, Space, SplitDir};

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
    /// Two panes side by side: no outlines, just the two darker pane cards on
    /// the panel surface, 8 pt of it between them.
    Split,
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
            Scene::Split => "split",
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
        // The split scene: the active space is two tabs side by side, so each
        // is two pane cards, so the gutter between them can be measured.
        if scene == Scene::Split {
            let first = s.spaces[1].root.clone();
            let id = s.next_pane_id;
            s.next_pane_id += 1;
            s.spaces[1].root = Pane::Split {
                id: s.next_pane_id,
                dir: SplitDir::Horizontal,
                ratio: 0.5,
                first: Box::new(first),
                second: Box::new(Pane::Leaf {
                    id,
                    kind: PaneKind::Terminal,
                }),
            };
            s.next_pane_id += 1;
        }
        // A pane outside any repository draws no Open-changes control: the
        // plain scenes show the bar as it looks on a clean working tree.
        let active = s.active_pane_id;
        s.set_pane_path(active, dir.path().display().to_string());
        // The second tab is the selected one throughout: Chrome's selected tab
        // merges into the content with a floating neighbour on each side. Its
        // pane is the focused one, so it wears the accent corner marker.
        s.active_space = 1;
        let active_pane = s.spaces[1].root.first_leaf_id();
        s.active_pane_id = active_pane;
        s.pane_sessions.entry(active_pane).or_default();
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

/// Crop a logical rectangle out of a rendered frame, scale it up `zoom` times on
/// top of the raster pass's x2 and write it out. Unlike [`write_zoom`], the
/// rectangle starts at an explicit `top` (a tab's corner sits far below the
/// window's first row).
fn write_zoom_at(
    full: &std::path::Path,
    out: &std::path::Path,
    (left, top, width, height): (f32, f32, f32, f32),
    zoom: u32,
) -> anyhow::Result<()> {
    let frame = image::open(full)?.to_rgba8();
    let cropped = image::imageops::crop_imm(
        &frame,
        (left * SCALE) as u32,
        (top * SCALE) as u32,
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

/// Keep a crop rectangle inside the rendered frame.
fn clamp_crop(left: f32, top: f32, width: f32, height: f32) -> (f32, f32, f32, f32) {
    (
        left.max(0.0).min(WIDTH - width),
        top.max(0.0).min(HEIGHT - height),
        width,
        height,
    )
}

/// The window's own edge: the top-left corner, the topbar under the top rule and
/// the left rule running on into the pane area, so the window outline can be
/// read as one continuous frame around every surface — the bar included.
fn window_edge_crop() -> (f32, f32, f32) {
    let height = TOPBAR_HEIGHT + goble_app::ui::shell::TOPBAR_BOTTOM_BAND + 96.0;
    (0.0, 344.0, height)
}

/// The whole top of the window at a lower zoom: the window's top rule, the
/// full-width topbar band under it and the bar's bottom junction, so the band's
/// own tone against the content can be read across the window.
fn window_top_crop() -> (f32, f32, f32) {
    let height = TOPBAR_HEIGHT + goble_app::ui::shell::TOPBAR_BOTTOM_BAND + 24.0;
    (0.0, WIDTH, height)
}

/// The pane cards a frame paints: the big darker-`Bg` fills under the app
/// topbar that also carry the card's own `Border` edge, ordered left to right.
/// The panel paints no outline around them.
fn pane_cards(commands: &[RenderCommand], app: &AppContext) -> Vec<RectF> {
    let bg = app.theme.color(ColorToken::Bg);
    let border = app.theme.color(ColorToken::Border);
    let mut cards: Vec<RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == bg
                    && rect.width() > 120.0
                    && rect.height() > 120.0
                    && rect.min_y() >= TOPBAR_HEIGHT
                    && commands.iter().any(|other| {
                        matches!(
                            other,
                            RenderCommand::StrokeRect { rect: edge, color: edge_color, width, .. }
                                if edge == rect
                                    && *edge_color == border
                                    && (*width - 1.0).abs() < 0.01
                        )
                    }) =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    cards.sort_by(|a, b| a.min_x().total_cmp(&b.min_x()));
    cards
}

/// Whether a frame paints any 1 pt rounded rule that is not a pane card's own
/// `Border` edge: the panel wears no tab outline.
fn panel_outlined(commands: &[RenderCommand], app: &AppContext) -> bool {
    commands.iter().any(|command| match command {
        RenderCommand::StrokeRect {
            rect,
            width,
            corner_radius,
            ..
        } if (*width - 1.0).abs() < 0.01
            && (*corner_radius - 6.0).abs() < 0.01
            && !commands.iter().any(|other| {
                matches!(
                    other,
                    RenderCommand::FillRect { rect: filled, .. } if filled == rect
                )
            }) =>
        {
            !pane_cards(commands, app)
                .iter()
                .any(|card| card.min_x() <= rect.min_x() && card.max_x() >= rect.max_x())
        }
        _ => false,
    })
}

/// The focused pane's accent corner marker: the upper-left triangle icon.
fn accent_marker(commands: &[RenderCommand], app: &AppContext) -> Option<Vector2F> {
    let accent = app.theme.color(ColorToken::Accent);
    commands.iter().find_map(|command| match command {
        RenderCommand::DrawIcon {
            origin,
            name,
            color,
            ..
        } if name == "upper-left-triangle" && *color == accent => Some(*origin),
        _ => None,
    })
}

/// The measured chrome of one rendered frame, printed as the geometry evidence
/// for the owner's four points: the window outline's path (the whole window,
/// topbar included), the dark bar block up to it with no lighter gap, the
/// content sheet's origin and the merge line, and — for a hovered frame — that
/// the hovered pill is painted after the selected tab's inverted-corner discs.
fn print_geometry(label: &str, commands: &[RenderCommand], app: &AppContext) {
    let surface = app.theme.color(ColorToken::Surface);
    let bg = app.theme.color(ColorToken::Bg);
    let accent = app.theme.color(ColorToken::Accent);
    let band_top = TOPBAR_HEIGHT;
    let band = goble_app::ui::shell::TOPBAR_BOTTOM_BAND;
    let sheet_top = band_top + band;

    // The window's own rule is neutral: the panel wears no outline, and the
    // around the outer window.
    let window_rule = commands.iter().find_map(|command| match command {
        RenderCommand::StrokeRect {
            rect, color, width, ..
        } if rect.width() == WIDTH && rect.height() == HEIGHT => Some((*rect, *color, *width)),
        _ => None,
    });
    match window_rule {
        Some((rect, color, width)) => println!(
            "[{label}] window rule: {width} px at ({:.0},{:.0})..({:.0},{:.0}), \
             colour {:?} — neutral? {}",
            rect.min_x(),
            rect.min_y(),
            rect.max_x(),
            rect.max_y(),
            color,
            color != accent,
        ),
        None => println!("[{label}] window rule: MISSING"),
    }

    // The panel wears no outline around its tabs: only the pane cards' own
    // `Border` edges frame them, and the focused pane is marked by its accent
    // corner triangle.
    let cards = pane_cards(commands, app);
    println!(
        "[{label}] tab outline: {} (expected none); pane cards: {}",
        if panel_outlined(commands, app) {
            "PRESENT"
        } else {
            "none"
        },
        cards.len(),
    );
    if let Some(marker) = accent_marker(commands, app) {
        println!(
            "[{label}] focused corner: accent upper-left triangle at ({:.1},{:.1})",
            marker.x, marker.y
        );
    } else {
        println!("[{label}] focused corner: MISSING");
    }
    for (index, card) in cards.iter().enumerate() {
        println!(
            "[{label}] pane card {index}: ({:.1},{:.1})..({:.1},{:.1}), Bg on the panel surface",
            card.min_x(),
            card.min_y(),
            card.max_x(),
            card.max_y(),
        );
    }
    if cards.len() >= 2 {
        let gutter = cards[1].min_x() - cards[0].max_x();
        let between = vec2f(
            cards[0].max_x() + gutter * 0.5,
            cards[0].min_y() + cards[0].height() * 0.5,
        );
        let fill = commands.iter().rev().find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if between.x >= rect.min_x()
                    && between.x <= rect.max_x()
                    && between.y >= rect.min_y()
                    && between.y <= rect.max_y() =>
            {
                Some(*color)
            }
            _ => None,
        });
        println!(
            "[{label}] pane gutter: {gutter:.1} pt between the two cards at x {:.1}; \
             fill {:?} (panel Surface = {:?})",
            cards[0].max_x(),
            fill,
            surface
        );
    }

    // The bar block: the bar's Bg from the window's top edge down to the
    // content sheet. The sheet's first lighter row must be exactly the band's
    // bottom: gap = 0.
    let band_fill = commands.iter().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. }
            if *color == bg
                && (rect.min_y() - band_top).abs() < 0.01
                && (rect.height() - band).abs() < 0.01
                && rect.width() == WIDTH =>
        {
            Some(*rect)
        }
        _ => None,
    });
    let lip = commands.iter().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. }
            if *color == surface
                && (rect.min_y() - band_top).abs() < 0.01
                && rect.width() == WIDTH =>
        {
            Some(*rect)
        }
        _ => None,
    });
    let sheet = commands.iter().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. }
            if *color == surface && rect.min_x() == 0.0 && rect.height() > HEIGHT / 2.0 =>
        {
            Some(*rect)
        }
        _ => None,
    });
    println!(
        "[{label}] bar block: Bg rows 0..{:.0} (bar {:.0} + band {:.0}); band fill {};          content sheet top {:.0}; dark gap to the outline at the top = 0",
        band_top + band,
        band_top,
        band,
        if band_fill.is_some() { "yes" } else { "MISSING" },
        sheet.map(|rect| rect.min_y()).unwrap_or(f32::NAN),
    );
    println!(
        "[{label}] lighter band under the bar (old lip): {} (expected none)",
        if lip.is_some() { "PRESENT" } else { "none" }
    );
    if let Some(sheet) = sheet {
        println!(
            "[{label}] sheet origin {:.0} vs bar block end {:.0}: lighter-gap = {:.1} px",
            sheet.min_y(),
            sheet_top,
            sheet.min_y() - sheet_top
        );
    }

    // The selected tab's merge: its fill starts in the bar and ends on the pane
    // card's surface, one point past the pane card's top border.
    let body = commands.iter().find_map(|command| match command {
        RenderCommand::FillRect { rect, color, .. }
            if *color == surface
                && rect.min_y() > 0.0
                && rect.min_y() < band_top
                && rect.max_y() > band_top =>
        {
            Some(*rect)
        }
        _ => None,
    });
    if let Some(body) = body {
        let pane_inset = body.max_y() - sheet_top - 1.0;
        println!(
            "[{label}] selected merge: fill y {:.0}..{:.0} (merge line {:.0} = sheet {:.0} +              pane inset {:.0} + border 1)",
            body.min_y(),
            body.max_y(),
            body.max_y(),
            sheet_top,
            pane_inset
        );
    }

    // The fillet-vs-pill layering: the hovered pill must be painted after the
    // selected tab's inverted-corner discs.
    let pill_bottom = band_top - 1.0;
    let pill = commands.iter().position(|command| match command {
        RenderCommand::FillRect { rect, color, .. } => {
            *color == app.theme.color(ColorToken::Hover)
                && (rect.max_y() - pill_bottom).abs() < 0.01
        }
        _ => false,
    });
    let discs: Vec<usize> = commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == bg
                    && (rect.width() - 24.0).abs() < 0.01
                    && (rect.height() - 24.0).abs() < 0.01 =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();
    if let Some(pill) = pill {
        println!(
            "[{label}] hovered pill painted at command {pill} vs fillet discs {discs:?}:              pill above discs = {}",
            discs.iter().all(|disc| pill > *disc)
        );
    }
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
        Scene::Split,
    ];
    // The owner reads the window frame in the purple accent: the preview draws
    // that state too, so the window outline is visible in the evidence.
    let purple = Theme {
        accent: AccentColor::Purple,
        ..Theme::dark()
    };
    let themes = [
        ("dark", Theme::dark()),
        ("light", Theme::light()),
        ("purple", purple),
    ];
    for scene in scenes {
        for (theme_name, theme) in themes.clone() {
            let app = AppContext {
                theme,
                ..AppContext::default()
            };
            let (commands, body) = scene_commands(&app, scene)?;
            if matches!(scene, Scene::Rest | Scene::Hover | Scene::Split) {
                print_geometry(&format!("{theme_name}/{}", scene.name()), &commands, &app);
            }
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
            // The window-edge evidence: the outline as one frame around the
            // whole window, the bar included, and the tone of the band under it.
            let edge = std::path::Path::new(OUT_DIR).join(format!(
                "topbar-{theme_name}-{}-window-edge-zoom.png",
                scene.name()
            ));
            write_zoom(&out, &edge, window_edge_crop(), 4)?;
            let top = std::path::Path::new(OUT_DIR).join(format!(
                "topbar-{theme_name}-{}-window-top-zoom.png",
                scene.name()
            ));
            write_zoom(&out, &top, window_top_crop(), 3)?;
            // The panel corner evidence: no outline around the tab — just the
            // focused pane's accent corner triangle over its darker card, with
            // the neutral window rule outside it.
            let cards = pane_cards(&commands, &app);
            if let Some(card) = cards.first() {
                let top_corner = std::path::Path::new(OUT_DIR).join(format!(
                    "topbar-{theme_name}-{}-tab-top-corner-zoom.png",
                    scene.name()
                ));
                write_zoom_at(
                    &out,
                    &top_corner,
                    clamp_crop(card.min_x() - 10.0, card.min_y() - 10.0, 52.0, 52.0),
                    8,
                )?;
                let corner = std::path::Path::new(OUT_DIR).join(format!(
                    "topbar-{theme_name}-{}-tab-corner-zoom.png",
                    scene.name()
                ));
                write_zoom_at(
                    &out,
                    &corner,
                    clamp_crop(card.max_x() - 40.0, card.max_y() - 40.0, 48.0, 48.0),
                    8,
                )?;
            }
            if scene == Scene::Split && cards.len() >= 2 {
                let boundary = (cards[0].max_x() + cards[1].min_x()) * 0.5;
                let split = std::path::Path::new(OUT_DIR)
                    .join(format!("topbar-{theme_name}-split-zoom.png"));
                write_zoom_at(
                    &out,
                    &split,
                    clamp_crop(boundary - 20.0, cards[0].min_y() - 8.0, 40.0, 96.0),
                    8,
                )?;
            }
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
