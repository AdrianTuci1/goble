//! The window's single open-menu state: one menu at a time, and the surface
//! that belongs to it.
//!
//! Menus in this UI are immediate-mode elements: every frame rebuilds them, so
//! "which menu is open" cannot live on an element. It also cannot live on one
//! of the many independent open flags the app keeps (`sidebar_menu_open`,
//! `model_menu_open`, `inbox_open`…) because then the window has no one place
//! that knows a menu is up — and that is precisely what the owner's rule needs:
//! *all open menus must be recognised as open menus*, one at a time, so that
//! hovering the open menu does not affect anything behind it and a click
//! outside closes it.
//!
//! [`OpenMenus`] is that one place. It is an `Rc` handle carried by
//! [`AppContext`](crate::elements::AppContext), so every element and the root
//! view share the same state:
//!
//! * Opening a menu calls [`OpenMenus::open`], which runs the previously open
//!   menu's close handle first — opening another menu closes the previous one.
//! * A menu with its own open flag (the app-owned `Rc<RefCell<bool>>` controls,
//!   the composer's model card, a row's tray) reports itself and the rectangles
//!   its surface painted while it draws ([`OpenMenus::report`]). A menu that
//!   appeared without claiming (an action flipped its flag) is adopted here, so
//!   direct state changes are recognised too.
//! * The root view reads the surface and gives the menu the pointer over it:
//!   hover resolution ([`PaintContext::hovered`](crate::elements::PaintContext))
//!   returns false for anything painted outside the menu's surface while the
//!   pointer is on it, and the root view's own dispatch closes the menu on an
//!   outside primary press and on Escape.
//!
//! This is the native counterpart of DSH's menus: DSH renders a menu in a
//! portal above the page (`MenuSurface`/`Menu` in
//! `@deepseek-ai/dsh-client-ui-primitives`, the model menu's
//! `ModelSelect.module.css` `.menu` with `position: fixed; z-index: 1100`), so
//! the browser's topmost hit-testing sends the pointer to the menu and never to
//! the page behind it; a document-level `pointerdown` listener closes it when
//! the press lands outside, and Escape does the same. There is no backdrop, so
//! hovering somewhere else on the page still works — and this module keeps that
//! same rule.

use std::cell::RefCell;
use std::rc::Rc;

use crate::event::DispatchedEvent;
use crate::geometry::{RectF, Vector2F};

/// The identity a menu registers under: a stable label, both for diagnostics
/// and so the same menu rebuilt next frame can be recognised as the one open.
pub type MenuId = &'static str;

/// How a menu closes itself. Calling it clears whatever app state keeps the
/// menu open (an `Rc<RefCell<bool>>`, a row's tray cell, an action callback).
/// The root view and [`OpenMenus::open`] use it so dismissing a menu is one
/// path for every menu in the window.
pub type MenuClose = Rc<RefCell<dyn FnMut()>>;

/// The label a menu reports under when its builder was given no id. Kept so a
/// menu that never opted into a label still takes part in the shared state.
pub const DEFAULT_MENU_ID: MenuId = "menu";

/// How the open menu wants pointer moves treated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuPointer {
    /// The menu decides hover from the paint-time cursor (the default: the
    /// popup panel, a row's tray). While the pointer is on the menu's surface
    /// the root view keeps `MouseMove` away from everything behind it, exactly
    /// as the web's topmost hit-testing would.
    #[default]
    Paint,
    /// The menu reads `MouseMove` itself to move its highlight (the composer's
    /// model card and the slash card do this). Moves still reach it; the paint
    /// gate keeps the elements behind it from hovering.
    Events,
}

/// One menu as the registry holds it.
struct OpenMenu {
    id: MenuId,
    close: MenuClose,
    surfaces: Vec<RectF>,
    /// The menu's own trigger box, when it has one: a press on it toggles the
    /// menu through the element rather than reading as an outside dismiss.
    anchor: Option<RectF>,
    pointer: MenuPointer,
}

/// What one menu reported while it painted this frame.
struct MenuReport {
    id: MenuId,
    close: MenuClose,
    surfaces: Vec<RectF>,
    pointer: MenuPointer,
}

#[derive(Default)]
struct State {
    /// The menu the window considers open.
    open: Option<OpenMenu>,
    /// The menus that painted open during the current frame.
    reports: Vec<MenuReport>,
    /// The ids that painted open on the frame before, so a menu that appears
    /// without claiming can be told apart from one that was already up.
    last_ids: Vec<MenuId>,
}

/// The window's shared open-menu state. Cloned freely (it is one `Rc`), so the
/// app context, the root view and every menu element read and write the same
/// state.
#[derive(Clone, Default)]
pub struct OpenMenus {
    state: Rc<RefCell<State>>,
}

impl OpenMenus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a menu is open.
    pub fn is_open(&self) -> bool {
        self.state.borrow().open.is_some()
    }

    /// The id of the open menu, if any.
    pub fn id(&self) -> Option<MenuId> {
        self.state.borrow().open.as_ref().map(|open| open.id)
    }

    /// The rectangles the open menu painted on the last frame: its panel, its
    /// side tray and the corridor between them. Empty while nothing is open.
    pub fn surfaces(&self) -> Vec<RectF> {
        self.state
            .borrow()
            .open
            .as_ref()
            .map(|open| open.surfaces.clone())
            .unwrap_or_default()
    }

    /// Whether `position` is on the open menu's own surface — the menu's
    /// hit-testing region. A press here belongs to the menu; a press anywhere
    /// else closes it.
    pub fn pointer_on_surface(&self, position: Vector2F) -> bool {
        let state = self.state.borrow();
        state.open.as_ref().is_some_and(|open| {
            open.surfaces
                .iter()
                .any(|surface| contains(*surface, position))
        })
    }

    /// How the open menu wants pointer moves handled (see [`MenuPointer`]).
    pub fn pointer(&self) -> MenuPointer {
        self.state
            .borrow()
            .open
            .as_ref()
            .map(|open| open.pointer)
            .unwrap_or_default()
    }

    /// Open `id` as the one open menu: whatever was open is closed first, so a
    /// new menu always replaces the previous one. An element calls this from
    /// the press that opens it.
    pub fn open(&self, id: MenuId, close: MenuClose) {
        let previous = self.state.borrow_mut().open.take();
        if let Some(previous) = previous {
            (previous.close.borrow_mut())();
        }
        self.state.borrow_mut().open = Some(OpenMenu {
            id,
            close,
            surfaces: Vec::new(),
            anchor: None,
            pointer: MenuPointer::Paint,
        });
    }

    /// Close the open menu, if any, through its own close handle.
    pub fn close(&self) {
        let previous = self.state.borrow_mut().open.take();
        if let Some(previous) = previous {
            (previous.close.borrow_mut())();
        }
    }

    /// Clear the reports gathered by the last frame. Called by the root view
    /// before the tree paints.
    pub fn begin_paint(&self) {
        self.state.borrow_mut().reports.clear();
    }

    /// Report that `id` is painting its surface this frame. A menu that paints
    /// while open calls this once per surface rectangle it owns; the root view
    /// reconciles the reports after the tree has painted
    /// ([`OpenMenus::end_paint`]).
    pub fn report(
        &self,
        id: MenuId,
        close: MenuClose,
        surfaces: Vec<RectF>,
        pointer: MenuPointer,
    ) {
        self.state.borrow_mut().reports.push(MenuReport {
            id,
            close,
            surfaces,
            pointer,
        });
    }

    /// Reconcile what painted open this frame with the one open menu:
    ///
    /// * the standing menu adopts the surface it painted this frame; a menu
    ///   that painted nothing any more closed itself (a row taken, Escape, its
    ///   own flag cleared);
    /// * a menu that appeared open without claiming (an action flipped its
    ///   flag) becomes the one open menu — it is newer than the one standing —
    ///   and the standing one is closed;
    /// * any other menu still reporting itself open is a straggler from a
    ///   direct state change and is closed, so only one menu stays up.
    pub fn end_paint(&self) {
        let mut state = self.state.borrow_mut();
        let reports = std::mem::take(&mut state.reports);
        let last = std::mem::take(&mut state.last_ids);
        state.last_ids = reports.iter().map(|report| report.id).collect();

        let mut open = state.open.take();
        if let Some(standing) = open.as_mut() {
            match reports.iter().rev().find(|report| report.id == standing.id) {
                Some(report) => {
                    standing.surfaces = report.surfaces.clone();
                    standing.close = report.close.clone();
                    standing.pointer = report.pointer;
                }
                // The open menu painted nothing: it closed itself.
                None => open = None,
            }
        }

        // A menu that is open this frame but was not open last frame and is not
        // the one standing opened through a state change rather than a claim.
        // It is the newest, so it takes over.
        let standing_id = open.as_ref().map(|standing| standing.id);
        let newly_open = reports.iter().find(|report| {
            Some(report.id) != standing_id && !last.iter().any(|id| *id == report.id)
        });
        let mut replaced: Option<MenuId> = None;
        if let Some(report) = newly_open {
            if let Some(previous) = open.take() {
                replaced = Some(previous.id);
                (previous.close.borrow_mut())();
            }
            open = Some(OpenMenu {
                id: report.id,
                close: report.close.clone(),
                surfaces: report.surfaces.clone(),
                anchor: None,
                pointer: report.pointer,
            });
        }

        // Nothing standing but something painted open: adopt it, so a menu that
        // only ever appears through app state (the tab context menu, the slash
        // card) is recognised as the open menu too.
        if open.is_none() {
            if let Some(report) = reports.last() {
                open = Some(OpenMenu {
                    id: report.id,
                    close: report.close.clone(),
                    surfaces: report.surfaces.clone(),
                    anchor: None,
                    pointer: report.pointer,
                });
            }
        }

        // One menu at a time: everything else that painted open is closed. The
        // menu that just lost the window was closed above already, so its own
        // report is not closed a second time.
        if let Some(standing) = open.as_ref() {
            for report in &reports {
                if report.id != standing.id && Some(report.id) != replaced {
                    (report.close.borrow_mut())();
                }
            }
        }

        state.open = open;
    }
}

/// Whether `rect` holds `position`. Kept local so the module does not depend on
/// the interactive helpers.
fn contains(rect: RectF, position: Vector2F) -> bool {
    position.x >= rect.min_x()
        && position.x <= rect.max_x()
        && position.y >= rect.min_y()
        && position.y <= rect.max_y()
}

/// The pointer position a mouse event carries, if it carries one. The root view
/// uses this to check a press against the open menu's surface.
pub fn pointer_of(event: &DispatchedEvent) -> Option<Vector2F> {
    match event {
        DispatchedEvent::MouseDown { position, .. }
        | DispatchedEvent::MouseUp { position, .. }
        | DispatchedEvent::MouseMove { position } => Some(*position),
        _ => None,
    }
}

/// The point a menu's own trigger/control occupies, when the menu wants presses
/// on its control to reach it rather than be read as an outside dismiss. The
/// root view asks [`OpenMenus::pointer_on_anchor`].
impl OpenMenus {
    /// Whether `position` is on the open menu's anchor (its trigger). A press
    /// there toggles the menu through the menu's own element, the way a
    /// drop-down's own trigger does, so the root view leaves it alone.
    pub fn pointer_on_anchor(&self, position: Vector2F) -> bool {
        let state = self.state.borrow();
        state.open.as_ref().is_some_and(|open| {
            open.anchor
                .is_some_and(|anchor| contains(anchor, position))
        })
    }

    /// Record the open menu's own trigger box, so a press on the trigger is
    /// left to the menu instead of dismissing it. Called by menus that own a
    /// trigger element (the popup panel, a row's dots control).
    pub fn report_anchor(&self, id: MenuId, anchor: RectF) {
        let mut state = self.state.borrow_mut();
        if let Some(open) = state.open.as_mut() {
            if open.id == id {
                open.anchor = Some(anchor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{rectf, vec2f};

    /// A close handle that records it was run, so a test can see which menu the
    /// registry dismissed.
    fn closer(log: Rc<RefCell<Vec<&'static str>>>, id: &'static str) -> MenuClose {
        Rc::new(RefCell::new(move || log.borrow_mut().push(id)))
    }

    fn surface() -> RectF {
        rectf(100.0, 100.0, 120.0, 80.0)
    }

    #[test]
    fn opening_a_menu_keeps_one_open_and_closes_the_previous() {
        let menus = OpenMenus::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        menus.open("a", closer(Rc::clone(&log), "a"));
        assert!(menus.is_open());
        assert_eq!(menus.id(), Some("a"));

        menus.open("b", closer(Rc::clone(&log), "b"));
        assert_eq!(menus.id(), Some("b"));
        assert_eq!(*log.borrow(), vec!["a"], "opening b closed a");

        menus.close();
        assert!(!menus.is_open());
        assert_eq!(*log.borrow(), vec!["a", "b"], "close ran b's own handle");
    }

    #[test]
    fn the_open_menu_owns_the_pointer_on_its_surface_only() {
        let menus = OpenMenus::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        menus.open("panel", closer(Rc::clone(&log), "panel"));
        menus.begin_paint();
        menus.report(
            "panel",
            closer(Rc::clone(&log), "panel"),
            vec![surface()],
            MenuPointer::Paint,
        );
        menus.end_paint();

        assert!(menus.pointer_on_surface(vec2f(150.0, 140.0)));
        assert!(!menus.pointer_on_surface(vec2f(20.0, 20.0)));
        assert_eq!(menus.surfaces(), vec![surface()]);
    }

    #[test]
    fn a_menu_that_stops_painting_closed_itself() {
        let menus = OpenMenus::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        menus.open("panel", closer(Rc::clone(&log), "panel"));
        menus.begin_paint();
        menus.report(
            "panel",
            closer(Rc::clone(&log), "panel"),
            vec![surface()],
            MenuPointer::Paint,
        );
        menus.end_paint();
        assert!(menus.is_open());

        // Next frame it paints nothing: a row was taken (the element set its own
        // flag false), so the registry lets go without running the close handle
        // again.
        menus.begin_paint();
        menus.end_paint();
        assert!(!menus.is_open());
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn a_menu_opened_by_app_state_is_adopted_and_the_standing_one_closed() {
        let menus = OpenMenus::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        menus.open("a", closer(Rc::clone(&log), "a"));
        menus.begin_paint();
        menus.report(
            "a",
            closer(Rc::clone(&log), "a"),
            vec![surface()],
            MenuPointer::Paint,
        );
        menus.end_paint();

        // A shortcut flips another menu's flag; nothing claimed it.
        menus.begin_paint();
        menus.report(
            "a",
            closer(Rc::clone(&log), "a"),
            vec![surface()],
            MenuPointer::Paint,
        );
        menus.report(
            "b",
            closer(Rc::clone(&log), "b"),
            vec![rectf(300.0, 100.0, 60.0, 40.0)],
            MenuPointer::Paint,
        );
        menus.end_paint();

        assert_eq!(menus.id(), Some("b"), "the newer menu takes over");
        assert_eq!(*log.borrow(), vec!["a"], "the older one is closed");
    }

    #[test]
    fn a_straggler_menu_is_closed_so_only_one_stays_open() {
        let menus = OpenMenus::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        // Both report open on the same frame; the registry keeps the newest and
        // closes the other.
        menus.begin_paint();
        menus.report(
            "a",
            closer(Rc::clone(&log), "a"),
            vec![surface()],
            MenuPointer::Paint,
        );
        menus.report(
            "b",
            closer(Rc::clone(&log), "b"),
            vec![rectf(300.0, 100.0, 60.0, 40.0)],
            MenuPointer::Paint,
        );
        menus.end_paint();
        assert!(menus.is_open());
        assert_eq!(log.borrow().len(), 1, "one of the two was closed");
    }
}
