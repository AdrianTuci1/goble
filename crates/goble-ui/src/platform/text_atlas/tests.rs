use super::*;
use crate::theme::FontFamily;

use super::fonts::{font_set, rasterize_text};

#[test]
fn line_height_grows_wrapped_block_height() {
    let short = measure_text_family(
        "one two three four five",
        13.0,
        1.2,
        40.0,
        FontWeight::Regular,
        FontFamily::System,
        false,
    );
    let tall = measure_text_family(
        "one two three four five",
        13.0,
        2.0,
        40.0,
        FontWeight::Regular,
        FontFamily::System,
        false,
    );
    assert!(
        short.y > 0.0,
        "wrapped text should occupy more than one line"
    );
    assert!(
        tall.y > short.y,
        "a larger line-height multiplier must grow the block height"
    );
}

#[test]
fn semibold_uses_static_distinct_font() {
    let set = font_set().expect("bundled fonts must load");
    let regular = set
        .select(FontWeight::Regular, FontFamily::System, false)
        .file_hash();
    let semibold = set
        .select(FontWeight::SemiBold, FontFamily::System, false)
        .file_hash();
    let bold = set
        .select(FontWeight::Bold, FontFamily::System, false)
        .file_hash();
    let italic = set
        .select(FontWeight::Regular, FontFamily::System, true)
        .file_hash();
    let bold_italic = set
        .select(FontWeight::Bold, FontFamily::System, true)
        .file_hash();
    assert_ne!(
        semibold, bold,
        "SemiBold must not resolve to the Bold font file"
    );
    assert_ne!(semibold, regular, "SemiBold must differ from Regular");
    assert_ne!(bold, regular, "Bold must differ from Regular");
    assert_ne!(
        italic, regular,
        "Italic must resolve to an oblique font file"
    );
    assert_ne!(
        bold_italic, bold,
        "BoldItalic must resolve to an oblique font file"
    );
    assert_ne!(bold_italic, italic, "BoldItalic must differ from Italic");
}

#[test]
fn raster_quad_agrees_with_measure_for_single_line() {
    let text = "Hello, world!";
    let (entry, atlas, width, height) =
        rasterize_text(text, 13, FontWeight::Regular, false, false, 400.0, 1.2)
            .expect("single line must rasterize");

    // The quad the renderer draws (origin `offset`, size `size`) must match the
    // box `measure_text_family` produced for the element, otherwise text is
    // clipped or shifted.
    let measured = measure_text_family(
        text,
        13.0,
        1.2,
        400.0,
        FontWeight::Regular,
        FontFamily::System,
        false,
    );
    assert!(
        (entry.size[0] - measured.x).abs() <= 2.0,
        "quad width {} vs measured {}",
        entry.size[0],
        measured.x
    );
    assert!(
        (entry.size[1] - measured.y).abs() <= 2.0,
        "quad height {} vs measured {}",
        entry.size[1],
        measured.y
    );

    assert_eq!(entry.size[0], width as f32);
    assert_eq!(entry.size[1], height as f32);

    // No double-counted left bearing: the ink must start at the element origin.
    assert!(
        entry.offset[0].abs() <= 1.0,
        "left offset should be ~0, got {}",
        entry.offset[0]
    );
    assert!(
        entry.offset[1].abs() <= 1.0,
        "top offset should be ~0, got {}",
        entry.offset[1]
    );

    // The atlas region must actually contain ink, not be blank.
    assert!(
        atlas.iter().any(|&p| p > 0),
        "rasterized region must contain non-zero coverage"
    );
}

#[test]
fn italic_rasterizes_a_distinct_face() {
    let regular = rasterize_text("Hello", 13, FontWeight::Regular, false, false, 400.0, 1.2)
        .expect("regular must rasterize");
    let italic = rasterize_text("Hello", 13, FontWeight::Regular, false, true, 400.0, 1.2)
        .expect("italic must rasterize");
    let mono_italic = rasterize_text("Hello", 13, FontWeight::Regular, true, true, 400.0, 1.2)
        .expect("mono italic must rasterize");
    assert!(
        italic.1.iter().any(|&p| p > 0),
        "italic raster must contain non-zero coverage"
    );
    assert_ne!(
        regular.1, italic.1,
        "italic must rasterize the oblique face, not the regular one"
    );
    assert!(mono_italic.1.iter().any(|&p| p > 0));
}

#[test]
fn raster_quad_agrees_with_measure_for_wrapped_block() {
    let text = "line one\nline two\nline three";
    let (entry, atlas, _w, _h) =
        rasterize_text(text, 13, FontWeight::Regular, false, false, 400.0, 1.5)
            .expect("multi-line must rasterize");
    let measured = measure_text_family(
        text,
        13.0,
        1.5,
        400.0,
        FontWeight::Regular,
        FontFamily::System,
        false,
    );
    assert!(
        (entry.size[1] - measured.y).abs() <= 2.0,
        "quad height {} vs measured {}",
        entry.size[1],
        measured.y
    );
    assert!(
        atlas.iter().any(|&p| p > 0),
        "multi-line raster must contain ink"
    );
}
