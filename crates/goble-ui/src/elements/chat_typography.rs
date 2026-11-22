//! The chat transcript's type system, in one place.
//!
//! Every in-chat surface — assistant prose, the user's own message, reasoning
//! rows, tool rows and their expanded bodies, fenced code, the work dock's
//! announcements, the queued-prompt band, the failed-turn block and the footer
//! pills — takes its size, line height, weight, ink and shape from here. The
//! numbers are DeepSeek Harness's own, sourced from its `gradient-shadow-text`
//! token sheet and its per-component CSS modules (see
//! `/tmp/goble-work/chat-typography-spec.md` §1.3, §1.4 and §3.3).
//!
//! Two rules keep the Settings surface the single source of truth:
//!
//! * every colour is resolved through `app.theme.color(token)` — no literal hex
//!   lives in chat code, so the Appearance overrides keep driving every ink;
//! * the family is resolved through [`chat_font`], which reads `app.theme.font`
//!   (a future Settings family control is a one-line write there), never a
//!   hard-coded `FontFamily` at a call site.
//!
//! Sizes and line heights are logical pixels, like the rest of Goble's layout,
//! so the existing Settings "Font size" zoom (`ui_zoom`, applied by the render
//! pass) keeps scaling every chat surface. Density is deliberately *not* applied
//! to type: DSH has no density axis, and "same size as DSH" has to hold at every
//! density setting. Padding and gaps still use `theme.spacing_px` where DSH's
//! number coincides with a Goble token, so Density keeps moving the rhythm.
//!
//! Letterform gap (documented, not hidden): Goble renders the bundled Roboto /
//! Hack faces through fontdue; DSH renders the platform UI face (SF Pro on
//! macOS) through CoreText. True letterform parity needs a CoreText/static-faces
//! pipeline (spec §4.5) and is out of scope here — this module delivers DSH's
//! size / line-height / weight / colour / shape system, which is the visible
//! gap.

use crate::color::ColorU;
use crate::elements::{AppContext, Border, Fill};
use crate::theme::{ColorToken, FontFamily, SpacingToken};

// ---------------------------------------------------------------------------
// Assistants' prose (DSH `markdown-base`, `emphasis-strong`)
// ---------------------------------------------------------------------------

/// Assistant prose, list text and table-of-contents body: DSH's 14 px.
pub const BODY_SIZE: f32 = 14.0;
/// 24 / 14: DSH's markdown base line height.
pub const BODY_LH: f32 = 1.714_285_7;
/// The gap between two prose blocks: DSH's `AssistantMarkdown` 16 px.
pub const BLOCK_GAP: f32 = 16.0;

// ---------------------------------------------------------------------------
// The user's own message (DSH `s-14`, UserMessageBubble)
// ---------------------------------------------------------------------------

/// 22 / 14: DSH's user-bubble line height.
pub const USER_LH: f32 = 1.571_428_6;
/// DSH's user-bubble corner radius: `--dsw-radius-xl` = 20 px at defaults.
pub const USER_BUBBLE_RADIUS: f32 = 20.0;
/// DSH's user-stack width cap: `--dsh-chat-content-width * 0.702` (525 px on
/// the 748 px default column).
pub const USER_BUBBLE_MAX_RATIO: f32 = 0.702;
/// DSH's fallback user-stack cap: `82%` of the width it is mounted in, used
/// when that width is narrower than the conversation column the 0.702 ratio
/// assumes.
pub const USER_BUBBLE_FALLBACK_RATIO: f32 = 0.82;
/// The lane width below which the 82% fallback applies: the narrowest content
/// column Goble's lane can be dragged to (`CONTENT_LANE_MIN_CONTENT_WIDTH`).
pub const USER_BUBBLE_NARROW_LANE: f32 = 640.0;
/// The widest a user bubble may grow inside a `lane`-wide transcript: DSH's
/// `min(content-width * 0.702, 82%)`, expressed against the lane Goble has.
/// A narrow lane takes the 82% branch so a prompt does not shrink to a column.
pub fn user_bubble_max_width(lane: f32) -> f32 {
    if !lane.is_finite() {
        return lane;
    }
    let lane = lane.max(0.0);
    let ratio = if lane < USER_BUBBLE_NARROW_LANE {
        USER_BUBBLE_FALLBACK_RATIO
    } else {
        USER_BUBBLE_MAX_RATIO
    };
    lane * ratio
}
/// A transcript band's horizontal inset: DSH's bubble padding 10 × 16.
pub const BAND_PADDING_X: f32 = 16.0;
/// Goble's own role marker: the `❯` that leads the user's row (DSH has no
/// marker; the owner's requirement keeps Goble's), its glyph size and the room
/// between it and the message's first character.
pub const PROMPT_ARROW_SIZE: f32 = 11.0;
/// The room between the marker and the message's first character.
pub const PROMPT_ARROW_GAP: f32 = 4.0;
/// Where the marker sits inside the band, centred on the user line's box.
pub fn prompt_marker_top() -> f32 {
    (BODY_SIZE * USER_LH - PROMPT_ARROW_SIZE) / 2.0
}
/// A transcript band's vertical inset.
pub const BAND_PADDING_Y: f32 = 10.0;

// ---------------------------------------------------------------------------
// Process rows: tool rows, reasoning, notices, dock rows (DSH `xs-13`, 13/24)
// ---------------------------------------------------------------------------

/// One process row's text: DSH's 13 px secondary tier.
pub const ROW_SIZE: f32 = 13.0;
/// 24 / 13: DSH's process-row line height.
pub const ROW_LH: f32 = 1.846_153_8;
/// 20 / 13: DSH's compact row / failure reason line height.
pub const ROW_TIGHT_LH: f32 = 1.538_461_5;
/// The fixed height of a process row (tool, notice, dock row): DSH's 24 px.
pub const ROW_HEIGHT: f32 = 24.0;
/// A group header's fixed height (dock, turn-run summary): DSH's 33 px.
pub const HEADER_HEIGHT: f32 = 33.0;
/// The box a row's leading glyph occupies: DSH's 16 × 16.
pub const LEADING_BOX: f32 = 16.0;
/// The glyph drawn in that box: DSH's 14 px icon slot.
pub const GLYPH_SIZE: f32 = 14.0;
/// The gap between the leading box and the row's words: DSH's 6 px.
pub const GLYPH_GAP: f32 = 6.0;
/// The separator dot between a row's title and its summary: 2 × 2.
pub const DOT_SIZE: f32 = 2.0;
/// The dot's corner radius.
pub const DOT_RADIUS: f32 = 1.0;
/// The dot's margins: `0 8`.
pub const DOT_GAP: f32 = 8.0;
/// The indent of a row's body from the row's text origin: DSH's 22 px
/// (`16 px leading box + 6 px gap`).
pub const BODY_INDENT: f32 = 22.0;

// ---------------------------------------------------------------------------
// Small type: timestamps, metas, footer pills (DSH `xxs-12`)
// ---------------------------------------------------------------------------

/// DSH's 12 px small tier.
pub const SMALL_SIZE: f32 = 12.0;
/// 18 / 12: DSH's small line height (timestamps, metas).
pub const SMALL_LH: f32 = 1.5;
/// 20 / 12: DSH's stat-pill line height.
pub const PILL_LH: f32 = 1.666_666_7;
/// The glyph a footer pill leads with: DSH's 14 px.
pub const PILL_ICON: f32 = 14.0;
/// A footer pill's height: DSH's 28 px.
pub const PILL_HEIGHT: f32 = 28.0;

// ---------------------------------------------------------------------------
// Code (DSH `markdown-code-block`, `markdown-code-block-small`)
// ---------------------------------------------------------------------------

/// A fenced code block's body: DSH's 11 px code size.
pub const CODE_SIZE: f32 = 11.0;
/// 19 / 11: DSH's fenced-code line height.
pub const CODE_LH: f32 = 1.727_272_7;
/// 16 / 11: DSH's tool-body / context line height.
pub const CODE_SMALL_LH: f32 = 1.454_545_5;
/// A fenced block's language banner: DSH's 11 px.
pub const CODE_LANGUAGE_SIZE: f32 = 11.0;
/// 18 / 11: DSH's code-banner line height.
pub const CODE_LANGUAGE_LH: f32 = 1.636_363_6;
/// The inline-code size DSH derives at the default content size (`0.875 × 14`).
pub const INLINE_CODE_SIZE: f32 = 12.0;

// ---------------------------------------------------------------------------
// Headings (DSH `markdown-h1..h4`), tables and the hairline card tokens
// ---------------------------------------------------------------------------

/// DSH's h1 size.
pub const H1_SIZE: f32 = 21.0;
/// 30 / 21.
pub const H1_LH: f32 = 1.428_571_4;
/// DSH's h2 size.
pub const H2_SIZE: f32 = 19.0;
/// 28 / 19.
pub const H2_LH: f32 = 1.473_684_2;
/// DSH's h3 size.
pub const H3_SIZE: f32 = 18.0;
/// 26 / 18.
pub const H3_LH: f32 = 1.444_444_4;
/// DSH's h4 size.
pub const H4_SIZE: f32 = 14.0;
/// 24 / 14.
pub const H4_LH: f32 = BODY_LH;
/// The margin above DSH's h1–h3 headings (and the same value below them,
/// which the prose block gap already supplies). h4's margin is the block gap
/// alone.
pub const H1_TOP_MARGIN: f32 = 32.0;
/// A table cell's size.
pub const TABLE_SIZE: f32 = 13.0;
/// 22 / 13.
pub const TABLE_LH: f32 = 1.692_307_7;
/// A card's hairline stroke (DSH's `0.5px`).
pub const HAIRLINE: f32 = 0.5;

// ---------------------------------------------------------------------------
// In-chat announcements (DSH TurnTriggerNodeView)
// ---------------------------------------------------------------------------

/// The announcement card's header horizontal padding: 12 × 16.
pub const ANNOUNCEMENT_PAD_Y: f32 = 12.0;
/// The announcement card's header trailing padding.
pub const ANNOUNCEMENT_PAD_X: f32 = 16.0;
/// The gap between an announcement's glyph, title and trailing time: 10 px.
pub const ANNOUNCEMENT_GAP: f32 = 10.0;
/// The source glyph an announcement leads with: 14 px.
pub const ANNOUNCEMENT_ICON: f32 = 14.0;
/// The chevron an announcement's header carries: 12 px.
pub const ANNOUNCEMENT_CHEVRON: f32 = 12.0;
/// The indent of an announcement's opened body: 40 px.
pub const ANNOUNCEMENT_BODY_INDENT: f32 = 40.0;
/// The card radius DSH gives an announcement: `radius_xl` = 20 px at defaults,
/// expressed as Goble's Settings-driven radius plus 12.
pub const ANNOUNCEMENT_RADIUS_EXTRA: f32 = 12.0;

/// A code / tool card's radius: DSH's `radius-lg` = 16 px at defaults,
/// expressed as Goble's Settings-driven radius plus 8.
pub const CARD_RADIUS_EXTRA: f32 = 8.0;
/// The tallest an expanded tool body's card grows before it scrolls
/// (`GenericCommandCard`'s `max-height: 260px`), inset by the card's own
/// vertical padding.
pub const CARD_MAX_INNER_HEIGHT: f32 = 236.0;
/// A tool card's padding: DSH's `12 16 12 12`.
pub const CARD_PADDING: crate::elements::EdgeInsets = crate::elements::EdgeInsets {
    left: 12.0,
    top: 12.0,
    right: 16.0,
    bottom: 12.0,
};

// ---------------------------------------------------------------------------
// Settings seams
// ---------------------------------------------------------------------------

/// The family chat prose and process rows are drawn in: the app's theme family,
/// so a future Settings family control drives every chat surface by writing
/// `theme.font` once. Code and tool bodies deliberately stay
/// [`FontFamily::Mono`] (DSH also uses its code stack there).
pub fn chat_font(app: &AppContext) -> FontFamily {
    app.theme.font
}

/// The primary ink (`label-primary` in DSH) every chat body reads.
pub fn chat_ink(app: &AppContext) -> ColorU {
    app.theme.color(ColorToken::Text)
}

/// The quiet ink (`label-secondary` / `label-tertiary` in DSH) for the process
/// rows, metas and announcements. This is the single seam through which a later
/// mid-tier grey (DSH's third ink level, which Goble's two-tier theme does not
/// have) would be introduced — call sites do not need to change.
pub fn chat_ink_quiet(app: &AppContext) -> ColorU {
    app.theme.color(ColorToken::Muted)
}

/// An announcement card's corner radius. Settings-driven: the default radius
/// (8) yields DSH's 20.
pub fn announcement_radius(app: &AppContext) -> f32 {
    app.theme.radius_px() + ANNOUNCEMENT_RADIUS_EXTRA
}

/// A code / tool card's corner radius. Settings-driven: default yields 16.
pub fn card_radius(app: &AppContext) -> f32 {
    app.theme.radius_px() + CARD_RADIUS_EXTRA
}

/// A card's hairline border, in the theme's border colour.
pub fn card_border(app: &AppContext) -> Border {
    Border::all(HAIRLINE).with_border_color(app.theme.color(ColorToken::Border))
}

/// A card's fill: DSH's `markdown-code-block` surface.
pub fn card_surface(app: &AppContext) -> Fill {
    Fill::Solid(app.theme.color(ColorToken::SurfaceRaised))
}

/// A card's inset surface, used as the announcement background (DSH's
/// `turn-trigger-bg` falls back to `markdown-code-block`).
pub fn announcement_surface(app: &AppContext) -> Fill {
    Fill::Solid(app.theme.color(ColorToken::SurfaceRaised))
}

/// The line height a row's text mark is drawn at inside its leading cell:
/// `16 / 13`, so the mark's line box is exactly [`LEADING_BOX`] high and its
/// baseline is the words' baseline (both runs are the same font size painted at
/// the row's top).
pub const GLYPH_LH: f32 = LEADING_BOX / ROW_SIZE;

/// Draw a row's text mark (Goble's `◆`, `◈`, `❯`, `·` or a spinner frame) in
/// DSH's fixed 16 px leading cell.
///
/// The mark is a text run, not an icon: it is drawn at [`GLYPH_LH`] so its line
/// box measures exactly the 16 px cell. The mark and the row's words are painted
/// at the same top and the same font size, so they share a baseline — no top
/// padding, which would shift the mark (and the row height) down by four.
pub fn leading_text(
    mark: impl Into<String>,
    color: ColorU,
    app: &AppContext,
) -> Box<dyn crate::elements::Element> {
    use crate::elements::{ConstrainedBox, Element, InlineText, TextSpan};
    let run = InlineText::new(vec![TextSpan::plain(mark.into())
        .with_color(color)
        .with_family(chat_font(app))])
    .with_font_size(ROW_SIZE)
    .with_line_height(GLYPH_LH)
    .finish();
    ConstrainedBox::new(run)
        .with_width(LEADING_BOX)
        .with_height(LEADING_BOX)
        .finish()
}

/// Draw a row's leading icon in DSH's fixed 16 × 16 cell, centred inside it
/// (DSH's 14 px glyph slot leaves a 1 px inset; Goble's 12 px dock chevron
/// leaves 2 px). The cell is the whole leading element, so a row laid out
/// cross-centre puts the icon on the title's optical centre and a row laid out
/// start-aligned keeps it on the first line.
pub fn leading_icon(
    icon: &'static str,
    glyph_size: f32,
    color: ColorU,
) -> Box<dyn crate::elements::Element> {
    use crate::elements::{ConstrainedBox, EdgeInsets, Element, Icon, Padding};
    let inset = ((LEADING_BOX - glyph_size) / 2.0).max(0.0);
    let body = Padding::new(
        Icon::new(icon)
            .with_size(glyph_size)
            .with_color(color)
            .finish(),
        EdgeInsets::uniform(inset),
    )
    .finish();
    ConstrainedBox::new(body)
        .with_width(LEADING_BOX)
        .with_height(LEADING_BOX)
        .finish()
}

/// The prose block gap in logical px: DSH's literal 16.
pub fn block_gap(app: &AppContext) -> f32 {
    // The DSH number coincides with Goble's `Lg`, so Density keeps moving it.
    app.theme.spacing_px(SpacingToken::Lg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn line_height_multipliers_are_dsh_px_over_dsh_size() {
        // The multipliers are what `Text`/`InlineText` take, so they are pinned
        // as the exact quotients rather than re-derived at each call site.
        assert!((BODY_LH - 24.0 / 14.0).abs() < 1e-6);
        assert!((USER_LH - 22.0 / 14.0).abs() < 1e-6);
        assert!((ROW_LH - 24.0 / 13.0).abs() < 1e-6);
        assert!((ROW_TIGHT_LH - 20.0 / 13.0).abs() < 1e-6);
        assert!((SMALL_LH - 18.0 / 12.0).abs() < 1e-6);
        assert!((PILL_LH - 20.0 / 12.0).abs() < 1e-6);
        assert!((CODE_LH - 19.0 / 11.0).abs() < 1e-6);
        assert!((CODE_SMALL_LH - 16.0 / 11.0).abs() < 1e-6);
        assert!((H1_LH - 30.0 / 21.0).abs() < 1e-6);
        assert!((H2_LH - 28.0 / 19.0).abs() < 1e-6);
        assert!((H3_LH - 26.0 / 18.0).abs() < 1e-6);
        assert!((TABLE_LH - 22.0 / 13.0).abs() < 1e-6);
    }

    #[test]
    fn radii_and_surfaces_follow_the_theme_settings() {
        let mut app = AppContext::default();
        // Default radius 8: DSH's 20 for an announcement, 16 for a card.
        assert_eq!(announcement_radius(&app), 20.0);
        assert_eq!(card_radius(&app), 16.0);

        // A Settings radius change moves both, derived — never a literal.
        app.theme = Theme::dark();
        app.theme.radius = crate::theme::Radius::Rounded;
        assert_eq!(announcement_radius(&app), 14.0 + 12.0);
        assert_eq!(card_radius(&app), 14.0 + 8.0);

        assert_eq!(
            card_surface(&app),
            Fill::Solid(app.theme.color(ColorToken::SurfaceRaised))
        );
        assert_eq!(
            announcement_surface(&app),
            Fill::Solid(app.theme.color(ColorToken::SurfaceRaised))
        );
        // The border is a theme read too: the hairline width is fixed, the ink
        // follows the Settings-driven `Border` token.
        let _ = card_border(&app);
    }

    #[test]
    fn colour_helpers_read_the_theme_channels_never_literals() {
        let mut app = AppContext::default();
        assert_eq!(chat_ink(&app), app.theme.color(ColorToken::Text));
        assert_eq!(chat_ink_quiet(&app), app.theme.color(ColorToken::Muted));

        // An Appearance override changes what the helpers return, which is the
        // Settings-linkage contract every chat call site inherits.
        let overridden = crate::color::ColorU::new(1, 2, 3, 255);
        app.theme = Theme::dark().with_overrides(Some(overridden), None, None, None, None);
        assert_eq!(chat_ink(&app), overridden);
    }

    #[test]
    fn the_user_bubble_cap_is_dshs_ratio_with_a_narrow_lane_fallback() {
        // A full content lane takes DSH's 0.702 cap.
        assert!((user_bubble_max_width(920.0) - 920.0 * 0.702).abs() < 1e-4);
        // Below Goble's narrowest content column the 82% fallback keeps the
        // bubble from collapsing to a 70.2% sliver.
        assert!((user_bubble_max_width(600.0) - 600.0 * 0.82).abs() < 1e-4);
        assert!((user_bubble_max_width(639.0) - 639.0 * 0.82).abs() < 1e-4);
        assert!((user_bubble_max_width(640.0) - 640.0 * 0.702).abs() < 1e-4);
        // A degenerate lane never produces a negative cap.
        assert_eq!(user_bubble_max_width(0.0), 0.0);
    }

    #[test]
    fn the_family_seam_reads_theme_font() {
        let mut app = AppContext::default();
        assert_eq!(chat_font(&app), FontFamily::System);
        app.theme = Theme::dark();
        app.theme.font = FontFamily::Mono;
        assert_eq!(chat_font(&app), FontFamily::Mono);
    }
}
