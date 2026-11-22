//! A file row/card's context menu, opened by a right click at the pointer.
//!
//! The rows are the file actions the explorer and the review cards share:
//! Open, Open in floating panel, Copy path, Revert (only when a real git
//! backend resolves the file) and Open in editor. The panel is the app's own
//! [`PopupMenu`] — the same surface, rows, keyboard highlight and
//! close-on-a-press-outside every other menu of the shell uses — hung from the
//! pointer by a [`PointerMenu`].
//!
//! The one open target lives in app state ([`crate::state::FileMenu`]) because
//! the tree is rebuilt every frame; this module only builds the frame's panel
//! and the wrapper a row uses to report a secondary press.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Element, Empty, EventContext, LayoutContext, PaintContext, Point, PointerMenu,
    PopupMenu, PopupMenuItem, SizeConstraint,
};
use goble_ui::event::{DispatchedEvent, BUTTON_SECONDARY};
use goble_ui::geometry::{rectf, Vector2F};

use super::UiActions;
use crate::state::FileMenu;

/// One action a file's context menu can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileMenuAction {
    /// Open the file in a docked file pane.
    Open,
    /// Open the file as one window over the current layout.
    OpenFloating,
    /// Copy the path shown for the file.
    CopyPath,
    /// Restore the file from `HEAD` (or remove an untracked one). Offered only
    /// when the real git backend resolved the file.
    Revert,
    /// Hand the file to the platform's editor.
    OpenEditor,
}

/// The rows the menu draws, in order, as `(label, action)`. The list is one
/// value so a row and the action choosing it cannot drift apart, and the
/// Revert row exists only for a real backend.
pub(crate) fn rows(target: &FileMenu) -> Vec<(&'static str, FileMenuAction)> {
    let mut rows = vec![
        ("Open", FileMenuAction::Open),
        ("Open in floating panel", FileMenuAction::OpenFloating),
        ("Copy path", FileMenuAction::CopyPath),
    ];
    if target.revert.is_some() {
        rows.push(("Revert", FileMenuAction::Revert));
    }
    rows.push(("Open in editor", FileMenuAction::OpenEditor));
    rows
}

/// Whether a row is enabled for this target.
fn enabled(target: &FileMenu, action: FileMenuAction) -> bool {
    match action {
        FileMenuAction::Open | FileMenuAction::OpenFloating | FileMenuAction::OpenEditor => {
            Path::new(&target.path).exists()
        }
        FileMenuAction::CopyPath | FileMenuAction::Revert => true,
    }
}

/// Build the menu the open target names, hung from the point the right click
/// landed on. The app owns the target, the action and the close callback,
/// because an element cannot remember that it was open across frames.
pub(crate) fn build_file_menu(target: &FileMenu, actions: &UiActions) -> Box<dyn Element> {
    let rows = rows(target);
    let items = rows
        .iter()
        .map(|(label, action)| {
            let mut item = PopupMenuItem::new(*label);
            item = match action {
                FileMenuAction::Open => item.with_icon("file"),
                FileMenuAction::OpenFloating => item.with_icon("fullscreen-enter"),
                FileMenuAction::CopyPath => item.with_icon("copy"),
                FileMenuAction::Revert => item.with_icon("undo"),
                FileMenuAction::OpenEditor => item.with_icon("maximize-01"),
            };
            if !enabled(target, *action) {
                item = item.disabled();
            }
            item
        })
        .collect();
    let on_action = actions.on_file_menu_action.clone();
    let chosen_rows = rows.clone();
    let menu = PopupMenu::new(Box::new(Empty::new()), items)
        // The panel lives in the frame that the app's target opened; the app's
        // own target is what the next frame reads.
        .with_open(Rc::new(RefCell::new(true)))
        // A menu of the window like any other: the pointer over its panel
        // belongs to it, Escape/outside close it, and opening it puts any other
        // menu away.
        .with_menu_id("file.menu")
        .with_on_select(move |row| {
            if let Some((_, action)) = chosen_rows.get(row) {
                (on_action.borrow_mut())(*action);
            }
        })
        .with_on_close({
            let on_close = actions.on_file_menu_close.clone();
            move || (on_close.borrow_mut())()
        })
        .finish();
    Box::new(PointerMenu::new(menu, target.at))
}

/// Wrap a file row or card so a secondary press reports the point it landed on
/// and opens the menu, while every other event reaches the content unchanged.
pub(crate) fn context_click(
    content: Box<dyn Element>,
    on_context: Rc<RefCell<dyn FnMut(Vector2F)>>,
) -> Box<dyn Element> {
    Box::new(ContextClick {
        content,
        on_context,
        size: None,
        origin: None,
    })
}

struct ContextClick {
    content: Box<dyn Element>,
    on_context: Rc<RefCell<dyn FnMut(Vector2F)>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ContextClick {
    fn bounds(&self) -> Option<goble_ui::geometry::RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }
}

impl Element for ContextClick {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.content.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.content.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if let DispatchedEvent::MouseDown { position, button } = event {
            if *button == BUTTON_SECONDARY
                && self
                    .bounds()
                    .is_some_and(|bounds| contains(bounds, *position))
            {
                (self.on_context.borrow_mut())(*position);
                // The right press opens the menu; it is not also a click on
                // the row or card behind it.
                return true;
            }
        }
        self.content.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FileMenu, FileRevertTarget};
    use goble_ui::elements::Text;
    use goble_ui::event::ModifiersState;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;

    fn target(exists: bool, revert: bool) -> (FileMenu, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("main.rs");
        if exists {
            std::fs::write(&path, "fn main() {}").expect("file");
        }
        (
            FileMenu {
                path: path.to_string_lossy().to_string(),
                copy_path: "src/main.rs".to_string(),
                at: vec2f(120.0, 80.0),
                revert: revert.then(|| FileRevertTarget {
                    repo: dir.path().to_string_lossy().to_string(),
                    git_path: "src/main.rs".to_string(),
                }),
            },
            dir,
        )
    }

    fn render(element: &mut Box<dyn Element>) -> Vec<RenderCommand> {
        let app = AppContext::default();
        let _ = element.layout(
            SizeConstraint::loose(vec2f(800.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(goble_ui::render::Renderer::new());
        element.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    fn labels(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_lists_the_shared_file_actions() {
        let (without_revert_target, _dir) = target(true, false);
        let without_revert = rows(&without_revert_target);
        assert_eq!(
            without_revert
                .iter()
                .map(|(label, _)| *label)
                .collect::<Vec<_>>(),
            vec![
                "Open",
                "Open in floating panel",
                "Copy path",
                "Open in editor"
            ]
        );
        assert!(
            !without_revert
                .iter()
                .any(|(_, action)| *action == FileMenuAction::Revert),
            "without a real backend there is no Revert row"
        );
        let (with_revert_target, _dir) = target(true, true);
        assert_eq!(
            rows(&with_revert_target)
                .iter()
                .map(|(label, _)| *label)
                .collect::<Vec<_>>(),
            vec![
                "Open",
                "Open in floating panel",
                "Copy path",
                "Revert",
                "Open in editor"
            ],
            "a real backend adds Revert in place"
        );
    }

    #[test]
    fn a_deleted_file_disables_the_rows_that_need_it_on_disk() {
        let (target, _dir) = target(false, true);
        assert!(enabled(&target, FileMenuAction::CopyPath));
        assert!(enabled(&target, FileMenuAction::Revert));
        assert!(!enabled(&target, FileMenuAction::Open));
        assert!(!enabled(&target, FileMenuAction::OpenFloating));
        assert!(!enabled(&target, FileMenuAction::OpenEditor));
    }

    #[test]
    fn the_panel_draws_every_row() {
        let app = AppContext::default();
        let actions = crate::actions::make_actions(
            Rc::new(RefCell::new(crate::state::UiState::mock())),
            None,
            Rc::new(RefCell::new(crate::media::MediaState::mock())),
            app.window_control.clone(),
            Rc::new(RefCell::new(1.0)),
        );
        let (target, _dir) = target(true, true);
        let mut menu = build_file_menu(&target, &actions);
        let commands = render(&mut menu);
        let drawn = labels(&commands);
        for label in [
            "Open",
            "Open in floating panel",
            "Copy path",
            "Revert",
            "Open in editor",
        ] {
            assert!(
                drawn.iter().any(|text| text == label),
                "the panel draws {label:?}: {drawn:?}"
            );
        }
    }

    #[test]
    fn escape_and_an_outside_press_close_the_menu() {
        let app = AppContext::default();
        let (target, _dir) = target(true, false);
        let mut state = crate::state::UiState::mock();
        state.open_file_menu(target.path, "src/main.rs".to_string(), vec2f(40.0, 40.0));
        let state = Rc::new(RefCell::new(state));
        let actions = crate::actions::make_actions(
            state.clone(),
            None,
            Rc::new(RefCell::new(crate::media::MediaState::mock())),
            app.window_control.clone(),
            Rc::new(RefCell::new(1.0)),
        );
        let target = state.borrow().file_menu.borrow().clone().unwrap();
        let mut menu = build_file_menu(&target, &actions);
        let _ = render(&mut menu);

        let mut ctx = EventContext::default();
        let escape = DispatchedEvent::KeyDown {
            key: "Escape".to_string(),
            modifiers: ModifiersState::none(),
        };
        assert!(menu.dispatch_event(&escape, &mut ctx, &app));
        assert!(
            state.borrow().file_menu.borrow().is_none(),
            "Escape closes through the app-owned close callback"
        );
    }

    #[test]
    fn enter_runs_the_highlighted_action() {
        let app = AppContext::default();
        let (target, _dir) = target(true, false);
        let mut state = crate::state::UiState::mock();
        state.open_file_menu(target.path, "src/main.rs".to_string(), vec2f(40.0, 40.0));
        let state = Rc::new(RefCell::new(state));
        let actions = crate::actions::make_actions(
            state.clone(),
            None,
            Rc::new(RefCell::new(crate::media::MediaState::mock())),
            app.window_control.clone(),
            Rc::new(RefCell::new(1.0)),
        );
        let target = state.borrow().file_menu.borrow().clone().unwrap();
        let mut menu = build_file_menu(&target, &actions);
        let _ = render(&mut menu);

        let mut ctx = EventContext::default();
        let enter = DispatchedEvent::KeyDown {
            key: "Enter".to_string(),
            modifiers: ModifiersState::none(),
        };
        // The menu opens with the first row highlighted; Enter takes it.
        assert!(menu.dispatch_event(&enter, &mut ctx, &app));
        assert!(
            state.borrow().file_menu.borrow().is_none(),
            "taking a row dismisses the menu"
        );
        assert!(
            state
                .borrow()
                .spaces
                .iter()
                .any(|space| space.visible_leaf_ids().iter().any(|_| true)),
            "the action layer is alive"
        );
    }

    #[test]
    fn a_secondary_press_on_a_row_is_the_context_gesture() {
        let app = AppContext::default();
        let at = Rc::new(RefCell::new(None));
        let callback = Rc::clone(&at);
        let mut row = context_click(
            Text::new("src/main.rs").finish(),
            Rc::new(RefCell::new(move |position| {
                *callback.borrow_mut() = Some(position);
            })),
        );
        let _ = render(&mut row);
        let mut ctx = EventContext::default();
        let press = DispatchedEvent::MouseDown {
            position: vec2f(4.0, 4.0),
            button: BUTTON_SECONDARY,
        };
        assert!(row.dispatch_event(&press, &mut ctx, &app));
        assert_eq!(*at.borrow(), Some(vec2f(4.0, 4.0)));

        let primary = DispatchedEvent::MouseDown {
            position: vec2f(4.0, 4.0),
            button: 0,
        };
        *at.borrow_mut() = None;
        let _ = row.dispatch_event(&primary, &mut ctx, &app);
        assert!(
            at.borrow().is_none(),
            "a primary press belongs to the row, not the menu"
        );
    }
}
