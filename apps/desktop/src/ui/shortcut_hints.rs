//! The instructions the rich input shows above its editor.
//!
//! The vocabulary is grok-build's: its TUI draws a contextual shortcuts bar
//! (`views/shortcuts_bar.rs`, fed by `views/agent.rs::build_hints`) and the
//! entries below are that prompt-focused set, worded as grok-build words it —
//! "send", "tasks". The list is the GUI's own, though: a hint is kept only
//! where this app binds the chord it names, so the strip never promises a key
//! nothing answers. grok-build's "shell" (`!`), "new conversation" (`⌘↵`) and
//! "commands" (`⌘K`) are not shown: the agent input no longer promises those
//! gestures, and warp-new's cloud gesture (`⌘⌥↵`) takes their place. Its
//! "newline", "mode", "cancel", "yolo", "todos", "sessions", "stash" and
//! "multiline" have no binding here either.
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
        // warp-new's cloud gesture: `⌘⌥⏎` submits the draft to the cloud agent
        // instead of the local one (its `ENTER_CLOUD_AGENT_VIEW_...` chord,
        // worded "submit to cloud agent").
        ShortcutHint::new(&["⌘", "⌥", "↵"], "send to cloud"),
        ShortcutHint::new(&["⌘", "⇧", "W"], "tasks"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every hint names a chord the app really binds: `⌘⇧W` is the workspace's
    /// own chord, while `↵` and `⌘⌥↵` are the composer's.
    #[test]
    fn the_hints_are_the_gui_s_own_bindings() {
        let idle = agent_rich_input_hints(false);
        let labels: Vec<&str> = idle.iter().map(|hint| hint.label()).collect();
        assert_eq!(labels, vec!["send", "send to cloud", "tasks"]);
        let caps: Vec<Vec<String>> = idle.iter().map(|hint| hint.keys().to_vec()).collect();
        assert_eq!(
            caps,
            vec![
                vec!["↵".to_string()],
                vec!["⌘".to_string(), "⌥".to_string(), "↵".to_string()],
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
    use goble_ui::geometry::{vec2f, RectF};
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
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.workflow_views.clear();
        }
        (Box::new(root), state, dir)
    }

    /// One frame through the whole app, returning what it painted.
    fn frame(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        let _ = root.layout(
            SizeConstraint::loose(vec2f(1024.0, 768.0)),
            &mut LayoutContext,
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
                RenderCommand::DrawText {
                    text: run, origin, ..
                } if run == text => Some(origin.y),
                _ => None,
            })
            .collect()
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

    /// The floating rich-input card: the borderless card is a rounded
    /// `Surface` fill, so its rect is what locates the input block now that the
    /// old separator over it is gone.
    fn composer_card(commands: &[RenderCommand], app: &AppContext) -> RectF {
        let surface = app.theme.color(goble_ui::theme::ColorToken::Surface);
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == surface
                    && (*corner_radius - goble_ui::elements::COMPOSER_CARD_RADIUS).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the composer draws its floating card")
    }

    /// 302: the chat composer draws no shortcut strip.
    #[test]
    fn the_agent_composer_draws_no_shortcut_strip() {
        let (mut root, _state, _dir) = workspace();
        let app = AppContext::default();
        let commands = frame(&mut root, &app);
        for label in ["send", "send to cloud", "tasks"] {
            assert!(
                lines_of(&commands, label).is_empty(),
                "the shortcut strip label {label:?} is gone"
            );
        }
        let drawn_icons: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        for icon in ["key-command", "key-option", "key-return", "key-shift"] {
            assert!(
                !drawn_icons.contains(&icon),
                "the shortcut key cap {icon:?} is gone"
            );
        }
    }
}
