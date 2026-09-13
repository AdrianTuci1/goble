//! The instructions the rich input shows above its editor.
//!
//! The vocabulary is grok-build's: its TUI draws a contextual shortcuts bar
//! (`views/shortcuts_bar.rs`, fed by `views/agent.rs::build_hints`) and the
//! entries below are that prompt-focused set, worded as grok-build words it —
//! "send", "shell", "tasks". The list is the GUI's own, though: a hint is kept
//! only where this app binds the chord it names, so the strip never promises a
//! key nothing answers. grok-build's "newline", "mode", "cancel", "yolo",
//! "todos", "queue", "sessions", "stash" and "multiline" have no binding here
//! and are therefore not shown.
//!
//! The shape the strip draws (a cap per key, then the name) is warp-new's
//! shortcuts view; see [`goble_ui::elements::ShortcutHints`].

use goble_ui::elements::ShortcutHint;

/// The agent rich input's instructions: the gestures this pane's composer
/// answers, plus the two chords the workspace answers wherever the keyboard is.
///
/// `agent_busy` is the pane's live turn state. grok-build relabels its send
/// hint while a turn runs — the same key queues a follow-up instead of starting
/// a turn — and this composer queues the same way.
pub(crate) fn agent_rich_input_hints(agent_busy: bool) -> Vec<ShortcutHint> {
    vec![
        ShortcutHint::new(&["↵"], if agent_busy { "queue" } else { "send" }),
        ShortcutHint::new(&["⌘", "↵"], "new conversation"),
        // grok-build's BashMode: a leading `!` forces a shell command instead of
        // a prompt. The composer classifies the draft the same way.
        ShortcutHint::new(&["!"], "shell"),
        ShortcutHint::new(&["⌘", "K"], "commands"),
        ShortcutHint::new(&["⌘", "⇧", "W"], "tasks"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every hint names a chord the app really binds: `⌘K` and `⌘⇧W` are the
    /// workspace's two global chords, `↵` and `⌘↵` are the composer's own, and
    /// `!` is the shell prefix the composer classifies.
    #[test]
    fn the_hints_are_the_gui_s_own_bindings() {
        let idle = agent_rich_input_hints(false);
        let labels: Vec<&str> = idle.iter().map(|hint| hint.label()).collect();
        assert_eq!(
            labels,
            vec!["send", "new conversation", "shell", "commands", "tasks"]
        );
        let caps: Vec<Vec<String>> = idle.iter().map(|hint| hint.keys().to_vec()).collect();
        assert_eq!(
            caps,
            vec![
                vec!["↵".to_string()],
                vec!["⌘".to_string(), "↵".to_string()],
                vec!["!".to_string()],
                vec!["⌘".to_string(), "K".to_string()],
                vec!["⌘".to_string(), "⇧".to_string(), "W".to_string()],
            ]
        );

        // A running turn turns the same key into a queue, and only that entry
        // changes.
        let busy = agent_rich_input_hints(true);
        assert_eq!(busy[0].label(), "queue");
        assert_eq!(busy[1..], idle[1..]);
    }
}

/// The strip in the real tree: the agent pane's rich input draws grok-build's
/// instructions between the transcript and the input's separator line — above
/// the context pills and the editor — and a running turn turns the send hint
/// into a queue.
#[cfg(test)]
mod rich_input_tests {
    use crate::root_view::RootView;
    use crate::state::UiState;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::geometry::vec2f;
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::{AppContext, Element, LayoutContext, PaintContext, SizeConstraint};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    /// A whole app tree over a real store, with the workspace mounted (no
    /// overlay, no start chooser) so a pane's rich input is on screen.
    fn workspace() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let root = RootView::new(&AppContext::default(), &desktop, None);
        let state = root.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.settings_overlay_open = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.task_workflow_open = false;
        }
        (Box::new(root), state, dir)
    }

    /// One frame through the whole app, returning what it painted.
    fn frame(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        let _ = root.layout(
            SizeConstraint::loose(vec2f(1024.0, 768.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// The lines a run was painted on. A run can appear more than once in a
    /// frame, so every line is returned.
    fn lines_of(commands: &[RenderCommand], text: &str) -> Vec<f32> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text: run, origin, .. } if run == text => Some(origin.y),
                _ => None,
            })
            .collect()
    }

    /// Every run of `text` sits over `ceiling`.
    fn above(commands: &[RenderCommand], text: &str, ceiling: f32, case: &str) {
        let lines = lines_of(commands, text);
        assert!(!lines.is_empty(), "{case}: {text:?} is drawn");
        assert!(
            lines.iter().all(|line| *line < ceiling),
            "{case}: {text:?} is drawn above {ceiling} (found {lines:?})"
        );
    }

    /// The thin horizontal rules in a frame — the pane's separators, in a
    /// widget-free pane, nothing else.
    fn rules(commands: &[RenderCommand]) -> Vec<f32> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. }
                    if rect.height() <= 2.0 && rect.width() > 200.0 =>
                {
                    Some(rect.min_y())
                }
                _ => None,
            })
            .collect()
    }

    /// The agent view's instructions are the pane's own strip over the input's
    /// separator: the last content row, under the transcript, over the rule and
    /// the rich input under it (the warp-new order, where the shortcuts view
    /// sits between the conversation and the input it describes). Nothing about
    /// the input is lifted when a pane switches between the shell and its agent
    /// view — the separator above it is its top edge on both surfaces.
    #[test]
    fn the_agent_strip_sits_over_the_inputs_separator() {
        let (mut root, state, _dir) = workspace();
        let app = AppContext::default();
        let commands = frame(&mut root, &app);

        // The editor is on screen, so the strip's place over it means
        // something: the same frame drew the rich input's placeholder.
        let placeholder = lines_of(&commands, "Ask anything...");
        let editor = *placeholder.first().expect("the rich input is drawn");
        let case = "idle";

        for label in ["send", "new conversation", "shell", "commands", "tasks"] {
            above(&commands, label, editor, case);
        }
        // Every key of every chord has its own cap on the strip's own line. A
        // key the bundled text faces lack is drawn from the icon set, so no
        // strip ever paints the font's `.notdef` box for it; the ASCII keys keep
        // their own glyph.
        let drawn_icons: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        for key in ["W", "!"] {
            above(&commands, key, editor, case);
        }
        for icon in ["key-command", "key-shift", "key-return"] {
            assert!(
                drawn_icons.contains(&icon),
                "{case}: the {icon} cap is drawn"
            );
        }
        for symbol in ["⌘", "⇧", "↵"] {
            assert!(
                lines_of(&commands, symbol).is_empty(),
                "{case}: {symbol:?} is never drawn as a character"
            );
        }

        // The separator is the input's top edge, under the strip: the nearest
        // rule under the instructions, not a heading far above them.
        let strip_bottom = ["send", "new conversation", "shell", "commands", "tasks"]
            .iter()
            .flat_map(|label| lines_of(&commands, label))
            .fold(f32::NEG_INFINITY, f32::max);
        let rule = rules(&commands)
            .into_iter()
            .find(|y| *y > strip_bottom && *y - strip_bottom < 60.0)
            .unwrap_or_else(|| {
                panic!(
                    "{case}: a separator under the strip (strip at {strip_bottom}, rules {:?})",
                    rules(&commands)
                )
            });
        assert!(rule > strip_bottom, "{case}: the strip is over the rule");

        // The context pills are the row under the instruction strip, inside the
        // input and over the editor, the way the shell bar keeps the directory
        // and the branch over the draft.
        let pills = lines_of(&commands, "~/Projects/goble/app");
        assert!(!pills.is_empty(), "{case}: the context pills are drawn");
        assert!(
            pills.iter().all(|line| *line < editor),
            "{case}: the pills sit above the editor (found {pills:?} against {editor})"
        );
        let pills_top = pills.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(
            rule < pills_top,
            "{case}: the pills are inside the input, under its separator (rule {rule}, pills {pills_top})"
        );

        // A running turn: the same key queues instead of sending, and the other
        // entries are untouched.
        let pane_id = state.borrow().active_pane_id;
        state
            .borrow_mut()
            .pane_runtime
            .entry(pane_id)
            .or_default()
            .busy = true;
        let commands = frame(&mut root, &app);
        let case = "turn running";
        assert!(
            lines_of(&commands, "send").is_empty(),
            "{case}: the send hint is gone"
        );
        let queue = lines_of(&commands, "queue");
        assert!(!queue.is_empty(), "{case}: the queue hint replaced it");
        for label in ["new conversation", "shell", "commands", "tasks"] {
            above(&commands, label, editor, case);
        }
        let running_strip_bottom = ["queue", "new conversation", "shell", "commands", "tasks"]
            .iter()
            .flat_map(|label| lines_of(&commands, label))
            .fold(f32::NEG_INFINITY, f32::max);
        let running_rule = rules(&commands)
            .into_iter()
            .find(|y| *y > running_strip_bottom && *y - running_strip_bottom < 60.0)
            .unwrap_or_else(|| panic!("{case}: a separator under the strip"));
        assert!(running_rule > running_strip_bottom, "{case}: the strip is over the rule");
    }
}
