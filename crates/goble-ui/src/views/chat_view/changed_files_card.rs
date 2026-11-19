//! The changed-files card that closes an assistant turn, in DSH's shape: one
//! 60pt header line with the file count, added/deleted totals, a disclosure and
//! the Review action, and — opened — the per-file rows with their own counts.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::ChangedFiles;
use crate::elements::chat_typography;
use crate::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element,
    Expanded, Fill, Flex, HoverRow, Icon, MainAxisSize, Text,
};
use crate::geometry::vec2f;
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily};

/// DSH's `ChangedFiles` rows shown before the card folds the rest away.
const COLLAPSED_ROWS: usize = 4;

/// Build the turn's changed-files card. `expanded` is app-owned so the fold
/// survives the per-frame rebuild; `on_toggle` flips it and `on_review` opens
/// the existing review pane.
pub(super) fn build_changed_files_card(
    app: &AppContext,
    summary: &ChangedFiles,
    expanded: Rc<RefCell<bool>>,
    on_review: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let open = *expanded.borrow();
    let files = summary.files_changed();
    let title = if files == 1 {
        "1 file changed".to_string()
    } else {
        format!("{files} files changed")
    };
    let counts = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(count_span(
            format!("+{}", summary.added()),
            ColorToken::Success,
            app,
        ))
        .with_child(count_span(
            format!("-{}", summary.removed()),
            ColorToken::Error,
            app,
        ))
        .finish();
    let chevron = Icon::new(if open {
        "chevron-down"
    } else {
        "chevron-right"
    })
    .with_size(12.0)
    .with_theme_color(ColorToken::Muted, app)
    .finish();
    let title = Text::new(title)
        .with_font_family(chat_typography::chat_font(app))
        .with_font_size(13.0)
        .with_line_height(20.0 / 13.0)
        .with_weight(FontWeight::Medium)
        .with_theme_color(ColorToken::Text, app)
        .finish();
    let header_left = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(title)
        .with_child(counts)
        .with_child(chevron)
        .finish();

    let on_toggle = {
        let expanded = expanded.clone();
        move || *expanded.borrow_mut() = !*expanded.borrow()
    };
    let left = HoverRow::new(header_left)
        .with_quiet_hover()
        .with_padding(EdgeInsets::new(7.0, 0.0, 7.0, 14.0))
        .with_on_click(on_toggle)
        .finish();

    let on_review_click = on_review.clone();
    let review = HoverRow::new(
        Flex::row()
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
                    .with_line_height(20.0 / 12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::new(6.0, 2.0, 6.0, 2.0))
    .with_corner_radius(8.0)
    .with_on_click(move || (on_review_click.borrow_mut())())
    .finish();

    let header = Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Expanded::new(left).finish())
            .with_child(review)
            .finish(),
    )
    .with_padding(EdgeInsets::new(0.0, 8.0, 0.0, 8.0))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
    .with_corner_radius(12.0)
    .finish();
    let header = ConstrainedBox::new(header).with_height(60.0).finish();

    let mut card = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header);
    if open {
        let mut rows = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(0.0);
        let shown = summary.files.iter().take(COLLAPSED_ROWS);
        for file in shown {
            rows = rows.with_child(file_row(app, file));
        }
        if summary.files.len() > COLLAPSED_ROWS {
            rows = rows.with_child(
                Container::new(
                    Text::new(format!(
                        "{} more files",
                        summary.files.len() - COLLAPSED_ROWS
                    ))
                    .with_font_family(chat_typography::chat_font(app))
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
                )
                .with_padding(EdgeInsets::new(7.0, 14.0, 7.0, 14.0))
                .finish(),
            );
        }
        card = card.with_child(
            Container::new(rows.finish())
                .with_padding(EdgeInsets::new(0.0, 8.0, 6.0, 8.0))
                .finish(),
        );
    }
    card.finish()
}

/// DSH's file row: `7px 18px 7px 14px`, path at 12 and counts at 11/18 mono.
fn file_row(
    app: &AppContext,
    file: &crate::elements::chat_content::ChangedFile,
) -> Box<dyn Element> {
    let counts = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(row_count_span(
            format!("+{}", file.added),
            ColorToken::Success,
            app,
        ))
        .with_child(row_count_span(
            format!("-{}", file.removed),
            ColorToken::Error,
            app,
        ))
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(12.0)
        .with_child(
            Expanded::new(
                Text::new(file.path.clone())
                    .with_font_family(chat_typography::chat_font(app))
                    .with_font_size(12.0)
                    .with_line_height(20.0 / 12.0)
                    .with_theme_color(ColorToken::Text, app)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish(),
        )
        .with_child(counts)
        .finish();
    Container::new(row)
        .with_padding(EdgeInsets::new(7.0, 18.0, 7.0, 14.0))
        .finish()
}

fn row_count_span(text: String, token: ColorToken, app: &AppContext) -> Box<dyn Element> {
    Text::new(text)
        .with_font_family(FontFamily::Mono)
        .with_font_size(11.0)
        .with_line_height(18.0 / 11.0)
        .with_theme_color(token, app)
        .finish()
}

fn count_span(text: String, token: ColorToken, app: &AppContext) -> Box<dyn Element> {
    Text::new(text)
        .with_font_family(FontFamily::Mono)
        .with_font_size(10.0)
        .with_line_height(16.0 / 10.0)
        .with_theme_color(token, app)
        .finish()
}
