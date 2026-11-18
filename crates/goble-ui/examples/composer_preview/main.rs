//! Headless PNG preview of the `goble-ui` composer and content-lane scenes.
//!
//! The native window cannot be captured in this environment (no display
//! permission), so these PNGs are the visual verification harness: the scenes
//! build real element trees from the public `goble_ui` API, lay them out, paint
//! them into a `RenderCommand` list and CPU-rasterize that list into an RGBA
//! PNG. The rasterizer itself lives in [`raster`] and is reusable by other
//! crates (see its module docs and the macro-free integration snippet there).
//!
//! Scenes:
//!
//! - `composer` — the agent `ChatComposer` at the reference photo's framing
//!   (876 logical points, 2x), context controls populated, empty draft with its
//!   placeholder. `--focused` renders the focused state (the editor's caret);
//!   the card itself paints no focus ring — the owner asked for no blue border
//!   or margin around the rich input.
//! - `menus` — the same composer with the environment popup open and with the
//!   branch popup open (two PNGs, `<stem>-env-menu.png` / `<stem>-branch-menu.png`).
//! - `env-menu`, `branch-menu` — one of those two, to `--out`.
//! - `model-menu` — the composer with the DSH model card open on its root pane
//!   (`Model` / `Effort` cells), anchored above the model chip.
//! - `model-pane` — the same card drilled into the provider-grouped model list,
//!   with DSH's trailing check on the model in force.
//! - `model-effort` — the same card drilled into the effort list, checked on the
//!   level in force; `--long-model` swaps in names that overflow the chip and the
//!   cells so the ellipsis pass is visible.
//! - `slash-menu` — the `/` command list as the DSH card over the composer, with
//!   the context band hidden so the card is the input segment plus the menu.
//! - `terminal-placeholder` — the shell bar's rich input: "Run any command" and
//!   the fixed instruction row under the footer.
//! - `agent-placeholder` — the agent input: "Ask anything..." with the same
//!   fixed-height instruction row.
//! - `lane` — a 1600x900 window (default) with the real [`ContentLane`] around a
//!   representative transcript + the composer; `--width 800` renders the
//!   narrow-window pass-through. `--dragging` paints the dragged edge rule and
//!   `--drag-edge right` picks the right edge.
//! - `all` — every scene above into the `--out` directory.
//!
//! ```text
//! cargo run -p goble-ui --example composer_preview -- \
//!     --scene composer --out /tmp/goble-work/out/composer.png --scale 2
//! /tmp/goble-work/render.sh            # every scene into /tmp/goble-work/out
//! ```
//!
//! The scene assets are the crate's own: fonts from `assets/fonts`, icons from
//! `assets/icons`, located through `env!("CARGO_MANIFEST_DIR")` here and handed
//! to the rasterizer.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use goble_ui::elements::chat_composer::{
    AGENT_COMPOSER_PLACEHOLDER, TERMINAL_COMPOSER_PLACEHOLDER,
};
use goble_ui::elements::{
    AppContext, ChatComposer, ComposerModelTray, ContentLane, CrossAxisAlignment, EdgeInsets,
    Element, Expanded, Fill, Flex, LaneEdge, MainAxisSize, ModelTrayHarness, ModelTrayModel,
    ModelTrayState, Padding, PopupMenuItem, ShortcutHint, SlashMenu, SlashMenuItem, Text,
};
use goble_ui::geometry::vec2f;
use goble_ui::theme::{ColorToken, Theme};

mod raster;

use raster::PreviewAssets;

const REFERENCE_WIDTH: f32 = 876.0;
/// Logical margin a scene leaves above the element block, so the card's top
/// edge is visible against the `Bg` page the way the reference photo frames it.
/// The composer brings its own side gutters (`COMPOSER_CARD_GUTTER`), so the
/// scene leaves none of its own: the card's width is the pane's width.
const SCENE_TOP_PAD: f32 = 12.0;

/// goble-ui's asset root: the bundled fonts and icon SVGs the rasterizer draws
/// with. `CARGO_MANIFEST_DIR` is this example's package (goble-ui).
fn assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let options = Options::parse()?;
    let assets = PreviewAssets::load(&assets_dir())?;
    match options.scene.as_str() {
        "composer" => render_composer(&options, &assets)?,
        "menus" => {
            render_menu(&options, &assets, MenuKind::Environment)?;
            render_menu(&options, &assets, MenuKind::Branch)?;
        }
        "env-menu" => render_menu(&options, &assets, MenuKind::Environment)?,
        "branch-menu" => render_menu(&options, &assets, MenuKind::Branch)?,
        "model-menu" => render_model_menu(&options, &assets)?,
        "model-pane" => {
            render_model_pane(&options, &assets, goble_ui::elements::ModelTrayPane::Model)?
        }
        "model-effort" => {
            render_model_pane(&options, &assets, goble_ui::elements::ModelTrayPane::Effort)?
        }
        "slash-menu" => render_slash_menu(&options, &assets)?,
        "terminal-placeholder" => {
            render_placeholder(&options, &assets, PlaceholderSurface::Terminal)?
        }
        "agent-placeholder" => render_placeholder(&options, &assets, PlaceholderSurface::Agent)?,
        "lane" => render_lane(&options, &assets)?,
        "all" => {
            // `all` writes one file per scene into the `--out` directory (the
            // parent directory when `--out` names a PNG).
            let dir = match options.out.extension() {
                Some(_) => options
                    .out
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(".")),
                None => options.out.clone(),
            };
            std::fs::create_dir_all(&dir)?;
            render_composer(
                &options.sub_scene("composer", dir.join("composer.png")),
                &assets,
            )?;
            let focused = options.sub_scene("composer", dir.join("composer-focused.png"));
            render_composer(
                &Options {
                    focused: true,
                    ..focused
                },
                &assets,
            )?;
            let menus = options.sub_scene("menus", dir.join("menus.png"));
            render_menu(&menus, &assets, MenuKind::Environment)?;
            render_menu(&menus, &assets, MenuKind::Branch)?;
            render_model_menu(
                &options.sub_scene("model-menu", dir.join("model-menu.png")),
                &assets,
            )?;
            render_model_pane(
                &options.sub_scene("model-pane", dir.join("model-pane.png")),
                &assets,
                goble_ui::elements::ModelTrayPane::Model,
            )?;
            render_model_pane(
                &options.sub_scene("model-effort", dir.join("model-effort.png")),
                &assets,
                goble_ui::elements::ModelTrayPane::Effort,
            )?;
            render_slash_menu(
                &options.sub_scene("slash-menu", dir.join("slash-menu.png")),
                &assets,
            )?;
            render_placeholder(
                &options.sub_scene("terminal-placeholder", dir.join("placeholder-terminal.png")),
                &assets,
                PlaceholderSurface::Terminal,
            )?;
            render_placeholder(
                &options.sub_scene("agent-placeholder", dir.join("placeholder-agent.png")),
                &assets,
                PlaceholderSurface::Agent,
            )?;
            for (name, width) in [("lane-wide.png", 1600.0), ("lane-narrow.png", 800.0)] {
                let lane = options.sub_scene("lane", dir.join(name));
                render_lane(
                    &Options {
                        width: Some(width),
                        height: Some(900.0),
                        ..lane
                    },
                    &assets,
                )?;
            }
        }
        other => anyhow::bail!(
            "unknown scene {other:?}; expected composer, menus, env-menu, branch-menu, \
             model-menu, model-pane, slash-menu, terminal-placeholder, agent-placeholder, \
             lane or all"
        ),
    }
    Ok(())
}

/// Print the paint commands that place text, icons and boxes, for debugging a
/// scene's geometry (used by `--dump-commands`).
fn dump_commands(commands: &[goble_ui::render::RenderCommand]) {
    use goble_ui::render::RenderCommand;
    for command in commands {
        match command {
            RenderCommand::DrawText {
                origin,
                text,
                font_size,
                color,
                max_width,
                ..
            } => println!(
                "text   {:.1},{:.1} size {font_size} color {:?} max_w {max_width} {text:?}",
                origin.x, origin.y, color
            ),
            RenderCommand::DrawIcon {
                origin,
                name,
                size,
                color,
            } => println!(
                "icon   {:.1},{:.1} size {size} color {:?} {name:?}",
                origin.x, origin.y, color
            ),
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } => println!(
                "fill   {:.1},{:.1} {:.1}x{:.1} r{corner_radius} color {:?}",
                rect.min_x(),
                rect.min_y(),
                rect.width(),
                rect.height(),
                color
            ),
            RenderCommand::StrokeRect {
                rect,
                color,
                width,
                corner_radius,
            } => println!(
                "stroke {:.1},{:.1} {:.1}x{:.1} w{width} r{corner_radius} color {:?}",
                rect.min_x(),
                rect.min_y(),
                rect.width(),
                rect.height(),
                color
            ),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

struct Options {
    scene: String,
    out: PathBuf,
    scale: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    theme: String,
    dragging: bool,
    drag_edge: LaneEdge,
    focused: bool,
    dump_commands: bool,
    /// Use the long-form catalogue for the composer/model scenes: a model whose
    /// display name and effort overflow the chip's 360pt cap and the menu's
    /// cells, the case the value-ellipsis pass exists for.
    long_model: bool,
}

impl Options {
    fn sub_scene(&self, scene: &str, out: PathBuf) -> Self {
        Self {
            scene: scene.to_string(),
            out,
            scale: self.scale,
            width: self.width,
            height: self.height,
            theme: self.theme.clone(),
            dragging: self.dragging,
            drag_edge: self.drag_edge,
            focused: self.focused,
            dump_commands: self.dump_commands,
            long_model: self.long_model,
        }
    }

    fn parse() -> anyhow::Result<Self> {
        let mut scene = String::from("composer");
        let mut out: Option<PathBuf> = None;
        let mut scale = None;
        let mut width = None;
        let mut height = None;
        let mut theme = String::from("dark");
        let mut dragging = false;
        let mut drag_edge = LaneEdge::Left;
        let mut focused = false;
        let mut dump_commands = false;
        let mut long_model = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut value = |name: &str| -> anyhow::Result<String> {
                args.next()
                    .ok_or_else(|| anyhow::anyhow!("{name} needs a value"))
            };
            match arg.as_str() {
                "--scene" => scene = value("--scene")?,
                "--out" => out = Some(PathBuf::from(value("--out")?)),
                "--scale" => {
                    scale = Some(value("--scale")?.parse::<f32>()?);
                }
                "--width" => {
                    width = Some(value("--width")?.parse::<f32>()?);
                }
                "--height" => {
                    height = Some(value("--height")?.parse::<f32>()?);
                }
                "--theme" => theme = value("--theme")?,
                "--dragging" => dragging = true,
                "--drag-edge" => {
                    drag_edge = match value("--drag-edge")?.as_str() {
                        "left" => LaneEdge::Left,
                        "right" => LaneEdge::Right,
                        other => anyhow::bail!("--drag-edge expects left or right, got {other:?}"),
                    }
                }
                "--focused" => focused = true,
                "--dump-commands" => dump_commands = true,
                "--long-model" => long_model = true,
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                other => anyhow::bail!("unknown argument {other:?} (try --help)"),
            }
        }
        let out = out.unwrap_or_else(|| {
            let name = match scene.as_str() {
                "all" => "composer".to_string(),
                other => other.to_string(),
            };
            PathBuf::from("/tmp/goble-work/out").join(format!("{name}.png"))
        });
        Ok(Self {
            scene,
            out,
            scale,
            width,
            height,
            theme,
            dragging,
            drag_edge,
            focused,
            dump_commands,
            long_model,
        })
    }

    fn scale(&self, default: f32) -> f32 {
        self.scale.unwrap_or(default)
    }

    fn theme(&self) -> anyhow::Result<Theme> {
        match self.theme.as_str() {
            "dark" | "" => Ok(Theme::dark()),
            "light" => Ok(Theme::light()),
            "midnight" => Ok(Theme::midnight()),
            other => anyhow::bail!("unknown theme {other:?}; expected dark, light or midnight"),
        }
    }
}

fn print_usage() {
    println!(
        "composer_preview — rasterize goble-ui scenes to PNG\n\
         \n\
         --scene composer|menus|env-menu|branch-menu|model-menu|model-pane|model-effort|slash-menu|\n\
         --scene terminal-placeholder|agent-placeholder|lane|all  (default: composer)\n\
         --out PATH        output PNG path (default: /tmp/goble-work/out/<scene>.png)\n\
         --scale N         device pixel ratio to rasterize at (default: 2 for composer/menus, 1 for lane)\n\
         --width N         logical scene width (default: 876 for composer/menus, 1600 for lane)\n\
         --height N        logical scene height (default: content-sized, or 900 for lane)\n\
         --theme NAME      dark (default), light or midnight\n\
         --dragging        lane scene: paint the lane's in-drag edge rule\n\
         --drag-edge E     lane drag edge: left (default) or right\n\
         --focused         composer scene: render the editor focused (caret only; no ring)\n\
         --long-model      composer/model scenes: use the overflow-length model catalogue\n\
         --dump-commands   print every DrawText/DrawIcon/rect command of the scene\n\
         \n\
         `menus` writes two PNGs next to --out: <stem>-env-menu.png and <stem>-branch-menu.png.\n\
         Use --scene all to render every scene."
    );
}

fn with_out_suffix(out: &Path, suffix: &str) -> PathBuf {
    let is_dir = out.is_dir() || out.to_string_lossy().ends_with(std::path::MAIN_SEPARATOR);
    if is_dir {
        return out.join(format!("{suffix}.png"));
    }
    let stem = out
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "scene".to_string());
    let name = format!("{stem}-{suffix}.png");
    out.with_file_name(name)
}

/// The topmost `y` any command paints at. A popup opens above its trigger, so
/// a menu scene's commands reach negative `y`; the scene uses that to leave
/// exactly enough headroom for the open panel instead of a guessed margin.

fn frame_scene(
    width: f32,
    scale: f32,
    app: &AppContext,
    assets: &PreviewAssets,
    out: &Path,
    content_height: f32,
    build: impl Fn(f32) -> Box<dyn Element>,
) -> anyhow::Result<()> {
    let mut probe = build(0.0);
    let probe_commands = raster::render_commands(&mut probe, vec2f(width, 4000.0), app);
    let headroom = (-raster::commands_min_y(&probe_commands)).max(0.0) + SCENE_TOP_PAD;

    let mut root = build(headroom);
    let canvas = vec2f(width, headroom + content_height);
    raster::render_to_png(assets, app, &mut root, canvas, scale, None, out)
}

// ---------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------

/// The menu open flags an app owns across the composer's per-frame rebuild.
/// The composer reads them at paint time, so a scene sets the one it wants
/// open before layout.
#[derive(Clone)]
struct MenuFlags {
    /// The catalogue the model chip and card read: the short one by default,
    /// the overflow-length one with `--long-model`.
    harnesses: Vec<ModelTrayHarness>,
    tray_model: &'static str,
    tray_level: &'static str,
    dir_open: Rc<RefCell<bool>>,
    branch_open: Rc<RefCell<bool>>,
    harness_open: Rc<RefCell<bool>>,
    model_open: Rc<RefCell<bool>>,
    tray_open: Rc<RefCell<bool>>,
    tray_state: Rc<RefCell<ModelTrayState>>,
    slash_index: Rc<RefCell<usize>>,
    slash_dismissed: Rc<RefCell<bool>>,
    caret: Rc<RefCell<usize>>,
}

impl MenuFlags {
    /// The long-form catalogue: names that overflow the chip and the cells, plus
    /// a three-level model so the effort caption and the effort pane both show
    /// real values.
    fn with_long_catalogue(mut self) -> Self {
        self.harnesses = model_tray_harnesses(true);
        self.tray_model = "deepseek-v4-flash";
        self.tray_level = "high";
        self
    }

    fn new() -> Self {
        Self {
            dir_open: Rc::new(RefCell::new(false)),
            branch_open: Rc::new(RefCell::new(false)),
            harness_open: Rc::new(RefCell::new(false)),
            model_open: Rc::new(RefCell::new(false)),
            tray_open: Rc::new(RefCell::new(false)),
            tray_state: Rc::new(RefCell::new(ModelTrayState::default())),
            harnesses: model_tray_harnesses(false),
            tray_model: "deepseek-flash",
            tray_level: "off",
            slash_index: Rc::new(RefCell::new(0)),
            slash_dismissed: Rc::new(RefCell::new(false)),
            caret: Rc::new(RefCell::new(0)),
        }
    }
}

/// The model catalogue the DSH card lists: two providers, the models each one
/// runs, and the thinking levels those models accept. `long` swaps in names long
/// enough to overflow the chip's 360pt cap, the menu's 240pt cells and the
/// menu's 420pt list cap, so the ellipsis passes are visible in the PNGs.
fn model_tray_harnesses(long: bool) -> Vec<ModelTrayHarness> {
    let thinking = || vec!["off".to_string(), "high".to_string(), "max".to_string()];
    if long {
        return vec![
            ModelTrayHarness::new("deepseek", "DeepSeek").with_models(vec![
                ModelTrayModel::new(
                    "deepseek-v4-flash",
                    "DeepSeek-V4.1-Flash-Experimental-Preview-Candidate-2026-11",
                )
                .with_thinking(thinking()),
                ModelTrayModel::new(
                    "deepseek-v4-pro",
                    "DeepSeek-V4.1-Pro-Reasoning-Long-Context",
                )
                .with_thinking(thinking()),
            ]),
            ModelTrayHarness::new("anthropic", "Anthropic").with_models(vec![ModelTrayModel::new(
                "claude-sonnet-4-5-thinking",
                "Claude Sonnet 4.5 Thinking Extended",
            )
            .with_thinking(thinking())]),
        ];
    }
    let thinking = || vec!["off".to_string(), "on".to_string()];
    vec![
        ModelTrayHarness::new("deepseek", "DeepSeek").with_models(vec![
            ModelTrayModel::new("deepseek-flash", "DeepSeek Flash").with_thinking(thinking()),
            ModelTrayModel::new("deepseek-pro", "DeepSeek Pro").with_thinking(thinking()),
        ]),
        ModelTrayHarness::new("anthropic", "Anthropic").with_models(vec![ModelTrayModel::new(
            "claude-sonnet",
            "Claude Sonnet",
        )
        .with_thinking(thinking())]),
    ]
}

/// The `/` command list as the app hands it to the view: DSH's row faces — a
/// glyph, the command as it is typed, and what it does.
fn slash_menu_items() -> Vec<SlashMenuItem> {
    vec![
        SlashMenuItem::new("new", "Start a fresh conversation").with_icon("new-chat"),
        SlashMenuItem::new("clear", "Clear this conversation's transcript").with_icon("trash-02"),
        SlashMenuItem::new("fork", "Branch this conversation into a new one")
            .with_icon("git-branch"),
        SlashMenuItem::new("compact", "Compact older conversation history").with_icon("flash"),
        SlashMenuItem::new("permissions", "Switch the permission preset").with_icon("sliders"),
        SlashMenuItem::new("model deepseek-flash", "Run this pane on another model")
            .with_icon("database"),
    ]
}

/// The agent composer as the app wires it (`views/chat_view/transcript.rs`):
/// the same builder calls, on the same public element, with the reference
/// photo's context values.
fn composer_element(flags: &MenuFlags, focused: bool) -> Box<dyn Element> {
    composer_with(flags, focused, "").finish()
}

/// The agent composer builder the menu scenes start from: the same calls the app
/// makes, with the draft the scene needs.
fn composer_with(flags: &MenuFlags, focused: bool, value: &str) -> ChatComposer {
    let dir_items = vec![
        PopupMenuItem::new("brindlepad-web")
            .with_icon("folder")
            .selected(),
        PopupMenuItem::new("goble").with_icon("folder"),
        PopupMenuItem::new("sandbox").with_icon("folder"),
        PopupMenuItem::new("/Users/adrian/Projects/acme-api").with_icon("folder"),
    ];
    // The branch tray: the repository header row, its branches with the checked
    // one marked, and the worktree action.
    let branch_items = vec![
        PopupMenuItem::new("brindlepad-web").with_icon("folder"),
        PopupMenuItem::new("main").with_icon("branch").selected(),
        PopupMenuItem::new("feature/checkout-flow").with_icon("branch"),
        PopupMenuItem::new("fix/cart-totals").with_icon("branch"),
        PopupMenuItem::new("release/2.4").with_icon("branch"),
        PopupMenuItem::new("Work in a New Worktree")
            .with_icon("branch")
            .with_description("Keeps your checkout untouched")
            .with_separator_before(),
    ];
    let harness_items = vec![
        PopupMenuItem::new("This Computer")
            .with_icon("environment-computer")
            .selected(),
        // The reference environment tray explains what the remote choice is:
        // the 12px secondary line under the label.
        PopupMenuItem::new("Remote")
            .with_icon("environment-cloud")
            .with_description("Clones this repo in a cloud environment…"),
    ];
    let model_items = vec![
        PopupMenuItem::new("DeepSeek Flash").selected(),
        PopupMenuItem::new("Claude Sonnet 4.5"),
        PopupMenuItem::new("GPT-5.1 Codex"),
    ];
    ChatComposer::new()
        .with_value(value)
        .with_placeholder("Ask anything, @ to mention, / for actions")
        .with_caret(flags.caret.clone())
        .with_focused(focused)
        // The agent pane's composer: the context row rides above the editor and
        // the bar spans its pane, exactly as the app builds it.
        .with_context_above_editor(true)
        .with_edge_to_edge(true)
        .with_path_label("brindlepad-web")
        .with_branch_label("main")
        .with_harness_label("This Computer")
        .with_harness_icon("environment-computer")
        .with_model_label("DeepSeek Flash")
        // The DSH two-level model card the chip opens: the app's real catalogue
        // shape (providers → models → thinking levels) and selection callbacks.
        .with_model_tray(
            ComposerModelTray::new(
                flags.harnesses.clone(),
                flags.tray_state.clone(),
                flags.tray_open.clone(),
            )
            .with_current(flags.tray_model, flags.tray_level)
            .with_on_select_model(|_| {})
            .with_on_select_level(|_| {}),
        )
        .with_dir_menu(dir_items, flags.dir_open.clone(), |_| {})
        .with_branch_menu(branch_items, flags.branch_open.clone(), |_| {})
        .with_harness_menu(harness_items, flags.harness_open.clone(), |_| {})
        .with_model_menu(model_items, flags.model_open.clone(), |_| {})
        .with_on_attach(|| {})
        .with_on_select_model(|| {})
        .with_on_send(|_text| {})
}

/// The command list the `/` draft opens: the DSH card over the composer, with
/// the composer's context band stepped out of the way (the scene the owner asked
/// for: input segment + slash menu only).
fn slash_scene(flags: &MenuFlags, focused: bool) -> Box<dyn Element> {
    let slash_index = flags.slash_index.clone();
    let composer = composer_with(flags, focused, "/")
        .with_slash_menu(
            slash_menu_items(),
            flags.slash_index.clone(),
            flags.slash_dismissed.clone(),
        )
        .with_on_slash_move({
            let slash_index = slash_index.clone();
            move |index| *slash_index.borrow_mut() = index
        })
        .with_on_slash_accept(|_| {})
        .with_on_slash_dismiss(|| {})
        .finish();
    let menu = SlashMenu::new(slash_menu_items(), flags.slash_index.clone()).with_on_move({
        let slash_index = slash_index.clone();
        move |index| *slash_index.borrow_mut() = index
    });
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(menu.finish())
        .with_child(composer)
        .finish()
}

fn render_composer(options: &Options, assets: &PreviewAssets) -> anyhow::Result<()> {
    let theme = options.theme()?;
    let mut app = AppContext::default();
    app.theme = theme;
    let width = options.width.unwrap_or(REFERENCE_WIDTH);
    let scale = options.scale(2.0);
    let flags = if options.long_model {
        MenuFlags::new().with_long_catalogue()
    } else {
        MenuFlags::new()
    };
    let focused = options.focused;

    // Measure the composer at the width the scene will give it, so the canvas
    // is exactly the block plus the scene's top margin instead of a guess.
    let mut probe = composer_element(&flags, false);
    let content = raster::layout_size(&mut probe, vec2f(width, 4000.0), &app);
    if options.dump_commands {
        let mut element = composer_element(&flags, focused);
        let commands = raster::render_commands(&mut element, vec2f(width, 4000.0), &app);
        dump_commands(&commands);
    }
    let build_flags = flags.clone();
    frame_scene(
        width,
        scale,
        &app,
        assets,
        &options.out,
        content.y,
        move |headroom| {
            Padding::new(
                composer_element(&build_flags, focused),
                EdgeInsets::new(0.0, headroom, 0.0, 0.0),
            )
            .finish()
        },
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    Environment,
    Branch,
}

impl MenuKind {
    fn suffix(self) -> &'static str {
        match self {
            MenuKind::Environment => "env-menu",
            MenuKind::Branch => "branch-menu",
        }
    }
}

fn render_menu(options: &Options, assets: &PreviewAssets, kind: MenuKind) -> anyhow::Result<()> {
    let theme = options.theme()?;
    let mut app = AppContext::default();
    app.theme = theme;
    let width = options.width.unwrap_or(REFERENCE_WIDTH);
    let scale = options.scale(2.0);
    let out = if options.scene == "menus" {
        with_out_suffix(&options.out, kind.suffix())
    } else {
        options.out.clone()
    };

    let flags = MenuFlags::new();
    let focused = options.focused;
    match kind {
        MenuKind::Environment => *flags.harness_open.borrow_mut() = true,
        MenuKind::Branch => *flags.branch_open.borrow_mut() = true,
    }
    let mut probe = composer_element(&flags, false);
    let content = raster::layout_size(&mut probe, vec2f(width, 4000.0), &app);
    if options.dump_commands {
        let mut element = composer_element(&flags, focused);
        let commands = raster::render_commands(&mut element, vec2f(width, 4000.0), &app);
        dump_commands(&commands);
    }
    let build_flags = flags.clone();
    // The auto-framing pass finds the open panel above the composer and gives
    // the scene exactly that headroom.
    frame_scene(
        width,
        scale,
        &app,
        assets,
        &out,
        content.y,
        move |headroom| {
            Padding::new(
                composer_element(&build_flags, focused),
                EdgeInsets::new(0.0, headroom, 0.0, 0.0),
            )
            .finish()
        },
    )
}

/// The composer with the DSH model card open on its root pane: the `Model` and
/// `Effort` cells with their current values and the chevrons DSH drills with.
fn render_model_menu(options: &Options, assets: &PreviewAssets) -> anyhow::Result<()> {
    let mut app = AppContext::default();
    app.theme = options.theme()?;
    let width = options.width.unwrap_or(REFERENCE_WIDTH);
    let scale = options.scale(2.0);
    let flags = if options.long_model {
        MenuFlags::new().with_long_catalogue()
    } else {
        MenuFlags::new()
    };
    *flags.tray_open.borrow_mut() = true;
    let focused = options.focused;
    let mut probe = composer_element(&flags, false);
    let content = raster::layout_size(&mut probe, vec2f(width, 4000.0), &app);
    if options.dump_commands {
        let mut element = composer_element(&flags, focused);
        let commands = raster::render_commands(&mut element, vec2f(width, 4000.0), &app);
        dump_commands(&commands);
    }
    let build_flags = flags.clone();
    frame_scene(
        width,
        scale,
        &app,
        assets,
        &options.out,
        content.y,
        move |headroom| {
            Padding::new(
                composer_element(&build_flags, focused),
                EdgeInsets::new(0.0, headroom, 0.0, 0.0),
            )
            .finish()
        },
    )
}

/// The same card drilled into one of its lists: the model list with provider
/// headings over the models they run and DSH's trailing check on the value in
/// force, or the effort list with its check on the level in force.
fn render_model_pane(
    options: &Options,
    assets: &PreviewAssets,
    pane: goble_ui::elements::ModelTrayPane,
) -> anyhow::Result<()> {
    let mut app = AppContext::default();
    app.theme = options.theme()?;
    let width = options.width.unwrap_or(REFERENCE_WIDTH);
    let scale = options.scale(2.0);
    let flags = if options.long_model {
        MenuFlags::new().with_long_catalogue()
    } else {
        MenuFlags::new()
    };
    // The effort pane opens on the level in force, the model pane on the model
    // in force: the card marks what the composer runs.
    let level = flags
        .harnesses
        .iter()
        .flat_map(|group| group.models.iter())
        .find(|model| model.id == flags.tray_model)
        .and_then(|model| {
            model
                .thinking
                .iter()
                .position(|one| one == flags.tray_level)
        })
        .unwrap_or(0);
    *flags.tray_open.borrow_mut() = true;
    *flags.tray_state.borrow_mut() = ModelTrayState {
        pane,
        cell: 0,
        cell_active: false,
        row: 0,
        level,
        seeded: true,
    };
    let focused = options.focused;
    let mut probe = composer_element(&flags, false);
    let content = raster::layout_size(&mut probe, vec2f(width, 4000.0), &app);
    if options.dump_commands {
        let mut element = composer_element(&flags, focused);
        let commands = raster::render_commands(&mut element, vec2f(width, 4000.0), &app);
        dump_commands(&commands);
    }
    let build_flags = flags.clone();
    frame_scene(
        width,
        scale,
        &app,
        assets,
        &options.out,
        content.y,
        move |headroom| {
            Padding::new(
                composer_element(&build_flags, focused),
                EdgeInsets::new(0.0, headroom, 0.0, 0.0),
            )
            .finish()
        },
    )
}

/// The `/` command list: the DSH card over the composer, with the context band
/// hidden so the card is just the input segment plus the menu above it.
fn render_slash_menu(options: &Options, assets: &PreviewAssets) -> anyhow::Result<()> {
    let mut app = AppContext::default();
    app.theme = options.theme()?;
    let width = options.width.unwrap_or(REFERENCE_WIDTH);
    let scale = options.scale(2.0);
    let flags = MenuFlags::new();
    let focused = options.focused;
    let mut probe = slash_scene(&flags, false);
    let content = raster::layout_size(&mut probe, vec2f(width, 4000.0), &app);
    if options.dump_commands {
        let mut element = slash_scene(&flags, focused);
        let commands = raster::render_commands(&mut element, vec2f(width, 4000.0), &app);
        dump_commands(&commands);
    }
    let build_flags = flags.clone();
    frame_scene(
        width,
        scale,
        &app,
        assets,
        &options.out,
        content.y,
        move |headroom| {
            Padding::new(
                slash_scene(&build_flags, focused),
                EdgeInsets::new(0.0, headroom, 0.0, 0.0),
            )
            .finish()
        },
    )
}

///
/// Which surface's placeholder a scene shows: the plain shell's rich input, or
/// the agent/chat input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaceholderSurface {
    Terminal,
    Agent,
}

impl PlaceholderSurface {
    fn placeholder(self) -> &'static str {
        match self {
            PlaceholderSurface::Terminal => TERMINAL_COMPOSER_PLACEHOLDER,
            PlaceholderSurface::Agent => AGENT_COMPOSER_PLACEHOLDER,
        }
    }
}

/// The composer as that surface builds it: the shell bar carries only the
/// shell's own context and its one instruction row, while the agent input adds
/// the environment and model controls and its own hint.
fn placeholder_element(
    flags: &MenuFlags,
    surface: PlaceholderSurface,
    focused: bool,
) -> Box<dyn Element> {
    let mut composer = ChatComposer::new()
        .with_value("")
        .with_caret(flags.caret.clone())
        .with_focused(focused)
        .with_context_above_editor(true)
        .with_edge_to_edge(true)
        .with_path_label("brindlepad-web")
        .with_branch_label("main")
        .with_placeholder(surface.placeholder())
        .with_hints(match surface {
            PlaceholderSurface::Terminal => {
                vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")]
            }
            PlaceholderSurface::Agent => vec![ShortcutHint::new(&["/"], "commands")],
        })
        .with_on_send(|_| {});
    if surface == PlaceholderSurface::Agent {
        composer = composer
            .with_harness_label("This Computer")
            .with_harness_icon("environment-computer")
            .with_model_label("DeepSeek Flash")
            .with_model_tray(
                ComposerModelTray::new(
                    model_tray_harnesses(false),
                    flags.tray_state.clone(),
                    flags.tray_open.clone(),
                )
                .with_current("deepseek-flash", "off")
                .with_on_select_model(|_| {})
                .with_on_select_level(|_| {}),
            )
            .with_on_attach(|| {});
    }
    composer.finish()
}

/// The terminal bar vs the agent input placeholder: "Run any command" on the
/// shell surface, "Ask anything..." on the agent surface, each with the fixed
/// instruction row under the footer.
fn render_placeholder(
    options: &Options,
    assets: &PreviewAssets,
    surface: PlaceholderSurface,
) -> anyhow::Result<()> {
    let mut app = AppContext::default();
    app.theme = options.theme()?;
    let width = options.width.unwrap_or(REFERENCE_WIDTH);
    let scale = options.scale(2.0);
    let flags = MenuFlags::new();
    let focused = options.focused;
    let mut probe = placeholder_element(&flags, surface, false);
    let content = raster::layout_size(&mut probe, vec2f(width, 4000.0), &app);
    if options.dump_commands {
        let mut element = placeholder_element(&flags, surface, focused);
        let commands = raster::render_commands(&mut element, vec2f(width, 4000.0), &app);
        dump_commands(&commands);
    }
    let build_flags = flags.clone();
    frame_scene(
        width,
        scale,
        &app,
        assets,
        &options.out,
        content.y,
        move |headroom| {
            Padding::new(
                placeholder_element(&build_flags, surface, focused),
                EdgeInsets::new(0.0, headroom, 0.0, 0.0),
            )
            .finish()
        },
    )
}

/// A full-height pane column: a representative transcript and the real
/// `ChatComposer` pinned under it. This is the content a `ContentLane` bounds.
///
/// The pane paints `Surface` as its own background so the centred content
/// column is visible against the `Bg` gutters: the lane element itself paints
/// nothing at rest, which is the point of the invisible edge.
fn lane_pane(flags: &MenuFlags, app: &AppContext) -> Box<dyn Element> {
    let user_bubble = |text: &str| -> Box<dyn Element> {
        goble_ui::elements::Container::new(
            Text::new(text)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(14.0)
                .finish(),
        )
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(12.0)
        .with_padding(EdgeInsets::new(14.0, 10.0, 14.0, 10.0))
        .finish()
    };
    let assistant = |text: &str| -> Box<dyn Element> {
        Text::new(text)
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(14.0)
            .with_line_height(1.4)
            .finish()
    };
    let tool_row = goble_ui::elements::Container::new(
        Text::new("Read  crates/goble-ui/src/elements/content_lane.rs")
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(12.0)
            .with_font_family(goble_ui::theme::FontFamily::Mono)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Hover)))
    .with_corner_radius(8.0)
    .with_padding(EdgeInsets::new(12.0, 8.0, 12.0, 8.0))
    .finish();
    let transcript = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(16.0)
        .with_child(user_bubble(
            "Redesign the composer card: rounded segments, borderless controls.",
        ))
        .with_child(assistant(
            "The card holds two segments: a context row above and the input box below, \
             both on theme tokens. The lane caps the whole content column and centres \
             it, so a wide window keeps the conversation readable.",
        ))
        .with_child(tool_row)
        .with_child(assistant(
            "Let me look at how the lane resolves its width before I touch the layout.",
        ))
        .finish();
    let transcript = Padding::new(transcript, EdgeInsets::uniform(24.0)).finish();
    // The column fills the lane's height so the composer pins to its bottom,
    // exactly as the app's pane column does.
    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Expanded::new(transcript).finish())
        .with_child(composer_element(flags, false))
        .finish();
    goble_ui::elements::Container::new(column)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish()
}

fn render_lane(options: &Options, assets: &PreviewAssets) -> anyhow::Result<()> {
    let theme = options.theme()?;
    let mut app = AppContext::default();
    app.theme = theme;
    // The wide window shows the lane active; 800 is below
    // `CONTENT_LANE_MIN_ACTIVE_COLUMN`, so the narrow window is the plain
    // full-width pass-through (today's small-screen behaviour).
    let width = options.width.unwrap_or(1600.0);
    let height = options.height.unwrap_or(900.0);
    let scale = options.scale(1.0);
    let flags = MenuFlags::new();

    // The lane computes its own centred column: with no drag preference it is
    // `clamp(column * 0.64, 680, 920)` capped at `column - 176`, and the
    // gutters are where the draggable edges' 10px strips sit. The drag variant
    // paints the edge rule `ContentLane` draws on the dragged edge.
    let pane = lane_pane(&flags, &app);
    let lane = ContentLane::new(pane)
        .with_preference(None)
        .with_dragging(options.dragging)
        .with_drag_edge(options.dragging.then_some(options.drag_edge))
        .with_on_drag_start(|_pointer_x, _edge, _base_width, _column| {})
        .with_on_drag_move(|_pointer_x| {})
        .with_on_drag_end(|| {})
        .finish();
    let mut root: Box<dyn Element> = lane;
    raster::render_to_png(
        assets,
        &app,
        &mut root,
        vec2f(width, height),
        scale,
        None,
        &options.out,
    )
}
