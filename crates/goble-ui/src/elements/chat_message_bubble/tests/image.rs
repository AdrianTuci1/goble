//! Markdown images: a real local PNG is decoded and drawn at DSH's prose size,
//! a missing file falls back to the alt text, and a click dispatches the action
//! the app routes (a file pane for a local path, the external opener otherwise).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::elements::chat_content::ChatAction;
use crate::elements::EventContext;

/// A fixture directory unique to one test: the image cache is thread-local and
/// keyed by path, so two tests must never share a file name.
fn fixture_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "goble-chat-image-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("the fixture directory is writable");
    dir
}

/// Write a deterministic RGBA PNG and return its path. The bytes are a real
/// image file on disk, not fake pixels handed to a preview.
fn write_png(name: &str, width: u32, height: u32) -> PathBuf {
    let path = fixture_dir().join(name);
    let image = ::image::RgbaImage::from_fn(width, height, |x, y| {
        ::image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
    });
    image.save(&path).expect("the fixture PNG is written");
    path
}

fn cleanup(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

fn approx(what: &str, got: f32, want: f32) {
    assert!((got - want).abs() < 0.51, "{what}: got {got}, want {want}");
}

/// The one image the bubble drew, with the rect it was laid out at.
fn drawn_image(commands: &[RenderCommand]) -> (crate::geometry::RectF, u32, u32, String) {
    commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawImage {
                rect,
                width,
                height,
                source,
                ..
            } => Some((*rect, *width, *height, source.clone())),
            _ => None,
        })
        .expect("the image block draws its pixels")
}

/// A text run with the exact `label`.
fn drawn_text<'a>(commands: &'a [RenderCommand], label: &str) -> &'a RenderCommand {
    commands
        .iter()
        .find(|command| {
            matches!(
                command,
                RenderCommand::DrawText { text, .. } if text == label
            )
        })
        .unwrap_or_else(|| panic!("{label:?} is drawn: {commands:?}"))
}

#[test]
fn a_local_image_draws_at_the_capped_size_and_keeps_its_aspect() {
    let path = write_png("wide.png", 640, 320);
    let app = AppContext::default();
    let mut bubble: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::image(
            "A wide diagram",
            path.to_string_lossy().into_owned(),
        )],
    ));
    let commands = render_element(&mut bubble, vec2f(600.0, 800.0), &app);
    let (rect, width, height, source) = drawn_image(&commands);

    assert_eq!((width, height), (640, 320), "the decoded file's pixels");
    approx("the image's capped width", rect.width(), 360.0);
    approx("the image's aspect-preserving height", rect.height(), 180.0);
    assert_eq!(
        source,
        path.to_string_lossy(),
        "the texture cache is keyed by the file's own path"
    );

    // A 1 px theme border traces the same rounded rect.
    let border = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::StrokeRect {
                rect: stroke,
                color,
                width,
                corner_radius,
            } if *width == 1.0 && *color == app.theme.color(ColorToken::Border) => {
                Some((*stroke, *corner_radius))
            }
            _ => None,
        })
        .expect("the image draws its hairline border");
    assert_eq!(
        (border.0.min_x(), border.0.max_x()),
        (rect.min_x(), rect.max_x()),
        "the border traces the image's own rect"
    );
    assert!(border.1 > 0.0, "the border follows the rounded corners");

    // The alt text is a quiet caption under the picture, not the accent chip.
    let caption = drawn_text(&commands, "A wide diagram");
    let RenderCommand::DrawText {
        origin,
        color,
        font_size,
        ..
    } = caption
    else {
        unreachable!()
    };
    assert_eq!(*color, app.theme.color(ColorToken::Muted), "quiet caption");
    assert_eq!(*font_size, 12.0, "DSH's small tier");
    assert!(
        origin.x >= rect.min_x() - 0.51,
        "the caption starts under the image"
    );
    assert!(
        origin.y >= rect.max_y() + 6.0 - 0.51,
        "the caption sits below the image's {:.1} px bottom",
        rect.max_y()
    );
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { color, .. }
                if *color == app.theme.color(ColorToken::Accent)
        )),
        "a decoded image does not draw the accent fallback chip"
    );

    cleanup(&path);
}

#[test]
fn a_tall_image_caps_its_height_and_keeps_its_aspect() {
    let path = write_png("tall.png", 100, 1000);
    let app = AppContext::default();
    let mut bubble: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::image("", path.to_string_lossy().into_owned())],
    ));
    let commands = render_element(&mut bubble, vec2f(600.0, 900.0), &app);
    let (rect, _, _, _) = drawn_image(&commands);
    approx("the tall image's capped height", rect.height(), 280.0);
    approx("the tall image's aspect width", rect.width(), 28.0);
    cleanup(&path);
}

#[test]
fn a_missing_image_falls_back_to_the_alt_chip() {
    let path = fixture_dir().join("missing.png");
    let app = AppContext::default();
    let mut bubble: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::image(
            "missing chart",
            path.to_string_lossy().into_owned(),
        )],
    ));
    let commands = render_element(&mut bubble, vec2f(600.0, 400.0), &app);
    assert_eq!(
        command_counts(&commands).draw_image,
        0,
        "a file that cannot be decoded draws no pixels"
    );
    let fallback = drawn_text(&commands, "missing chart");
    let RenderCommand::DrawText { color, .. } = fallback else {
        unreachable!()
    };
    assert_eq!(
        *color,
        app.theme.color(ColorToken::Accent),
        "the fallback is the existing accent alt chip"
    );
    cleanup(&path);
}

#[test]
fn a_remote_image_never_fetches_and_falls_back_to_alt() {
    let app = AppContext::default();
    let mut bubble: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::image(
            "remote picture",
            "https://example.com/picture.png",
        )],
    ));
    let commands = render_element(&mut bubble, vec2f(600.0, 400.0), &app);
    assert_eq!(
        command_counts(&commands).draw_image,
        0,
        "a network URL is never fetched"
    );
    let fallback = drawn_text(&commands, "remote picture");
    let RenderCommand::DrawText { color, .. } = fallback else {
        unreachable!()
    };
    assert_eq!(*color, app.theme.color(ColorToken::Accent));
}

#[test]
fn clicking_a_local_image_emits_open_file() {
    let path = write_png("clickable.png", 32, 16);
    let app = AppContext::default();
    let received: Rc<RefCell<Vec<ChatAction>>> = Rc::default();
    let sink = Rc::clone(&received);
    let mut bubble: Box<dyn Element> = Box::new(
        ChatMessageBubble::new(
            ChatRole::Assistant,
            vec![ChatFragment::image(
                "clickable picture",
                path.to_string_lossy().into_owned(),
            )],
        )
        .with_on_action(move |action| sink.borrow_mut().push(action)),
    );
    let commands = render_element(&mut bubble, vec2f(600.0, 400.0), &app);
    let (rect, ..) = drawn_image(&commands);
    let position = vec2f(
        (rect.min_x() + rect.max_x()) / 2.0,
        (rect.min_y() + rect.max_y()) / 2.0,
    );
    let mut ctx = EventContext::default();
    bubble.dispatch_event(
        &DispatchedEvent::MouseDown {
            position,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    bubble.dispatch_event(
        &DispatchedEvent::MouseUp {
            position,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert_eq!(
        *received.borrow(),
        vec![ChatAction::OpenFile(path.to_string_lossy().into_owned())],
        "a local picture opens in a file pane"
    );
    cleanup(&path);
}

#[test]
fn clicking_the_remote_fallback_emits_open_url() {
    let app = AppContext::default();
    let received: Rc<RefCell<Vec<ChatAction>>> = Rc::default();
    let sink = Rc::clone(&received);
    let mut bubble: Box<dyn Element> = Box::new(
        ChatMessageBubble::new(
            ChatRole::Assistant,
            vec![ChatFragment::image(
                "remote picture",
                "https://example.com/picture.png",
            )],
        )
        .with_on_action(move |action| sink.borrow_mut().push(action)),
    );
    let commands = render_element(&mut bubble, vec2f(600.0, 400.0), &app);
    let fallback = drawn_text(&commands, "remote picture");
    let RenderCommand::DrawText { origin, .. } = fallback else {
        unreachable!()
    };
    let position = vec2f(origin.x + 2.0, origin.y + 4.0);
    let mut ctx = EventContext::default();
    bubble.dispatch_event(
        &DispatchedEvent::MouseDown {
            position,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    bubble.dispatch_event(
        &DispatchedEvent::MouseUp {
            position,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert_eq!(
        *received.borrow(),
        vec![ChatAction::OpenUrl(
            "https://example.com/picture.png".to_string()
        )],
        "a remote picture opens through the external opener"
    );
}

#[test]
fn decoded_pixels_are_reused_across_per_frame_rebuilds() {
    let path = write_png("cached.png", 48, 24);
    let app = AppContext::default();
    let mut bubble: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::image("", path.to_string_lossy().into_owned())],
    ));
    let data = |commands: &[RenderCommand]| {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawImage { data, .. } => Some(Arc::clone(data)),
                _ => None,
            })
            .expect("the image is drawn")
    };
    let first = data(&render_element(&mut bubble, vec2f(600.0, 400.0), &app));
    let second = data(&render_element(&mut bubble, vec2f(600.0, 400.0), &app));
    assert!(
        Arc::ptr_eq(&first, &second),
        "the second frame reuses the decoded pixels instead of re-decoding"
    );
    cleanup(&path);
}

#[test]
fn a_rewritten_image_is_decoded_again() {
    let path = write_png("rewritten.png", 20, 10);
    let app = AppContext::default();
    let mut bubble: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::image("", path.to_string_lossy().into_owned())],
    ));
    let first = render_element(&mut bubble, vec2f(600.0, 400.0), &app);
    let (first_rect, ..) = drawn_image(&first);
    let first_data = first
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawImage { data, .. } => Some(Arc::clone(data)),
            _ => None,
        })
        .expect("the first frame draws the image");

    // A different-size rewrite updates the cache key (size and mtime), so the
    // next frame decodes the new bytes instead of drawing the old ones.
    let rewritten = ::image::RgbaImage::from_fn(40, 20, |x, y| {
        ::image::Rgba([(x * 3) as u8, (y * 5) as u8, 128, 255])
    });
    rewritten.save(&path).expect("the rewritten PNG is saved");
    let second = render_element(&mut bubble, vec2f(600.0, 400.0), &app);
    let (second_rect, second_width, second_height, _) = drawn_image(&second);
    let second_data = second
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawImage { data, .. } => Some(Arc::clone(data)),
            _ => None,
        })
        .expect("the second frame draws the image");
    assert_eq!((second_width, second_height), (40, 20));
    approx("the rewritten image's width", second_rect.width(), 40.0);
    approx("the rewritten image's height", second_rect.height(), 20.0);
    assert_ne!(
        first_rect, second_rect,
        "the drawn rect follows the new bytes"
    );
    assert!(
        !Arc::ptr_eq(&first_data, &second_data),
        "a changed file is decoded again"
    );
    cleanup(&path);
}

#[test]
fn image_targets_resolve_local_paths_and_file_urls_only() {
    use crate::elements::chat_message_bubble::image::{resolve_image_target, ImageTarget};

    assert_eq!(
        resolve_image_target("/tmp/a.png"),
        ImageTarget::Local(PathBuf::from("/tmp/a.png"))
    );
    assert_eq!(
        resolve_image_target("file:///tmp/a%20b.png"),
        ImageTarget::Local(PathBuf::from("/tmp/a b.png"))
    );
    assert_eq!(
        resolve_image_target("file://localhost/tmp/a.png"),
        ImageTarget::Local(PathBuf::from("/tmp/a.png"))
    );
    assert_eq!(
        resolve_image_target("https://example.com/a.png"),
        ImageTarget::Remote("https://example.com/a.png".to_string())
    );
    assert_eq!(
        resolve_image_target("data:image/png;base64,AAAA"),
        ImageTarget::Remote("data:image/png;base64,AAAA".to_string())
    );
    // A markdown destination is relative to the working directory.
    let relative = resolve_image_target("screenshots/login.png");
    assert!(
        matches!(relative, ImageTarget::Local(ref path) if path.ends_with("screenshots/login.png")),
        "a markdown path resolves to a local file: {relative:?}"
    );
}
