//! Headless PNG preview of the chat transcript's DSH-parity typography.
//!
//! Renders the real `ChatView` element tree — assistant prose, the user's own
//! row, a reasoning disclosure, a folded tool run with an expanded body, a
//! fenced code card, a notice, the work dock's announcement card, the queued
//! prompt band, the failed-turn block and the footer pills — through the
//! reusable preview rasterizer in `composer_preview/raster.rs`, in both the
//! dark and the light theme.
//!
//! ```text
//! cargo run -p goble-ui --example chat_typography_preview -- \
//!     --out /tmp/goble-work/out/chat-typography
//! ```
//!
//! The `--out` stem gets `-dark.png` / `-light.png` and `-dark-2x.png` /
//! `-light-2x.png` (a 2x pass for letterform inspection).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use goble_core::harness::ToolCallStatus;
use goble_ui::elements::chat_content::{
    ChatFragment, ChatMessage, ChatRole, SubAgentRow, SubAgentRowStatus, ToolCall, ToolDisplayMode,
    WorkDock, WorkGroup, WorkRow, WorkSection,
};
use goble_ui::elements::{AppContext, Element, HoverChipLayer, ScrollState, Stack};
use goble_ui::geometry::Vector2F;
use goble_ui::theme::Theme;
use goble_ui::{tool_fold_key, ChatView, TokenUsage};

#[path = "composer_preview/raster.rs"]
mod raster;

use raster::PreviewAssets;

const WIDTH: f32 = 860.0;
const HEIGHT: f32 = 1500.0;

fn assets_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

fn tool_call(name: &str, arguments: &str, result: &str, status: ToolCallStatus) -> ToolCall {
    ToolCall {
        id: format!("call_{name}"),
        name: name.to_string(),
        arguments: arguments.to_string(),
        status,
        result: Some(result.to_string()),
        diff: None,
    }
}

fn sub_agent_row() -> SubAgentRow {
    SubAgentRow {
        child_id: "conv-child-1".to_string(),
        subagent_type: "reviewer".to_string(),
        description: "audit the migration".to_string(),
        status: SubAgentRowStatus::Completed,
        activity: String::new(),
        elapsed: std::time::Duration::from_millis(43_000),
        turns: 2,
        tool_calls: 4,
        tokens: 1234,
        outcome: None,
        background: false,
    }
}

/// The transcript scene: every typographic surface the pass touched, in the
/// order a real conversation draws them.
fn scene() -> Box<dyn Element> {
    let read = tool_call(
        "read_file",
        r#"{"path":"src/sidebar.rs"}"#,
        "1  fn build() {}\n2  let rows = 4;\n3  fn frame() {}\n",
        ToolCallStatus::Finished,
    );
    let edit = tool_call(
        "edit_file",
        r#"{"path":"src/panes.rs","old_text":"a","new_text":"b"}"#,
        "edited",
        ToolCallStatus::Finished,
    );
    let failed = tool_call(
        "run_command",
        r#"{"command":"cargo test -p goble-ui"}"#,
        "error: 3 tests failed",
        ToolCallStatus::Error,
    );

    // The read/edit run starts folded so its header (■ DSH's aggregated label)
    // is what the transcript shows.
    let mut fold: HashMap<String, ToolDisplayMode> = HashMap::new();
    for (index, call) in [&read, &edit].iter().enumerate() {
        fold.insert(tool_fold_key(call, index), ToolDisplayMode::Collapsed);
    }
    fold.insert(
        tool_fold_key(&failed, 2),
        ToolDisplayMode::Expanded,
    );

    let messages = vec![
        ChatMessage::new(
            ChatRole::User,
            vec![ChatFragment::text(
                "The transcript's rows should read like DeepSeek Harness. Check the sidebar and the panes, then tell me what changed.",
            )],
        ),
        ChatMessage::from_markdown(
            ChatRole::Assistant,
            "## What changed\n\nThe row heights and inks now follow DSH's token sheet.\n\n- Assistant prose is 14 / 24\n- The user's row is 14 / 22\n- Process rows are 13 / 24\n\nInline `cargo test` stays on the quiet band.\n\n```rust\nlet rows = 4;\n```\n\n| Surface | Size |\n|---|---|\n| prose | 14 / 24 |\n| row | 13 / 24 |\n",
        ),
        ChatMessage::new(
            ChatRole::Assistant,
            vec![
                ChatFragment::reasoning(
                    "c1:0",
                    "done",
                    "Weighing the row grid and the announcement card.",
                    true,
                ),
                ChatFragment::notice("Context 82% full. Compacting…"),
            ],
        ),
        ChatMessage::new(ChatRole::Assistant, Vec::new())
            .with_tool_calls(vec![read, edit, failed]),
    ];

    let dock = WorkDock::new(vec![WorkGroup::new(
        WorkSection::Subagents,
        vec![WorkRow::new("c1", "Subagent", "audit the migration").with_meta("43s")],
        true,
    )])
    .finish();

    ChatView::new()
        .without_composer()
        .with_scroll_state(Rc::new(RefCell::new(ScrollState::default())))
        .with_work_dock(Some(dock))
        .with_messages(messages)
        .with_tool_fold(Rc::new(RefCell::new(fold)))
        .with_tool_hover(Rc::new(RefCell::new(None)))
        .with_reasoning_expanded(Rc::new(RefCell::new(HashMap::from([(
            "c1:0".to_string(),
            true,
        )]))))
        .with_sub_agents(HashMap::from([(
            "call_run_agent".to_string(),
            sub_agent_row(),
        )]))
        .with_queued_prompt(Some("then run the linter".to_string()))
        .with_turn_error(Some(
            "LLM request failed: 400 Bad Request {\"error\":{\"message\":\"The reasoning_content in the assistant message is not supported\"}}"
                .to_string(),
        ))
        .with_usage(TokenUsage {
            input: 12_345,
            cached: Some(4_000),
            output: 2_100,
        })
        .with_usage_detail(
            Some(TokenUsage {
                input: 7_200,
                cached: Some(5_000),
                output: 640,
            }),
            5,
            Some(256_000),
        )
        // The usage dialog open, so the preview shows the DSH stat-dialog
        // anatomy the owner asked for, not only the footer pill.
        .with_usage_open(Rc::new(RefCell::new(true)))
        .finish()
}

/// The transcript with the frame's last layer over it: the usage popover is
/// queued during the transcript's paint, and the layer draws it above the
/// scrollport (the app root does the same).
fn scene_with_usage_layer() -> Box<dyn Element> {
    Stack::new()
        .with_children(vec![scene()])
        .with_overlay(HoverChipLayer::new().finish(), Vector2F::zero())
        .finish()
}

fn main() -> anyhow::Result<()> {
    // `--out` names either a directory (the PNGs land in it, as
    // `chat-typography-dark.png` and so on) or a file stem (`<stem>-dark.png`).
    let mut out = PathBuf::from("/tmp/goble-work/out/chat-typography");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--out" {
            if let Some(value) = args.next() {
                out = PathBuf::from(value);
            }
        }
    }
    let out_is_dir = out.extension().is_none() || out.is_dir();
    let dir = if out_is_dir {
        out.clone()
    } else {
        out.parent().map(Path::to_path_buf).unwrap_or_default()
    };
    std::fs::create_dir_all(&dir)?;
    let stem = if out_is_dir {
        "chat-typography".to_string()
    } else {
        out.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("chat-typography")
            .to_string()
    };

    let assets = PreviewAssets::load(&assets_dir())?;
    for (label, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let app = AppContext {
            theme,
            ..AppContext::default()
        };
        for scale in [1.0f32, 2.0] {
            let suffix = if scale > 1.0 {
                format!("{label}-2x.png")
            } else {
                format!("{label}.png")
            };
            let path = dir.join(format!("{stem}-{suffix}"));
            raster::render_element_to_png(
                &assets,
                &app,
                &mut scene_with_usage_layer(),
                WIDTH,
                HEIGHT,
                scale,
                None,
                &path,
            )?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
}
