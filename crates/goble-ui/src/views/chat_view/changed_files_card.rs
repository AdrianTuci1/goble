//! The changed-files card that closes an assistant turn.
//!
//! S/T: the simpler T3 model — a single bordered card on the chat's own
//! background. The header says "N files changed", the green/red counts and a
//! chevron; a Review button sits at the right. Expanding shows one row per
//! file: type icon, bold filename, muted full directory path and the counts
//! flush right. Only the path shrinks on narrow widths; it loses characters
//! from the left and keeps a leading ellipsis so the tail stays readable.

use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::chat_content::{ChangedFile, ChangedFiles};
use crate::elements::chat_typography;
use crate::elements::file_icon_name;
use crate::elements::{
    AppContext, Border, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Expanded,
    Fill, Flex, HoverRow, Icon, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext,
    Point, SizeConstraint, Text,
};
use crate::geometry::{vec2f, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily};

const CARD_RADIUS: f32 = 12.0;
const HAIRLINE: f32 = 1.0;
const TITLE_SIZE: f32 = 15.0;
const ROW_SIZE: f32 = 13.0;
const ROW_LINE: f32 = 20.0;
const ROW_PAD_X: f32 = 14.0;
const ROW_PAD_Y: f32 = 9.0;
const HEADER_PAD_Y: f32 = 11.0;

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
    let header = build_header(
        app,
        &title,
        summary,
        Rc::clone(&expanded),
        on_review.clone(),
    );

    let mut card = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header);
    if open {
        card = card.with_child(Divider::horizontal().with_thickness(HAIRLINE).finish());
        for (index, file) in summary.files.iter().enumerate() {
            if index > 0 {
                card = card.with_child(Divider::horizontal().with_thickness(HAIRLINE).finish());
            }
            card = card.with_child(file_row(app, file, on_review.clone()));
        }
    }

    // Same background as the chat (no raised fill): only a thin border and
    // the photo's radius give the card its shape.
    Container::new(card.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .with_border(Border::all(HAIRLINE).with_border_color(app.theme.color(ColorToken::Border)))
        .with_corner_radius(CARD_RADIUS)
        .finish()
}

fn build_header(
    app: &AppContext,
    title: &str,
    summary: &ChangedFiles,
    expanded: Rc<RefCell<bool>>,
    on_review: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let open = *expanded.borrow();
    let stats = counts(
        summary.added(),
        summary.removed(),
        ROW_SIZE,
        ROW_LINE,
        6.0,
        app,
    );
    let chevron = Icon::new(if open {
        "chevron-down"
    } else {
        "chevron-right"
    })
    .with_size(13.0)
    .with_theme_color(ColorToken::Muted, app)
    .finish();
    let left = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(
            Text::new(title)
                .with_font_family(chat_typography::chat_font(app))
                .with_font_size(TITLE_SIZE)
                .with_line_height(22.0 / TITLE_SIZE)
                .with_weight(FontWeight::Medium)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        )
        .with_child(stats)
        .with_child(chevron)
        .finish();
    // The header (and its chevron) collapses/expands the whole card.
    let left = {
        let expanded = expanded.clone();
        HoverRow::new(left)
            .with_padding(EdgeInsets::new(4.0, 4.0, 4.0, 4.0))
            .with_corner_radius(6.0)
            .with_on_click(move || {
                let next = !*expanded.borrow();
                *expanded.borrow_mut() = next;
            })
            .finish()
    };

    let content = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(Expanded::new(left).finish())
        .with_child(review_button(app, on_review))
        .finish();
    Container::new(content)
        .with_padding(EdgeInsets::new(
            ROW_PAD_X,
            HEADER_PAD_Y,
            ROW_PAD_X,
            HEADER_PAD_Y,
        ))
        .finish()
}

fn review_button(app: &AppContext, on_review: Rc<RefCell<dyn FnMut()>>) -> Box<dyn Element> {
    let content = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new("file")
                .with_size(13.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new("Review")
                .with_font_family(chat_typography::chat_font(app))
                .with_font_size(12.0)
                .with_line_height(18.0 / 12.0)
                .with_theme_color(ColorToken::Text, app)
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
    let identity = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(
            Icon::new(file_icon_name(&file.path))
                .with_size(15.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new(name)
                .with_font_family(chat_typography::chat_font(app))
                .with_font_size(ROW_SIZE)
                .with_line_height(ROW_LINE / ROW_SIZE)
                .with_weight(FontWeight::SemiBold)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        );
    let identity = if dir.is_empty() {
        identity.finish()
    } else {
        let path = TailPathText::new(
            dir,
            ROW_SIZE,
            ROW_LINE,
            app.theme.color(ColorToken::Muted),
            chat_typography::chat_font(app),
        );
        identity
            .with_child(Expanded::new(path.finish()).finish())
            .finish()
    };
    let counts = counts(file.added, file.removed, ROW_SIZE, ROW_LINE, 6.0, app);
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .with_child(Expanded::new(identity).finish())
        .with_child(counts)
        .finish();
    HoverRow::new(
        Container::new(row)
            .with_padding(EdgeInsets::new(ROW_PAD_X, ROW_PAD_Y, ROW_PAD_X, ROW_PAD_Y))
            .finish(),
    )
    .with_padding(EdgeInsets::default())
    .with_on_click(move || (on_review.borrow_mut())())
    .finish()
}

/// A muted directory path that, when it does not fit, loses characters from
/// the LEFT and keeps a leading ellipsis: the tail (the location next to the
/// filename) stays visible. Filename and counts are separate siblings, so only
/// this element shrinks when the card narrows.
struct TailPathText {
    text: String,
    font_size: f32,
    line_height: f32,
    color: ColorU,
    family: FontFamily,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TailPathText {
    fn new(
        text: impl Into<String>,
        font_size: f32,
        line_height: f32,
        color: ColorU,
        family: FontFamily,
    ) -> Self {
        Self {
            text: text.into(),
            font_size,
            line_height,
            color,
            family,
            size: None,
            origin: None,
        }
    }
}

impl Element for TailPathText {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if constraint.max.x.is_finite() {
            constraint.max.x
        } else {
            self.text.chars().count() as f32 * self.font_size * 0.55
        };
        let size = vec2f(width.max(0.0), self.line_height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let approx_char = (self.font_size * 0.55).max(1.0);
        let max_chars = (size.x / approx_char).floor().max(0.0) as usize;
        let shown = left_truncated(&self.text, max_chars);
        let mut text = Text::new(shown)
            .with_font_family(self.family)
            .with_font_size(self.font_size)
            .with_line_height(self.line_height / self.font_size)
            .with_color(self.color)
            .with_max_lines(1)
            .finish();
        let child_size = text.layout(
            SizeConstraint::loose(vec2f(size.x, self.line_height)),
            &mut LayoutContext::default(),
            app,
        );
        let _ = child_size;
        text.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// The leading-ellipsis rule, split out so it can be tested without a lens.
fn left_truncated(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars || max_chars < 3 {
        return text.to_string();
    }
    let keep = max_chars - 3;
    let chars: Vec<char> = text.chars().collect();
    let mut start = count - keep;
    // Prefer a component boundary so the tail reads as a path, not a word.
    if let Some(offset) = chars[start..count].iter().position(|ch| *ch == '/') {
        start += offset;
    }
    let tail: String = chars[start..count].iter().collect();
    format!("...{tail}")
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

    /// T: only the path shrinks; it loses its left side and keeps the tail.
    #[test]
    fn a_long_path_truncates_from_the_left() {
        assert_eq!(left_truncated("a/b", 10), "a/b");
        let shown = left_truncated("/Users/adrian/scratch/wheelmq/deploy/grafana", 24);
        assert!(shown.starts_with(".../"), "{shown}");
        assert!(shown.ends_with("deploy/grafana"), "{shown}");
        assert!(shown.chars().count() <= 24, "{shown}");
        // A component boundary keeps the visible part path-shaped.
        let shown = left_truncated("src/components/panels/review.tsx", 24);
        assert!(shown.starts_with(".../"), "{shown}");
        assert!(shown.ends_with("panels/review.tsx"), "{shown}");
    }
}
