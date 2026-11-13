use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::SizeConstraint;
use crate::elements::chat_content::ChatMessage;
use crate::elements::chat_content::{ChatAction, ChatFragment, ChatRole};
use crate::elements::{AppContext, LayoutContext, PaintContext};
use crate::event::DispatchedEvent;
use crate::geometry::vec2f;
use crate::render::{RenderCommand, Renderer};
use super::*;
use super::super::{ChatMessageBubble};

#[test]
fn action_click_fires_callback() {
    let app = AppContext::default();
    let action = ChatAction::Custom("test".to_string());
    let triggered = Rc::new(RefCell::new(None));
    let triggered_clone = triggered.clone();
    let mut bubble = ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::action("Run", action.clone())],
    )
    .with_on_action(move |a| *triggered_clone.borrow_mut() = Some(a));

    bubble.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    bubble.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

    let mut event_ctx = crate::elements::EventContext::default();
    let down = DispatchedEvent::MouseDown {
        position: vec2f(20.0, 20.0),
        button: 0,
    };
    let up = DispatchedEvent::MouseUp {
        position: vec2f(20.0, 20.0),
        button: 0,
    };

    assert!(bubble.dispatch_event(&down, &mut event_ctx, &app));
    assert!(bubble.dispatch_event(&up, &mut event_ctx, &app));
    assert_eq!(triggered.borrow().as_ref(), Some(&action));
}

/// R3: a Markdown link inside a paragraph is one live action target that
/// carries its URL — clicking the link text fires exactly one `OpenUrl` for
/// it, and clicking the prose around it fires nothing.
#[test]
fn link_fragment_becomes_interactive_action() {
    let app = AppContext::default();
    let message = ChatMessage::from_markdown(
        ChatRole::Assistant,
        "see [Goble](https://goble.dev) for details",
    );
    let fired = Rc::new(RefCell::new(Vec::<ChatAction>::new()));
    let recorder = fired.clone();
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments)
        .with_on_action(move |action| recorder.borrow_mut().push(action));

    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();

    // Click inside the drawn link run, not on the prose before or after it.
    let link = run_origin(&commands, "Goble");
    click_at(&mut bubble, &app, link + vec2f(2.0, 4.0));
    assert_eq!(
        *fired.borrow(),
        vec![ChatAction::OpenUrl("https://goble.dev".to_string())],
        "a link inside a paragraph must be one live action target carrying its URL"
    );

    // The prose run keeps the space that separates it from the link.
    let prose = run_origin(&commands, "see ");
    click_at(&mut bubble, &app, prose + vec2f(2.0, 4.0));
    assert_eq!(
        fired.borrow().len(),
        1,
        "only the link, not the paragraph around it, is a target"
    );
}

/// A file written as inline code is a mention: the run leads with its file
/// type's icon, and a click asks the app to open that path.
#[test]
fn a_file_mention_opens_the_file() {
    let app = AppContext::default();
    let message = ChatMessage::from_markdown(
        ChatRole::Assistant,
        "See `crates/goble-ui/src/lib.rs:42` for the seam",
    );
    let fired = Rc::new(RefCell::new(Vec::<ChatAction>::new()));
    let recorder = fired.clone();
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments)
        .with_on_action(move |action| recorder.borrow_mut().push(action));

    let commands = paint_bubble_at(&mut bubble, &app, 600.0);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "file-rust"
        )),
        "the mention leads with its file type's icon, got {commands:?}"
    );

    let run = run_origin(&commands, "crates/goble-ui/src/lib.rs:42");
    click_at(&mut bubble, &app, run + vec2f(6.0, 4.0));
    assert_eq!(
        *fired.borrow(),
        vec![ChatAction::OpenFile(
            "crates/goble-ui/src/lib.rs".to_string()
        )],
        "a mention opens the path it names, without the line suffix"
    );
}

/// The table's copy control sits under the grid, and a click hands the app the
/// rows as CSV.
#[test]
fn a_table_copy_control_copies_csv() {
    let app = AppContext::default();
    let message = ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::table(
            vec!["File".to_string(), "Notes".to_string()],
            vec![vec!["a.rs".to_string(), "one, \"two\"".to_string()]],
        )],
    );
    let fired = Rc::new(RefCell::new(Vec::<ChatAction>::new()));
    let recorder = fired.clone();
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments)
        .with_on_action(move |action| recorder.borrow_mut().push(action));

    let commands = paint_bubble_at(&mut bubble, &app, 600.0);
    let (origin, size) = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon { origin, name, size, .. } if name == "copy" => {
                Some((*origin, *size))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the table draws a copy control, got {commands:?}"));

    click_at(&mut bubble, &app, origin + vec2f(size / 2.0, size / 2.0));
    assert_eq!(
        *fired.borrow(),
        vec![ChatAction::CopyText(
            "File,Notes\na.rs,\"one, \"\"two\"\"\"".to_string()
        )],
        "the control copies the table's rows as CSV"
    );
}
