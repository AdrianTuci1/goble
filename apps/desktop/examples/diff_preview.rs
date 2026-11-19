//! Headless PNG preview of the DSH-style file review: the 38pt header with a
//! file selector and comparison tools, the 60pt changed-file card header, and
//! the 22pt-line unified diff with its two number gutters and add/delete marks.
//!
//! The live review pane is built from app state; this example draws its
//! anatomy over the same `goble-ui` elements (the shared `Diff` row renderer)
//! so the metrics can be inspected without a window.

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use goble_ui::elements::diff::{DiffLine, DiffLineKind, Hunk};
use goble_ui::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, Diff, EdgeInsets, Element,
    Expanded, Fill, Flex, Icon, MainAxisAlignment, MainAxisSize, Padding, Text, TopbarButton,
};
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, FontFamily, Theme};

fn hunk(lines: Vec<(DiffLineKind, Option<u32>, Option<u32>, &str)>) -> Hunk {
    Hunk {
        old_start: 40,
        old_count: 18,
        new_start: 40,
        new_count: 20,
        section: "fn build()".to_string(),
        lines: lines
            .into_iter()
            .map(|(kind, old_line, new_line, text)| DiffLine {
                kind,
                old_line,
                new_line,
                text: text.to_string(),
            })
            .collect(),
    }
}

fn scene(app: &AppContext) -> Box<dyn Element> {
    let border = app.theme.color(ColorToken::Border);
    let muted = app.theme.color(ColorToken::Muted);
    let text = app.theme.color(ColorToken::Text);

    let selector = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0)
            .with_child(
                Text::new("src/panes.rs")
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(
                Icon::new("chevron-down")
                    .with_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::new(8.0, 0.0, 6.0, 0.0))
    .with_corner_radius(6.0)
    .finish();
    let selector = ConstrainedBox::new(selector).with_height(28.0).finish();
    let mut tools = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(2.0);
    for (glyph, active) in [
        ("layers-three-01", false),
        ("menu-01", false),
        ("maximize-01", false),
    ] {
        tools = tools.with_child(
            Container::new(
                Icon::new(glyph)
                    .with_size(15.0)
                    .with_theme_color(if active { ColorToken::Text } else { ColorToken::Muted }, app)
                    .finish(),
            )
            .with_padding(EdgeInsets::uniform(6.5))
            .with_corner_radius(6.0)
            .with_background(Fill::Solid(app.theme.color(if active {
                ColorToken::Hover
            } else {
                ColorToken::Surface
            })))
            .finish(),
        );
    }
    let counts = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Text::new("+18")
                .with_font_family(FontFamily::Mono)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Success, app)
                .finish(),
        )
        .with_child(
            Text::new("-4")
                .with_font_family(FontFamily::Mono)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Error, app)
                .finish(),
        )
        .finish();
    let header = Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(selector)
            .with_child(counts)
            .with_child(Expanded::new(
                Flex::row().with_main_axis_size(MainAxisSize::Max).finish(),
            ).finish())
            .with_child(tools.finish())
            .finish(),
    )
    .with_padding(EdgeInsets::new(8.0, 0.0, 6.0, 0.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish();
    let header = ConstrainedBox::new(header).with_height(38.0).finish();

    let tile = Container::new(
        Icon::new("file")
            .with_size(20.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(10.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(Border::all(1.0).with_border_color(border))
    .with_corner_radius(10.0)
    .finish();
    let card = Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(10.0)
            .with_child(tile)
            .with_child(
                Flex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                    .with_spacing(2.0)
                    .with_child(
                        Text::new("src/panes.rs")
                            .with_font_size(13.0)
                            .with_line_height(20.0 / 13.0)
                            .with_weight(FontWeight::Medium)
                            .with_theme_color(ColorToken::Text, app)
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(6.0)
                            .with_child(
                                Text::new("+18")
                                    .with_font_family(FontFamily::Mono)
                                    .with_font_size(10.0)
                                    .with_line_height(16.0 / 10.0)
                                    .with_theme_color(ColorToken::Success, app)
                                    .finish(),
                            )
                            .with_child(
                                Text::new("-4")
                                    .with_font_family(FontFamily::Mono)
                                    .with_font_size(10.0)
                                    .with_line_height(16.0 / 10.0)
                                    .with_theme_color(ColorToken::Error, app)
                                    .finish(),
                            )
                            .finish(),
                    )
                    .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::new(10.0, 8.0, 10.0, 8.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(Border::all(1.0).with_border_color(border))
    .with_corner_radius(12.0)
    .finish();
    let card = ConstrainedBox::new(card).with_height(60.0).finish();

    let hunks = vec![hunk(vec![
        (DiffLineKind::Context, Some(40), Some(40), "    let panes = build_panes(state);"),
        (DiffLineKind::Context, Some(41), Some(41), "    for pane in panes {"),
        (DiffLineKind::Removed, Some(42), None, "        draw_old(pane);"),
        (DiffLineKind::Added, None, Some(42), "        draw_new(pane);"),
        (DiffLineKind::Added, None, Some(43), "        mark_visible(pane);"),
        (DiffLineKind::Context, Some(43), Some(44), "    }"),
        (DiffLineKind::Removed, Some(44), None, "    old_footer();"),
        (DiffLineKind::Added, None, Some(45), "    new_footer();"),
        (DiffLineKind::Context, Some(45), Some(46), "}"),
    ])];
    let diff = Diff::new(hunks)
        .with_line_numbers(true)
        .with_fill_width(true)
        .with_viewport(Rc::new(std::cell::RefCell::new(
            goble_ui::ScrollState::default(),
        )));
    let diff: Box<dyn Element> = Box::new(diff);
    let diff = Container::new(diff)
        .with_padding(EdgeInsets::new(8.0, 8.0, 16.0, 8.0))
        .finish();
    let body = Container::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header)
            .with_child(card)
            .with_child(Expanded::new(diff).finish())
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish();
    let _ = (muted, text);
    body
}

/// The empty state the review pane draws when the working tree has no changes.
fn empty_scene(app: &AppContext) -> Box<dyn Element> {
    Container::new(
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_child(
                Icon::new("file")
                    .with_size(24.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Padding::new(
                    Text::new("No open changes")
                        .with_font_size(13.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                    EdgeInsets::new(0.0, 8.0, 0.0, 0.0),
                )
                .finish(),
            )
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish()
}

fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from("/tmp/goble-work/out");
    std::fs::create_dir_all(&dir)?;
    let assets_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/goble-ui/assets")
        .canonicalize()?;
    let assets = raster::PreviewAssets::load(&assets_dir)?;
    for (label, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let app = AppContext {
            theme,
            ..AppContext::default()
        };
        let mut root = scene(&app);
        let size: Vector2F = vec2f(900.0, 420.0);
        let path = dir.join(format!("diff-preview-{label}.png"));
        raster::render_element_to_png(&assets, &app, &mut root, size.x, size.y, 2.0, None, &path)?;
        println!("wrote {}", path.display());
        let mut empty = empty_scene(&app);
        let empty_path = dir.join(format!("diff-empty-{label}.png"));
        raster::render_element_to_png(
            &assets,
            &app,
            &mut empty,
            size.x,
            size.y,
            2.0,
            None,
            &empty_path,
        )?;
        println!("wrote {}", empty_path.display());
    }
    Ok(())
}
