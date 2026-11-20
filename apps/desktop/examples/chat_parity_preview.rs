//! Headless preview of the chat transcript's DSH parity: plain nested tool rows
//! (no icons or glyphs), markdown headings/lists/code/links, and the closing
//! changed-files card with its Review action.
//!
//! Usage: cargo run -p goble-app --example chat_parity_preview
//! Output: /tmp/goble-work/out/chat-parity-{dark,light}.png

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use goble_core::harness::ToolCallStatus;
use goble_ui::elements::chat_content::{tool_fold_key, ToolCall, ToolDisplayMode};
use goble_ui::elements::{AppContext, Element};
use goble_ui::theme::Theme;
use goble_ui::{vec2f, ChatFragment, ChatMessage, ChatRole, ChatView};

fn call(id: &str, name: &str, arguments: &str, result: &str, diff: Option<&str>) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
        status: ToolCallStatus::Finished,
        result: Some(result.to_string()),
        diff: diff.map(str::to_string),
    }
}

fn messages() -> Vec<ChatMessage> {
    let read = call(
        "c1",
        "read_file",
        r#"{"path":"src/panes.rs"}"#,
        "fn build() {\n    let panes = build_panes(state);\n}\n",
        None,
    );
    let read2 = call(
        "c1b",
        "read_file",
        r#"{"path":"src/lane.rs"}"#,
        "fn lane() {}\n",
        None,
    );
    let edit = call(
        "c2",
        "edit_file",
        r#"{"path":"src/panes.rs"}"#,
        "ok",
        Some(
            "--- a/src/panes.rs\n+++ b/src/panes.rs\n@@ -40,6 +40,8 @@\n let panes = build_panes(state);\n for pane in panes {\n-    draw_old(pane);\n+    draw_new(pane);\n+    mark_visible(pane);\n }\n-old_footer();\n+new_footer();\n",
        ),
    );
    let mut extra = Vec::new();
    for index in 2..7 {
        extra.push(call(
            &format!("c{index}x"),
            "edit_file",
            &format!(r#"{{"path":"docs/file-{index}.md"}}"#),
            "ok",
            Some(&format!(
                "--- a/docs/file-{index}.md\n+++ b/docs/file-{index}.md\n@@ -1 +1,2 @@\n-old\n+new\n+more\n"
            )),
        ));
    }
    let run = call(
        "c3",
        "run_command",
        r#"{"command":"cargo test -p goble-ui"}"#,
        "test result: ok. 926 passed; 0 failed",
        None,
    );
    vec![
        ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Make the panes DSH-like.")]),
        ChatMessage::from_markdown(
            ChatRole::Assistant,
            "## What changed\n\nThe pane rows now follow DSH's nested text model:\n\n- **Tool calls** are plain rows, with no icon or glyph.\n- The diff closes the turn in a card, and `Review` opens it.\n\n```rust\nlet rows = plain_rows(&calls);\n```\n\nSee [the pane guide](https://example.com/panes) for the token map.",
        )
        .with_tool_calls({
            let mut calls = vec![read, read2, edit, run];
            calls.extend(extra);
            calls
        }),
    ]
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
        let calls = messages().pop().unwrap().tool_calls;
        // Open the first run, so the preview shows the nested member rows and
        // their bodies; the group key is the first call's own fold key.
        let key = format!("verb-group:{}", tool_fold_key(&calls[0], 0));
        let key2 = format!("verb-group:{}", tool_fold_key(&calls[1], 1));
        let fold = HashMap::from([
            (key, ToolDisplayMode::Expanded),
            (key2, ToolDisplayMode::Expanded),
            (tool_fold_key(&calls[0], 0), ToolDisplayMode::Expanded),
            (tool_fold_key(&calls[1], 1), ToolDisplayMode::Expanded),
        ]);
        let mut view = ChatView::new()
            .without_composer()
            .with_messages(messages())
            .with_tool_fold(Rc::new(RefCell::new(fold)))
            .with_tool_hover(Rc::new(RefCell::new(None)))
            .with_on_review_changes(|| {})
            .finish();
        let path = dir.join(format!("chat-parity-{label}.png"));
        raster::render_element_to_png(&assets, &app, &mut view, 920.0, 980.0, 2.0, None, &path)?;
        println!("wrote {}", path.display());

        let open = std::rc::Rc::new(std::cell::RefCell::new(true));
        let mut expanded = ChatView::new()
            .without_composer()
            .with_messages(messages())
            .with_tool_fold(Rc::new(RefCell::new(HashMap::new())))
            .with_tool_hover(Rc::new(RefCell::new(None)))
            .with_changed_files_open(open)
            .with_on_review_changes(|| {})
            .finish();
        let path = dir.join(format!("chat-parity-card-open-{label}.png"));
        raster::render_element_to_png(
            &assets, &app, &mut expanded, 920.0, 1100.0, 2.0, None, &path,
        )?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
