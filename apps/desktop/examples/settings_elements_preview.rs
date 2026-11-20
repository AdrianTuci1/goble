//! Headless raster contact sheet for every settings primitive.
//!
//! The settings page is assembled from `goble_ui::elements::settings`; this
//! example lays those same elements out in a labelled grid so each state can be
//! inspected without the native window. It renders the real primitives through
//! the shared CPU rasterizer in both themes and at 1x and 2x:
//!
//! - `SettingsToggle` off/on/hover/focus/disabled;
//! - `SettingsSelect` closed/open/focus/disabled;
//! - `SettingsSegmented` with Queue / Send Immediately in both selected states;
//! - `SettingsButton` primary/secondary/destructive/ghost/icon in normal,
//!   hover and disabled states;
//! - `SettingsSlider`, `SettingsTextField`, `SettingsPathField`,
//!   `SettingsSecretField`;
//! - `SettingsNotice`, `SettingsStatusLine` and row statuses;
//! - a `SettingsSection` card with a heading, rows, dividers and a footer.
//!
//! Hover is injected the only way a headless paint allows: a tiny wrapper sets
//! `PaintContext::cursor_inside` and puts `cursor_position` at the child's
//! centre while that child paints, then restores the previous pointer. The
//! select's open panel is forced through the primitive's shared `with_open`
//! flag, the seam a page uses to keep a panel up across its per-frame rebuild.
//!
//! Usage: cargo run -p goble-app --example settings_elements_preview
//! Output: /tmp/goble-work/out/settings-elements/{dark,light}[-2x].png

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use goble_ui::elements::settings::fields::SettingsSegmented;
use goble_ui::elements::settings::slider::slider_with_output;
use goble_ui::elements::settings::status::{SettingsRule, SettingsStatusLine};
use goble_ui::elements::settings::{
    SettingsButton, SettingsNotice, SettingsPathField, SettingsRow, SettingsRowStatus,
    SettingsSecretField, SettingsSection, SettingsSelect, SettingsSlider, SettingsTextField,
    SettingsToggle,
};
use goble_ui::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, Flex,
    LayoutContext, MainAxisSize, PaintContext, Point, SizeConstraint, Text,
};
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::render::RenderCommand;
use goble_ui::theme::{ColorToken, Theme};

const OUT_DIR: &str = "/tmp/goble-work/out/settings-elements";
const CANVAS_WIDTH: f32 = 1220.0;
const PAGE_PADDING: f32 = 32.0;
const CELL_WIDTH: f32 = 220.0;
const NOTICE_CELL_WIDTH: f32 = 360.0;
const CELL_GAP: f32 = 14.0;
const GROUP_GAP: f32 = 26.0;

fn main() -> anyhow::Result<()> {
    let out_dir = PathBuf::from(OUT_DIR);
    std::fs::create_dir_all(&out_dir)?;
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/goble-ui/assets")
        .canonicalize()?;
    let assets = raster::PreviewAssets::load(&assets_dir)?;

    for (theme_name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let app = AppContext {
            theme,
            ..AppContext::default()
        };
        let mut root = build_scene(&app, theme_name);
        let height = scene_height(&mut root, &app);
        let canvas = vec2f(CANVAS_WIDTH, height);
        let commands = raster::render_commands(&mut root, canvas, &app);
        let texts = commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::DrawText { .. }))
            .count();
        let icons = commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::DrawIcon { .. }))
            .count();
        let strokes = commands
            .iter()
            .filter(|command| matches!(command, RenderCommand::StrokeRect { .. }))
            .count();
        for (scale_name, scale) in [("", 1.0_f32), ("-2x", 2.0_f32)] {
            let out = out_dir.join(format!("{theme_name}{scale_name}.png"));
            raster::rasterize_to_png(&assets, &app, &commands, canvas, scale, None, &out)?;
            println!(
                "{theme_name} · {scale}x · {:.0}x{:.0} logical · {} text · {} icons · {} strokes · {}",
                canvas.x,
                canvas.y,
                texts,
                icons,
                strokes,
                out.display()
            );
        }
    }

    println!(
        "note: SettingsSelect's open panel is forced with its shared with_open flag; SettingsToggle \
         and SettingsButton hover are pointer-injected. The primary button keeps its accent fill on \
         hover, so its hover cell differs only by the pointer being present."
    );
    Ok(())
}

fn scene_height(root: &mut Box<dyn Element>, app: &AppContext) -> f32 {
    let max = vec2f(CANVAS_WIDTH, 8000.0);
    let size = root.layout(
        SizeConstraint::loose(max),
        &mut LayoutContext::default(),
        app,
    );
    size.y.max(1.0)
}

fn build_scene(app: &AppContext, theme_name: &str) -> Box<dyn Element> {
    let mut groups: Vec<Box<dyn Element>> = vec![header(app, theme_name)];
    groups.push(group(
        app,
        "Toggle — SettingsToggle",
        "Rounded 32×18 capsule; hover lifts the track, focus draws the 2px ring, disabled refuses the press.",
        vec![row(vec![
            specimen(
                app,
                "off",
                Box::new(SettingsToggle::new(false)),
                "unchecked",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "on",
                Box::new(SettingsToggle::new(true)),
                "checked",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "hover",
                Box::new(CursorHover::new(Box::new(SettingsToggle::new(false)))),
                "pointer-injected hover",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "focus",
                Box::new(SettingsToggle::new(true).with_focused(true)),
                "2px Focus ring",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "disabled",
                Box::new(SettingsToggle::new(false).with_disabled(true)),
                "no press, muted pair",
                CELL_WIDTH,
            ),
        ])],
    ));
    groups.push(group(
        app,
        "Select — SettingsSelect",
        "Real dropdown: a closed trigger, a forced-open with_open panel, focus and disabled.",
        vec![
            row(vec![
                specimen(
                    app,
                    "closed",
                    Box::new(
                        SettingsSelect::new(["Default", "Turbo", "Read-only"])
                            .with_selected(0)
                            .with_width(210.0),
                    ),
                    "Default",
                    CELL_WIDTH,
                ),
                specimen(
                    app,
                    "focus",
                    Box::new(
                        SettingsSelect::new(["Default", "Turbo", "Read-only"])
                            .with_selected(1)
                            .with_focused(true)
                            .with_width(210.0),
                    ),
                    "2px Focus border",
                    CELL_WIDTH,
                ),
                specimen(
                    app,
                    "disabled",
                    Box::new(
                        SettingsSelect::new(["Default", "Turbo", "Read-only"])
                            .with_selected(0)
                            .with_disabled(true)
                            .with_width(210.0),
                    ),
                    "muted, refuses the click",
                    CELL_WIDTH,
                ),
            ]),
            open_select_specimen(app),
        ],
    ));
    groups.push(group(
        app,
        "Segmented — SettingsSegmented",
        "The Queue / Send Immediately pair in both selected states; the selected segment is the raised pill.",
        vec![row_with_width(
            vec![
                specimen(
                    app,
                    "Queue selected",
                    Box::new(
                        SettingsSegmented::new(["Queue", "Send Immediately"]).with_selected(0),
                    ),
                    "Queue selected",
                    360.0,
                ),
                specimen(
                    app,
                    "Send Immediately selected",
                    Box::new(
                        SettingsSegmented::new(["Queue", "Send Immediately"]).with_selected(1),
                    ),
                    "Send Immediately selected",
                    360.0,
                ),
            ],
            CELL_GAP,
        )],
    ));
    groups.push(group(
        app,
        "Buttons — SettingsButton",
        "Normal, pointer-injected hover, and disabled for each variant.",
        vec![
            state_row(app, "normal", button_cells(app, ButtonState::Normal)),
            state_row(app, "hover", button_cells(app, ButtonState::Hover)),
            state_row(app, "disabled", button_cells(app, ButtonState::Disabled)),
        ],
    ));
    groups.push(group(
        app,
        "Slider — SettingsSlider",
        "Track, filled accent, 14px knob; the output box is the page helper.",
        vec![row(vec![
            specimen(
                app,
                "0%",
                Box::new(SettingsSlider::new(0.0, 0.0, 100.0).with_width(190.0)),
                "at minimum",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "40%",
                Box::new(SettingsSlider::new(40.0, 0.0, 100.0).with_width(190.0)),
                "value 40",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "focus",
                Box::new(
                    SettingsSlider::new(40.0, 0.0, 100.0)
                        .with_width(190.0)
                        .with_focused(true),
                ),
                "2px Focus outline",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "disabled",
                Box::new(
                    SettingsSlider::new(40.0, 0.0, 100.0)
                        .with_width(190.0)
                        .with_disabled(true),
                ),
                "muted, refuses the press",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "with output",
                slider_with_output(
                    app,
                    Box::new(SettingsSlider::new(35.0, 0.0, 100.0).with_width(160.0)),
                    "35%",
                ),
                "slider_output helper",
                CELL_WIDTH,
            ),
        ])],
    ));
    groups.push(group(
        app,
        "Text & path fields — SettingsTextField / SettingsPathField",
        "The path field is the same 32px control with a path-shaped value.",
        vec![row(vec![
            specimen(
                app,
                "text",
                Box::new(
                    SettingsTextField::new()
                        .with_value("Ada")
                        .with_placeholder("Your name")
                        .with_width(220.0),
                ),
                "value",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "text focus",
                Box::new(
                    SettingsTextField::new()
                        .with_value("Ada")
                        .with_placeholder("Your name")
                        .with_focused(true)
                        .with_width(220.0),
                ),
                "Focus border",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "text disabled",
                Box::new(
                    SettingsTextField::new()
                        .with_value("Ada")
                        .with_disabled(true)
                        .with_width(220.0),
                ),
                "muted fill and border",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "path",
                Box::new(
                    SettingsPathField::new()
                        .with_value("~/.config/goble")
                        .with_width(220.0),
                ),
                "folder value",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "path disabled",
                Box::new(
                    SettingsPathField::new()
                        .with_value("~/.config/goble")
                        .with_disabled(true)
                        .with_width(220.0),
                ),
                "muted fill and border",
                CELL_WIDTH,
            ),
        ])],
    ));
    groups.push(group(
        app,
        "Secret field — SettingsSecretField",
        "Write-only: configured says so, a draft paints one dot per character.",
        vec![row(vec![
            specimen(
                app,
                "configured",
                Box::new(
                    SettingsSecretField::new()
                        .with_configured(true)
                        .with_width(220.0),
                ),
                "placeholder says configured",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "draft",
                Box::new(
                    SettingsSecretField::new()
                        .with_configured(true)
                        .with_draft("sk-live-1234")
                        .with_width(220.0),
                ),
                "one dot per character",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "draft focus",
                Box::new(
                    SettingsSecretField::new()
                        .with_configured(true)
                        .with_draft("sk-live-1234")
                        .with_focused(true)
                        .with_width(220.0),
                ),
                "masked, Focus border",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "not configured",
                Box::new(SettingsSecretField::new().with_width(220.0)),
                "paste affordance",
                CELL_WIDTH,
            ),
            specimen(
                app,
                "disabled",
                Box::new(
                    SettingsSecretField::new()
                        .with_configured(true)
                        .with_disabled(true)
                        .with_width(220.0),
                ),
                "muted, refuses input",
                CELL_WIDTH,
            ),
        ])],
    ));
    groups.push(group(
        app,
        "Notices & status — SettingsNotice / SettingsStatusLine",
        "Tinted status cards and the plain sentence a row or footer draws.",
        vec![
            row_with_width(
                vec![
                    specimen(
                        app,
                        "info",
                        Box::new(SettingsNotice::info("Config loaded from config.toml")),
                        "Info notice",
                        NOTICE_CELL_WIDTH,
                    ),
                    specimen(
                        app,
                        "success",
                        Box::new(SettingsNotice::success("Saved to config.toml.")),
                        "Success notice",
                        NOTICE_CELL_WIDTH,
                    ),
                    specimen(
                        app,
                        "warning",
                        Box::new(SettingsNotice::warning("File changed on disk. Reload.")),
                        "Warning notice",
                        NOTICE_CELL_WIDTH,
                    ),
                ],
                CELL_GAP,
            ),
            row_with_width(
                vec![
                    specimen(
                        app,
                        "error",
                        Box::new(SettingsNotice::error("Could not parse config.toml.")),
                        "Error notice",
                        NOTICE_CELL_WIDTH,
                    ),
                    specimen(
                        app,
                        "status line",
                        Box::new(SettingsStatusLine::unavailable()),
                        "Not available in this build",
                        NOTICE_CELL_WIDTH,
                    ),
                ],
                CELL_GAP,
            ),
        ],
    ));
    groups.push(group(
        app,
        "Section card — SettingsSection / SettingsRow",
        "One card: heading, description, rows, a 1px rule between rows, a rule above the footer.",
        vec![settings_section(app)],
    ));

    let content = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(GROUP_GAP)
        .with_children(groups)
        .finish();
    Container::new(content)
        .with_padding_uniform(PAGE_PADDING)
        .finish()
}

fn header(app: &AppContext, theme_name: &str) -> Box<dyn Element> {
    let title = Text::new(format!("Settings elements — {theme_name}"))
        .with_font_size(26.0)
        .with_line_height(1.2)
        .with_weight(FontWeight::SemiBold)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let subtitle = Text::new(
        "goble_ui::elements::settings · real primitives through the shared CPU rasterizer · 1x and 2x",
    )
    .with_font_size(12.0)
    .with_line_height(1.4)
    .with_color(app.theme.color(ColorToken::Muted))
    .finish();
    let legend = Text::new(
        "Hover injects PaintContext's pointer at each control's centre; the open select uses the shared with_open flag.",
    )
    .with_font_size(11.0)
    .with_line_height(1.4)
    .with_color(app.theme.color(ColorToken::Muted))
    .finish();
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(6.0)
        .with_children(vec![title, subtitle, legend])
        .finish()
}

fn group(
    app: &AppContext,
    title: &str,
    description: &str,
    rows: Vec<Box<dyn Element>>,
) -> Box<dyn Element> {
    let heading = Text::new(title.to_string())
        .with_font_size(14.0)
        .with_line_height(1.3)
        .with_weight(FontWeight::SemiBold)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let description = Text::new(description.to_string())
        .with_font_size(11.0)
        .with_line_height(1.4)
        .with_color(app.theme.color(ColorToken::Muted))
        .finish();
    let mut children: Vec<Box<dyn Element>> =
        vec![Box::new(SettingsRule::new()), heading, description];
    children.extend(rows);
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(10.0)
        .with_children(children)
        .finish()
}

fn specimen(
    app: &AppContext,
    label: &str,
    control: Box<dyn Element>,
    note: &str,
    width: f32,
) -> Box<dyn Element> {
    let label = Text::new(label.to_string())
        .with_font_size(11.0)
        .with_line_height(1.3)
        .with_weight(FontWeight::Medium)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let note = Text::new(note.to_string())
        .with_font_size(10.0)
        .with_line_height(1.4)
        .with_color(app.theme.color(ColorToken::Muted))
        .finish();
    let control = Container::new(control)
        .with_padding(EdgeInsets::new(0.0, 8.0, 0.0, 0.0))
        .finish();
    let column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(5.0)
        .with_children(vec![label, control, note])
        .finish();
    ConstrainedBox::new(column).with_width(width).finish()
}

fn row(cells: Vec<Box<dyn Element>>) -> Box<dyn Element> {
    row_with_width(cells, CELL_GAP)
}

fn row_with_width(cells: Vec<Box<dyn Element>>, gap: f32) -> Box<dyn Element> {
    Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(gap)
        .with_children(cells)
        .finish()
}

fn state_row(app: &AppContext, state: &str, cells: Vec<Box<dyn Element>>) -> Box<dyn Element> {
    let label = Text::new(state.to_string())
        .with_font_size(12.0)
        .with_line_height(1.3)
        .with_weight(FontWeight::SemiBold)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(8.0)
        .with_children(vec![label, row(cells)])
        .finish()
}

#[derive(Clone, Copy)]
enum ButtonState {
    Normal,
    Hover,
    Disabled,
}

fn button_cells(app: &AppContext, state: ButtonState) -> Vec<Box<dyn Element>> {
    let kinds: [(&str, fn() -> SettingsButton); 5] = [
        ("primary", || SettingsButton::new("Primary").primary()),
        ("secondary", || SettingsButton::new("Secondary")),
        ("destructive", || {
            SettingsButton::new("Delete").destructive()
        }),
        ("ghost", || SettingsButton::new("Ghost").ghost()),
        ("icon", || SettingsButton::new("").with_icon("plus")),
    ];
    kinds
        .into_iter()
        .map(|(label, build)| {
            let button = match state {
                ButtonState::Disabled => build().with_disabled(true),
                _ => build(),
            };
            let control: Box<dyn Element> = match state {
                ButtonState::Hover => Box::new(CursorHover::new(Box::new(button))),
                _ => Box::new(button),
            };
            let note = match (state, label) {
                (ButtonState::Normal, "icon") => "icon-only",
                (ButtonState::Normal, "primary") => "accent fill",
                (ButtonState::Normal, "secondary") => "raised fill + hairline",
                (ButtonState::Normal, "destructive") => "Error text, default fill",
                (ButtonState::Normal, "ghost") => "low emphasis",
                (ButtonState::Hover, "primary") => "accent fill unchanged",
                (ButtonState::Hover, _) => "pointer injected",
                (ButtonState::Disabled, _) => "muted, refuses click",
                _ => "",
            };
            specimen(app, label, control, note, CELL_WIDTH)
        })
        .collect()
}

fn open_select_specimen(app: &AppContext) -> Box<dyn Element> {
    let label = Text::new("open".to_string())
        .with_font_size(11.0)
        .with_line_height(1.3)
        .with_weight(FontWeight::Medium)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let note = Text::new("with_open(true) — real PopupMenu panel".to_string())
        .with_font_size(10.0)
        .with_line_height(1.4)
        .with_color(app.theme.color(ColorToken::Muted))
        .finish();
    let open = Rc::new(RefCell::new(true));
    let select: Box<dyn Element> = Box::new(
        SettingsSelect::new(["Default", "Turbo", "Read-only"])
            .with_selected(0)
            .with_open(open)
            .with_width(240.0),
    );
    let slot = FixedSlot::new(select, 170.0);
    let column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(5.0)
        .with_children(vec![label, note, Box::new(slot)])
        .finish();
    ConstrainedBox::new(column).with_width(360.0).finish()
}

fn settings_section(app: &AppContext) -> Box<dyn Element> {
    let friendly =
        SettingsRow::new(app, "Friendly name", "How Goble addresses you.").with_control(Box::new(
            SettingsTextField::new()
                .with_value("Ada")
                .with_placeholder("Your name")
                .with_width(240.0),
        ));
    let queue = SettingsRow::new(
        app,
        "Send immediately",
        "Queue follow-up messages or send them as soon as the turn ends.",
    )
    .with_control(Box::new(SettingsToggle::new(true)));
    let model =
        SettingsRow::new(app, "Model", "The model new sessions start on.").with_control(Box::new(
            SettingsSelect::new(["DeepSeek V4 Flash", "Local Qwen"])
                .with_selected(0)
                .with_width(240.0),
        ));
    let voice = SettingsRow::new(app, "Voice", "The voice used in Voice mode.")
        .with_disabled(true)
        .with_status(SettingsRowStatus::unavailable(
            "Voice mode is not available in this build.",
        ))
        .with_control(Box::new(SettingsToggle::new(false).with_disabled(true)));
    let saved = SettingsRow::new(app, "Config file", "Where the settings are stored.")
        .with_control(Box::new(SettingsButton::new("Reveal")))
        .with_status(SettingsRowStatus::success(
            "Saved to ~/.config/goble/config.toml.",
        ));
    let footer = Container::new(Box::new(SettingsStatusLine::unavailable()) as Box<dyn Element>)
        .with_padding(EdgeInsets::new(16.0, 12.0, 16.0, 12.0))
        .finish();
    Box::new(
        SettingsSection::new("General")
            .with_description(
                "A section card: a heading, a description, four rows and a footer, with the 1px rules drawn between them.",
            )
            .with_row(Box::new(friendly))
            .with_row(Box::new(queue))
            .with_row(Box::new(model))
            .with_row(Box::new(voice))
            .with_row(Box::new(saved))
            .with_footer(footer),
    )
}

/// Reserves vertical room for a child that paints past its layout box (an open
/// select's popup panel), so the contact sheet's next group starts below it.
struct FixedSlot {
    child: Box<dyn Element>,
    height: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl FixedSlot {
    fn new(child: Box<dyn Element>, height: f32) -> Self {
        Self {
            child,
            height,
            size: None,
            origin: None,
        }
    }
}

impl Element for FixedSlot {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let child = self.child.layout(constraint, ctx, app);
        let size = vec2f(child.x, child.y.max(self.height));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// Paints `child` with the paint-time pointer parked at its centre, which is
/// how a headless scene shows the hover a real window would show. The previous
/// pointer state is restored so the wrapper is composable in a grid.
struct CursorHover {
    child: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl CursorHover {
    fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            size: None,
            origin: None,
        }
    }
}

impl Element for CursorHover {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let previous_inside = ctx.cursor_inside;
        let previous_position = ctx.cursor_position;
        if let Some(size) = self.size {
            ctx.cursor_inside = true;
            ctx.cursor_position = vec2f(origin.x + size.x / 2.0, origin.y + size.y / 2.0);
        }
        self.child.paint(origin, ctx, app);
        ctx.cursor_inside = previous_inside;
        ctx.cursor_position = previous_position;
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
