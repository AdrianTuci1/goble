use crate::elements::EventContext;
use crate::event::DispatchedEvent;
use crate::geometry::{RectF, Vector2F};

/// Tracks pointer-interaction state for a single interactive element.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InteractiveState {
    pub hover: bool,
    pub pressed: bool,
}

impl InteractiveState {
    /// Returns true if the element should visually show an active/pressed state.
    pub fn is_active(&self) -> bool {
        self.pressed && self.hover
    }
}

/// Check whether a global pointer position is inside the element bounds.
pub fn contains(bounds: RectF, position: Vector2F) -> bool {
    position.x >= bounds.min_x()
        && position.x <= bounds.max_x()
        && position.y >= bounds.min_y()
        && position.y <= bounds.max_y()
}

/// Process a dispatched event and update interaction state.
///
/// Returns `true` if the event was consumed. When a click is completed, `on_click` is called.
pub fn handle_mouse_event(
    state: &mut InteractiveState,
    event: &DispatchedEvent,
    bounds: RectF,
    _ctx: &mut EventContext,
    on_click: &mut dyn FnMut(),
) -> bool {
    match event {
        DispatchedEvent::MouseMove { position } => {
            let inside = contains(bounds, *position);
            if state.hover != inside {
                state.hover = inside;
            }
            // Mouse move does not consume the event; let other elements see it too.
            false
        }
        DispatchedEvent::MouseDown { position, .. } => {
            if contains(bounds, *position) {
                state.pressed = true;
                true
            } else {
                false
            }
        }
        DispatchedEvent::MouseUp { position, .. } => {
            // The element tree is rebuilt every frame (see the root view's
            // `layout`), so the `pressed` flag set by an earlier MouseDown on
            // this instance is lost before the matching MouseUp arrives. Fire
            // the click on any release that lands inside the bounds regardless
            // of this instance's pressed flag, so a click still completes after
            // a rebuild. `pressed` only drives the down-state visual.
            state.pressed = false;
            if contains(bounds, *position) {
                on_click();
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
