use super::*;
use crate::theme::FontFamily;

use super::atlas::{text_key, AtlasStore, Placement, ATLAS_SIZE, PADDING};
use super::fonts::{advance_width, font_set, measure_uncached, rasterize_text};

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
        rasterize_text(text, 13.0, FontWeight::Regular, false, false, 400.0, 1.2)
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
    let regular = rasterize_text("Hello", 13.0, FontWeight::Regular, false, false, 400.0, 1.2)
        .expect("regular must rasterize");
    let italic = rasterize_text("Hello", 13.0, FontWeight::Regular, false, true, 400.0, 1.2)
        .expect("italic must rasterize");
    let mono_italic = rasterize_text("Hello", 13.0, FontWeight::Regular, true, true, 400.0, 1.2)
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
        rasterize_text(text, 13.0, FontWeight::Regular, false, false, 400.0, 1.5)
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

/// A run must break its lines where its own measurement broke them. Rasterizing
/// at the exact size and wrap width the element measured with is what makes
/// that hold; rounding either of them can move a line break and leave a block
/// one line taller than the box it was measured into.
#[test]
fn a_run_breaks_where_its_measurement_broke_it() {
    let paragraph = "The agent wrote a long paragraph of prose that has to reflow on every resize";
    let mut width = 120.0_f32;
    while width <= 420.0 {
        let measured = measure_text_family(
            paragraph,
            12.0,
            1.2,
            width,
            FontWeight::Regular,
            FontFamily::System,
            false,
        );
        let (_, _, _, height) = rasterize_text(
            paragraph,
            12.0,
            FontWeight::Regular,
            false,
            false,
            width,
            1.2,
        )
        .expect("the paragraph rasterizes");
        assert_eq!(
            height as f32, measured.y,
            "the raster of a run wrapped at {width} is {height} tall, its measurement {}",
            measured.y
        );
        width += 0.5;
    }
}

/// The advance width is the number a flow budgets a run with, and it is what
/// fontdue breaks on: a run fits a wrap width of exactly its advance width and
/// not one that is a pixel narrower.
#[test]
fn the_advance_width_is_the_wrap_budget() {
    for text in ["word", "word ", "a  b   c", "The quick", "iiiii"] {
        let advance = advance_width(
            text,
            12.0,
            FontWeight::Regular,
            FontFamily::System,
            false,
        );
        assert!(advance > 0.0, "{text:?} has a positive advance");
        let (_, _, _, one_line) = rasterize_text(
            text,
            12.0,
            FontWeight::Regular,
            false,
            false,
            f32::INFINITY,
            1.2,
        )
        .expect("rasterizes");
        let (_, _, _, at_the_advance) = rasterize_text(
            text,
            12.0,
            FontWeight::Regular,
            false,
            false,
            advance,
            1.2,
        )
        .expect("rasterizes");
        assert_eq!(
            at_the_advance, one_line,
            "{text:?} fits a wrap width of its own advance"
        );
        let (_, _, _, short_of_it) = rasterize_text(
            text,
            12.0,
            FontWeight::Regular,
            false,
            false,
            advance - 0.5,
            1.2,
        )
        .expect("rasterizes");
        assert!(
            short_of_it > at_the_advance,
            "{text:?} breaks when the width is half a pixel short of its advance"
        );
        // The ink is what the box is sized by, and it never exceeds the
        // advance: a run measured by its ink still fits the space it asked for.
        let ink = measure_text_family(
            text,
            12.0,
            1.2,
            f32::INFINITY,
            FontWeight::Regular,
            FontFamily::System,
            false,
        );
        assert!(
            ink.x <= advance,
            "{text:?} ink {} over its advance {advance}",
            ink.x
        );
    }
}

/// Runs are packed into the atlas with a gutter, so a glyph drawn at the edge
/// of its own cell cannot sample the run after it. The gutter is empty in the
/// pixels the texture holds, which is what makes that true.
#[test]
fn packed_runs_keep_a_gutter_and_stay_inside_the_atlas() {
    let mut store = AtlasStore::new();
    let runs = [
        "Hello, world!",
        "a  b   c",
        "iiiii",
        "one two three four five six seven eight nine ten eleven",
        "M",
        "wrapped text that is long enough to take a few lines in a narrow cell",
    ];
    let mut regions = Vec::new();
    for (index, text) in runs.iter().enumerate() {
        let key = text_key(
            text,
            12.0,
            FontWeight::Regular,
            FontFamily::System,
            false,
            if index % 2 == 0 { 90.0 } else { f32::INFINITY },
            1.2,
        );
        match store.place(&key) {
            Placement::Written { region, .. } => regions.push((key, region)),
            other => panic!("{text:?} was not written: {}", placement_name(&other)),
        }
    }

    for (key, region) in &regions {
        // The entry's uv rect is exactly the region, and the quad the renderer
        // emits from it is the region's own size in physical pixels: nothing
        // outside the region is ever covered.
        let entry = store.entry(key).expect("every placed run has an entry");
        assert_eq!(
            entry.uv_origin,
            [
                region.x as f32 / ATLAS_SIZE as f32,
                region.y as f32 / ATLAS_SIZE as f32
            ]
        );
        assert_eq!(
            entry.uv_size,
            [
                region.width as f32 / ATLAS_SIZE as f32,
                region.height as f32 / ATLAS_SIZE as f32
            ]
        );
        assert_eq!(entry.size, [region.width as f32, region.height as f32]);
        assert!(
            region.x + region.width + PADDING <= ATLAS_SIZE
                && region.y + region.height + PADDING <= ATLAS_SIZE,
            "region {region:?} keeps its gutter inside the atlas"
        );

        // The gutter around the region is clear.
        assert!(
            !gutter_has_ink(store.pixels(), *region),
            "the gutter around {region:?} holds ink"
        );
    }

    for (index, (_, a)) in regions.iter().enumerate() {
        for (_, b) in regions.iter().skip(index + 1) {
            assert!(
                !regions_overlap(*a, *b),
                "regions {a:?} and {b:?} overlap"
            );
        }
    }
}

/// Clearing the atlas drops its entries and wipes its pixels: a run placed
/// after the clear must not sit on the previous pass's ink, which the sampler
/// would reach through the gutter.
#[test]
fn clearing_the_atlas_wipes_the_pixels_it_held() {
    let mut store = AtlasStore::new();
    let wide = text_key(
        "Wwwwwwwwwwwwwwwwwwwwwwwww",
        40.0,
        FontWeight::Regular,
        FontFamily::System,
        false,
        f32::INFINITY,
        1.2,
    );
    let Placement::Written { region: wide_region, .. } = store.place(&wide) else {
        panic!("the wide run must be placed");
    };
    assert!(
        has_ink(store.pixels(), wide_region),
        "the wide run's pixels are in the atlas"
    );

    store.reset();
    assert!(
        store.pixels().iter().all(|&pixel| pixel == 0),
        "a reset leaves no pixel of the previous pass behind"
    );
    assert!(!store.contains(&wide));

    // The atlas re-rasters from the top, where the wide run used to sit.
    let small = text_key(
        "i",
        12.0,
        FontWeight::Regular,
        FontFamily::System,
        false,
        f32::INFINITY,
        1.2,
    );
    let Placement::Written { region, .. } = store.place(&small) else {
        panic!("the small run must be placed");
    };
    assert_eq!(region.x, PADDING, "the packing starts over at the top");
    assert_eq!(region.y, PADDING);
    assert!(
        !gutter_has_ink(store.pixels(), region),
        "the run placed after a reset sits on ink the clear did not wipe"
    );
}

fn placement_name(placement: &Placement) -> &'static str {
    match placement {
        Placement::Written { .. } => "written",
        Placement::Empty => "empty",
        Placement::Full => "full",
        Placement::TooLarge => "too large",
    }
}

fn has_ink(pixels: &[u8], region: super::atlas::Region) -> bool {
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if pixels[(y * ATLAS_SIZE + x) as usize] > 0 {
                return true;
            }
        }
    }
    false
}

/// Whether the `PADDING`-wide band around `region` holds any ink. The band is
/// what a linear sample of the run's outermost texels can reach.
fn gutter_has_ink(pixels: &[u8], region: super::atlas::Region) -> bool {
    let band = super::atlas::Region {
        x: region.x.saturating_sub(PADDING),
        y: region.y.saturating_sub(PADDING),
        width: region.width + PADDING * 2,
        height: region.height + PADDING * 2,
    };
    let size = ATLAS_SIZE;
    for y in band.y..(band.y + band.height).min(size) {
        for x in band.x..(band.x + band.width).min(size) {
            if x >= region.x
                && x < region.x + region.width
                && y >= region.y
                && y < region.y + region.height
            {
                continue;
            }
            if pixels[(y * size + x) as usize] > 0 {
                return true;
            }
        }
    }
    false
}

fn regions_overlap(a: super::atlas::Region, b: super::atlas::Region) -> bool {
    a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}

/// A run to measure through the cache, with the label its failure message
/// carries: (text, font size, line height, max width, weight, family, italic).
type Run<'a> = (&'a str, f32, f32, f32, FontWeight, FontFamily, bool);

/// Measure a run through the cache and assert it is what a fresh fontdue pass
/// says, returning it so a caller can compare it with another run's.
fn cached(run: Run<'_>, label: &str) -> crate::geometry::Vector2F {
    let (text, font_size, line_height, max_width, weight, family, italic) = run;
    let through_the_cache =
        measure_text_family(text, font_size, line_height, max_width, weight, family, italic);
    let fresh = measure_uncached(text, font_size, line_height, max_width, weight, family, italic);
    assert_eq!(
        through_the_cache, fresh,
        "{label}: the cache answered {through_the_cache:?}, a fresh measurement is {fresh:?}"
    );
    through_the_cache
}

/// Every number in the cache key has to be in it. Each run below is measured
/// after the baseline, so a key that dropped the field it varies would answer
/// with the baseline's size and fail against the fresh measurement beside it.
#[test]
fn a_cached_measurement_equals_a_fresh_one_for_every_key() {
    const PARAGRAPH: &str = "The agent wrote a long paragraph of prose that has to reflow on every resize";
    let baseline = cached(
        (PARAGRAPH, 12.0, 1.2, 400.0, FontWeight::Regular, FontFamily::System, false),
        "the baseline",
    );
    let narrow = cached(
        (PARAGRAPH, 12.0, 1.2, 140.0, FontWeight::Regular, FontFamily::System, false),
        "a narrower wrap width",
    );
    assert!(
        narrow.y > baseline.y,
        "the narrower box holds more lines: {narrow:?} against {baseline:?}"
    );
    let larger = cached(
        (PARAGRAPH, 24.0, 1.2, 400.0, FontWeight::Regular, FontFamily::System, false),
        "a larger font size",
    );
    assert!(
        larger.y > baseline.y,
        "the larger face takes more room: {larger:?} against {baseline:?}"
    );
    let loftier = cached(
        (PARAGRAPH, 12.0, 2.0, 400.0, FontWeight::Regular, FontFamily::System, false),
        "a taller line height",
    );
    assert!(
        loftier.y > baseline.y,
        "the taller lines take more room: {loftier:?} against {baseline:?}"
    );
    let bold = cached(
        (PARAGRAPH, 12.0, 1.2, 400.0, FontWeight::Bold, FontFamily::System, false),
        "a heavier weight",
    );
    assert_ne!(bold.x, baseline.x, "bold is not the regular face's ink");
    let mono = cached(
        (PARAGRAPH, 12.0, 1.2, 400.0, FontWeight::Regular, FontFamily::Mono, false),
        "the mono family",
    );
    assert_ne!(mono.x, baseline.x, "mono is not the system face's ink");
    let oblique = cached(
        (PARAGRAPH, 12.0, 1.2, 400.0, FontWeight::Regular, FontFamily::System, true),
        "an oblique face",
    );
    assert_ne!(
        oblique.x, baseline.x,
        "the oblique face is not the upright one's ink"
    );
    let other = cached(
        ("A different run", 12.0, 1.2, 400.0, FontWeight::Regular, FontFamily::System, false),
        "another string",
    );
    assert_ne!(other, baseline, "another string is another size");
}

/// The per-frame cost of the rebuild, from the transcript's own render: a frame
/// that shows the same messages as the frame before it must not run the fontdue
/// pass a second time. The numbers are printed so the effect is measured rather
/// than asserted.
#[test]
fn a_second_frame_of_the_same_transcript_measures_no_text() {
    use crate::elements::chat_content::{ChatFragment, ChatMessage, ChatRole};
    use crate::elements::{AppContext, Element, LayoutContext, SizeConstraint};
    use crate::geometry::vec2f;
    use crate::ChatView;

    // Load the bundled fonts first: the one-time font load is not the cost
    // being measured here.
    let _ = measure_text(
        "warm up the bundled fonts",
        12.0,
        1.2,
        400.0,
        FontWeight::Regular,
    );
    invalidate_text_measure_cache();

    let app = AppContext::default();
    let messages: Vec<ChatMessage> = (0..60)
        .map(|index| ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text(format!(
                "streamed answer {index} with a sentence long enough to wrap in the pane it is drawn in"
            ))],
        ))
        .collect();
    let constraint = SizeConstraint::loose(vec2f(700.0, 700.0));

    let mut first = ChatView::new().with_messages(messages.clone());
    let before = measure_cache_stats();
    let started = std::time::Instant::now();
    let _ = first.layout(constraint, &mut LayoutContext::default(), &app);
    let cold = started.elapsed();
    let after_first = measure_cache_stats();

    let mut second = ChatView::new().with_messages(messages.clone());
    let started = std::time::Instant::now();
    let _ = second.layout(constraint, &mut LayoutContext::default(), &app);
    let warm = started.elapsed();
    let after_second = measure_cache_stats();

    let measured_first = after_first.misses - before.misses;
    let measured_second = after_second.misses - after_first.misses;
    let asked = (after_second.hits + after_second.misses) - (after_first.hits + after_first.misses);
    eprintln!(
        "text measurement cache: the first frame ran {measured_first} fontdue passes in {cold:?}; \
         the second was asked for {asked} runs, ran {measured_second} passes in {warm:?}"
    );
    assert!(
        measured_first > 50,
        "the sample has to ask for a transcript's worth of runs, got {measured_first}"
    );
    assert_eq!(
        measured_second, 0,
        "the second frame re-measured {measured_second} runs"
    );
    assert!(
        asked >= measured_first,
        "the second frame asks for every run the first one measured: {asked} asked for {measured_first} measured"
    );

    // A new render scale (a zoom, or a move to another display) refills the
    // atlas, and drops these answers with it: the frame after it measures
    // everything again rather than reading a size the old atlas was sized for.
    invalidate_text_measure_cache();
    let dropped = measure_cache_stats();
    assert_eq!(dropped.entries, 0, "an invalidation drops the sizes");
    assert_eq!(dropped.advance_entries, 0, "and the advances");
    let mut third = ChatView::new().with_messages(messages);
    let _ = third.layout(constraint, &mut LayoutContext::default(), &app);
    assert!(
        measure_cache_stats().misses > dropped.misses,
        "the frame after an invalidation measures its runs again"
    );
}
