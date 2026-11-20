//! The changed-files card that closes an assistant turn.
//!
//! The card follows DSH's `ChangedFiles.module.css`: a 60 pt filled header with
//! a 40 pt file tile, a 13/20 title, a 10/16 mono stat line and a Review
//! action; file rows with status glyphs, a 12 pt path and 11/18 mono counts;
//! and the fold after four rows with an explicit expand control. The counts
//! come from the turn's own recorded tool diffs ([`ChangedFiles`]), so a turn
//! that changed nothing draws no card at all.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{ChangedFile, ChangedFiles};
use crate::elements::chat_typography;
use crate::elements::diff::DiffSpec;
use crate::elements::file_icon_name;
use crate::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Expanded, Fill, Flex, HoverRow, Icon, MainAxisAlignment, MainAxisSize, Text,
};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily};

pub(super) fn build_changed_files_card(
    app: &AppContext,
    summary: &ChangedFiles,
    expanded: Rc<RefCell<bool>>,
    on_review: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let open = *expanded.borrow();
    let files = summary.files_changed();
    let single = files == 1;
    let foldable = files > DiffSpec::CHANGED_FILES_FOLD;
    // A one-file turn is titled by the file itself, not by a count: DSH's
    // card names the change the reader is about to open. Multi-file turns
    // keep the summary count.
    let title = if single {
        summary.files[0]
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&summary.files[0].path)
            .to_string()
    } else {
        format!("{files} files changed")
    };
    let header = build_header(app, &title, summary, on_review.clone());

    let mut card = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header);

    // A single changed file stays a one-line card until it is opened; two to
    // four files show every row; more than four fold at DSH's fourth row.
    let visible = if single && !open {
        0
    } else if foldable && !open {
        DiffSpec::CHANGED_FILES_FOLD
    } else {
        summary.files.len()
    };
    if visible > 0 {
        card = card.with_child(
            Divider::horizontal()
                .with_thickness(DiffSpec::CHANGED_FILES_HAIRLINE)
                .finish(),
        );
        for file in summary.files.iter().take(visible) {
            card = card.with_child(file_row(app, file, on_review.clone()));
        }
    }
    if foldable {
        card = card.with_child(fold_toggle(app, expanded.clone(), files, open));
    }

    Container::new(card.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(
            Border::all(DiffSpec::CHANGED_FILES_HAIRLINE)
                .with_border_color(app.theme.color(ColorToken::Border)),
        )
        .with_corner_radius(DiffSpec::CHANGED_FILES_RADIUS)
        .finish()
}

fn build_header(
    app: &AppContext,
    title: &str,
    summary: &ChangedFiles,
    on_review: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let single = summary.files_changed() == 1;
    let icon = if single {
        file_icon_name(&summary.files[0].path)
    } else {
        "code"
    };
    let tile = Container::new(
        Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Icon::new(icon)
                    .with_size(DiffSpec::CHANGED_FILES_TILE_GLYPH)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(
        Border::all(DiffSpec::CHANGED_FILES_HAIRLINE)
            .with_border_color(app.theme.color(ColorToken::Border)),
    )
    .with_padding(EdgeInsets::uniform(
        (DiffSpec::CHANGED_FILES_TILE - DiffSpec::CHANGED_FILES_TILE_GLYPH) * 0.5,
    ))
    .with_corner_radius(DiffSpec::CHANGED_FILES_TILE_RADIUS)
    .finish();
    let tile = ConstrainedBox::new(tile)
        .with_width(DiffSpec::CHANGED_FILES_TILE)
        .with_height(DiffSpec::CHANGED_FILES_TILE)
        .finish();

    let title = Text::new(title)
        .with_font_family(chat_typography::chat_font(app))
        .with_font_size(DiffSpec::CHANGED_FILES_TITLE_SIZE)
        .with_line_height(
            DiffSpec::CHANGED_FILES_TITLE_LINE_HEIGHT / DiffSpec::CHANGED_FILES_TITLE_SIZE,
        )
        .with_weight(FontWeight::Medium)
        .with_theme_color(ColorToken::Text, app)
        .with_max_lines(1)
        .finish();
    let stat = counts(
        summary.added(),
        summary.removed(),
        DiffSpec::CHANGED_FILES_STAT_SIZE,
        DiffSpec::CHANGED_FILES_STAT_LINE_HEIGHT,
        DiffSpec::CHANGED_FILES_STAT_GAP,
        app,
    );
    let titles = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(title)
        .with_child(stat)
        .finish();

    let identity = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(DiffSpec::CHANGED_FILES_HEADER_GAP)
        .with_child(tile)
        .with_child(Expanded::new(titles).finish())
        .finish();
    let identity = {
        let on_review = on_review.clone();
        HoverRow::new(identity)
            .with_quiet_hover()
            .with_on_click(move || (on_review.borrow_mut())())
            .finish()
    };

    let content = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(DiffSpec::CHANGED_FILES_HEADER_GAP)
        .with_child(Expanded::new(identity).finish())
        .with_child(review_button(app, on_review))
        .finish();

    ConstrainedBox::new(
        Container::new(content)
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_padding(EdgeInsets::new(
                // The tile is 40 pt inside the 60 pt header: the extra 2 pt
                // above and below DSH's 8 pt padding keeps the fill 60 tall.
                (DiffSpec::CHANGED_FILES_HEADER_HEIGHT - DiffSpec::CHANGED_FILES_TILE) * 0.5,
                DiffSpec::CHANGED_FILES_HEADER_PAD_X,
                (DiffSpec::CHANGED_FILES_HEADER_HEIGHT - DiffSpec::CHANGED_FILES_TILE) * 0.5,
                DiffSpec::CHANGED_FILES_HEADER_PAD_X,
            ))
            .finish(),
    )
    .with_height(DiffSpec::CHANGED_FILES_HEADER_HEIGHT)
    .finish()
}

fn review_button(app: &AppContext, on_review: Rc<RefCell<dyn FnMut()>>) -> Box<dyn Element> {
    let content = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new("file")
                .with_size(14.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new("Review")
                .with_font_family(chat_typography::chat_font(app))
                .with_font_size(12.0)
                .with_line_height(18.0 / 12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    HoverRow::new(content)
        .with_padding(EdgeInsets::new(5.0, 10.0, 5.0, 10.0))
        .with_corner_radius(8.0)
        .with_hover_token(ColorToken::Selected)
        .with_on_click(move || (on_review.borrow_mut())())
        .finish()
}

fn file_row(
    app: &AppContext,
    file: &ChangedFile,
    on_review: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let (name, dir) = match file.path.rsplit_once('/') {
        Some((dir, name)) => (name.to_string(), dir.to_string()),
        None => (file.path.clone(), String::new()),
    };
    let (status_color, status_ink) = match file.status {
        'A' => (ColorToken::Success, "A"),
        'D' => (ColorToken::Error, "D"),
        _ => (ColorToken::Muted, "M"),
    };
    let status = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Icon::new(file_icon_name(&file.path))
                .with_size(DiffSpec::CHANGED_FILES_STATUS_GLYPH)
                .with_theme_color(status_color, app)
                .finish(),
        )
        .with_child(
            Text::new(status_ink)
                .with_font_family(FontFamily::Mono)
                .with_font_size(9.0)
                .with_line_height(14.0 / 9.0)
                .with_theme_color(status_color, app)
                .finish(),
        )
        .finish();

    let mut path = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Text::new(name)
                .with_font_family(chat_typography::chat_font(app))
                .with_font_size(DiffSpec::CHANGED_FILES_PATH_SIZE)
                .with_line_height(
                    DiffSpec::CHANGED_FILES_PATH_LINE_HEIGHT / DiffSpec::CHANGED_FILES_PATH_SIZE,
                )
                .with_weight(FontWeight::Medium)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        );
    if !dir.is_empty() {
        path = path.with_child(
            Text::new(dir)
                .with_font_family(chat_typography::chat_font(app))
                .with_font_size(DiffSpec::CHANGED_FILES_PATH_SIZE)
                .with_line_height(
                    DiffSpec::CHANGED_FILES_PATH_LINE_HEIGHT / DiffSpec::CHANGED_FILES_PATH_SIZE,
                )
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        );
    }
    let identity = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(status)
        .with_child(Expanded::new(path.finish()).finish())
        .finish();
    let counts = counts(
        file.added,
        file.removed,
        DiffSpec::CHANGED_FILES_COUNT_SIZE,
        DiffSpec::CHANGED_FILES_COUNT_LINE_HEIGHT,
        DiffSpec::CHANGED_FILES_ROW_GAP,
        app,
    );
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(DiffSpec::CHANGED_FILES_ROW_GAP)
        .with_child(Expanded::new(identity).finish())
        .with_child(counts)
        .finish();
    HoverRow::new(
        Container::new(row)
            .with_padding(EdgeInsets::new(
                DiffSpec::CHANGED_FILES_ROW_PAD_TOP,
                DiffSpec::CHANGED_FILES_ROW_PAD_RIGHT,
                DiffSpec::CHANGED_FILES_ROW_PAD_BOTTOM,
                DiffSpec::CHANGED_FILES_ROW_PAD_LEFT,
            ))
            .finish(),
    )
    .with_padding(EdgeInsets::default())
    .with_on_click(move || (on_review.borrow_mut())())
    .finish()
}

fn fold_toggle(
    app: &AppContext,
    expanded: Rc<RefCell<bool>>,
    files: usize,
    open: bool,
) -> Box<dyn Element> {
    let label = if open {
        "Show fewer".to_string()
    } else {
        format!("Show all {files} files")
    };
    let chevron = if open { "chevron-up" } else { "chevron-down" };
    let content = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Text::new(label)
                .with_font_family(chat_typography::chat_font(app))
                .with_font_size(DiffSpec::CHANGED_FILES_FOLD_SIZE)
                .with_line_height(
                    DiffSpec::CHANGED_FILES_FOLD_LINE_HEIGHT / DiffSpec::CHANGED_FILES_FOLD_SIZE,
                )
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Icon::new(chevron)
                .with_size(DiffSpec::CHANGED_FILES_FOLD_CHEVRON)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    let toggle = {
        let expanded = expanded.clone();
        move || {
            let next = !*expanded.borrow();
            *expanded.borrow_mut() = next;
        }
    };
    HoverRow::new(
        Container::new(content)
            .with_padding(EdgeInsets::new(
                DiffSpec::CHANGED_FILES_FOLD_PAD_TOP,
                DiffSpec::CHANGED_FILES_FOLD_PAD_RIGHT,
                DiffSpec::CHANGED_FILES_FOLD_PAD_BOTTOM,
                DiffSpec::CHANGED_FILES_FOLD_PAD_LEFT,
            ))
            .finish(),
    )
    .with_padding(EdgeInsets::default())
    .with_on_click(toggle)
    .finish()
}

/// A one-line `+A -D` pair at the size a surface asks for. A category with no
/// lines is left out — the card never prints `+0` or `-0` — and a change with
/// neither side prints a muted `0` so the line keeps its height.
fn counts(
    added: u64,
    removed: u64,
    font_size: f32,
    line_height: f32,
    gap: f32,
    app: &AppContext,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(gap);
    if added > 0 {
        row = row.with_child(count_text(
            format!("+{}", grouped(added)),
            ColorToken::Success,
            font_size,
            line_height,
            app,
        ));
    }
    if removed > 0 {
        row = row.with_child(count_text(
            format!("-{}", grouped(removed)),
            ColorToken::Error,
            font_size,
            line_height,
            app,
        ));
    }
    if added == 0 && removed == 0 {
        row = row.with_child(count_text(
            "0".to_string(),
            ColorToken::Muted,
            font_size,
            line_height,
            app,
        ));
    }
    row.finish()
}

fn count_text(
    text: String,
    token: ColorToken,
    font_size: f32,
    line_height: f32,
    app: &AppContext,
) -> Box<dyn Element> {
    Text::new(text)
        .with_font_family(FontFamily::Mono)
        .with_font_size(font_size)
        .with_line_height(line_height / font_size)
        .with_theme_color(token, app)
        .finish()
}

fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_like_dsh() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_234), "1,234");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
