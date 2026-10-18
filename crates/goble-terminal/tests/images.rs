//! Image-protocol fixtures through the public API.
//!
//! The sequences are the bytes a program writes to a pty, not a transformed
//! intermediate: an APC kitty PNG, a chunked kitty RGBA transmission and a
//! sixel DCS with its palette and raster attributes.

use goble_terminal::{ImageChunk, ImageFormat, ImageTap, Screen, ScreenConfig, ScreenSize};

const KITTY_PNG: &[u8] = b"\x1b_Ga=T,f=100,i=41;iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==\x1b\\";
const KITTY_CHUNKED: &[u8] = b"\x1b_Ga=T,f=32,s=1,v=1,i=42,m=1;/wAA\x1b\\";
const KITTY_CHUNKED_END: &[u8] = b"\x1b_Gm=0,i=42;/w==\x1b\\";
// `"1;1;3;6` raster attributes, `#1;2;100;0;0` defines red, `!3~` paints a
// three-pixel red band.
const SIXEL: &[u8] = b"\x1bPq\"1;1;3;6#1;2;100;0;0!3~\x1b\\";

fn image(chunks: &[ImageChunk]) -> &goble_terminal::ImageEvent {
    let found = chunks.iter().find_map(|chunk| match chunk {
        ImageChunk::Image(image) => Some(image.as_ref()),
        ImageChunk::Bytes(_) | ImageChunk::Delete(_) => None,
    });
    found.expect("an image event")
}

#[test]
fn a_kitty_png_fixture_decodes_to_one_rgba_pixel() {
    let mut tap = ImageTap::new();
    let chunks = tap.tap(KITTY_PNG);
    let image = image(&chunks);
    assert_eq!(image.id, 41);
    assert_eq!(image.format, ImageFormat::KittyPng);
    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(image.rgba.as_deref(), Some([255, 0, 0, 255].as_slice()));
}

#[test]
fn a_chunked_kitty_fixture_is_joined_before_decoding() {
    let mut tap = ImageTap::new();
    assert!(tap.tap(KITTY_CHUNKED).is_empty(), "the first chunk waits");
    let chunks = tap.tap(KITTY_CHUNKED_END);
    let image = image(&chunks);
    assert_eq!(image.id, 42);
    assert_eq!(image.format, ImageFormat::KittyRgba);
    assert_eq!((image.width, image.height), (1, 1));
    assert_eq!(image.rgba.as_deref(), Some([255, 0, 0, 255].as_slice()));
}

#[test]
fn a_sixel_fixture_decodes_its_raster_and_palette() {
    let mut tap = ImageTap::new();
    let chunks = tap.tap(SIXEL);
    let image = image(&chunks);
    assert_eq!(image.format, ImageFormat::Sixel);
    assert_eq!((image.width, image.height), (3, 6));
    let rgba = image.rgba.as_deref().expect("the sixel decoded");
    assert_eq!(rgba.len(), 3 * 6 * 4);
    assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
}

/// The primary and alternate grids are separate surfaces: an image placed on
/// one must not be painted over the other after a program switches.
#[test]
fn an_alt_screen_swap_clears_the_images_on_both_sides() {
    let mut screen = Screen::new(ScreenSize::new(20, 4), ScreenConfig::scrolling(8));
    screen.feed(KITTY_PNG);
    assert_eq!(screen.image_cache().len(), 1);

    screen.feed(b"\x1b[?1049h");
    let after_enter = screen.image_cache().len();

    screen.feed(KITTY_PNG);
    assert_eq!(
        screen.image_cache().len(),
        1,
        "the alternate screen can still draw its own image"
    );

    screen.feed(b"\x1b[?1049l");
    let after_exit = screen.image_cache().len();

    assert_eq!((after_enter, after_exit), (0, 0));
}
