//! The conversation footer's usage popover: DSH's trigger-anchored stat dialog,
//! rebuilt for Goble's immediate-mode UI.
//!
//! The footer is the closing child of the transcript's `Scrollable`, and that
//! region clips its children to the viewport (`Scrollable::paint` pushes a clip
//! rect). A details panel painted at a negative offset from the footer's own
//! paint pass would be cut off at the transcript's top edge and would scroll
//! away with the last message — which is not what DSH's portaled dialog does.
//! The panel therefore rides the frame's last layer instead: the element lays
//! it out and queues it with [`HoverChipRegistry::push`], and `HoverChipLayer`
//! — painted after the whole tree by the app root — draws it above everything,
//! exactly like a tooltip. That registry exists for a box that must be seen
//! over the surface it hangs from.
//!
//! The app owns the open flag (`ChatView::usage_open`, wired through the host),
//! so it survives the per-frame element rebuild the way every menu's flag does.
//! The pill itself is the caller's element: this wrapper only positions the
//! panel, owns the click/Escape/outside-press dismissal, and keeps the footer's
//! layout untouched (the wrapper is exactly the trigger's size; the panel is a
//! floating layer, not a row of the transcript).
//!
//! Placement is DSH's (shared `stat-dialog` skin): the panel hangs **above** the
//! trigger with an 8 px gap, its left edge on the trigger's, inside a 12 px
//! window inset. When the pane is too short above the trigger, the panel flips
//! below it rather than covering the pill — the pill has to stay pressable,
//! because it is also the close toggle.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::contains;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::{DispatchedEvent, BUTTON_PRIMARY};
use crate::geometry::{rectf, vec2f, RectF, Vector2F};

/// DSH `stat-dialog.module.css`: the anchored panel keeps 8 px from its
/// trigger's top edge.
const PANEL_GAP: f32 = 8.0;
/// DSH `stat-dialog.module.css`: 12 px between the panel and the window's edge.
const PANEL_WINDOW_INSET: f32 = 12.0;
/// The panel's own ceiling while it is measured. The Goble row set is a
/// handful of lines, so this only guards a pathological value.
const PANEL_MAX_HEIGHT: f32 = 400.0;
/// DSH's dialog width band: `min-width: min(300px, 100vw - 24px)` and
/// `max-width: min(440px, 100vw - 24px)`. The Goble rows measure under the
/// minimum, so 300 is the width an ordinary pane gets.
pub(super) const PANEL_MIN_WIDTH: f32 = 300.0;
const PANEL_MAX_WIDTH: f32 = 440.0;

/// The trigger-anchored details panel: the pill the caller built, and the panel
/// it opens above itself while the app-owned flag says so.
pub(super) struct UsagePopover {
    trigger: Box<dyn Element>,
    /// The app-owned open flag (`ChatView::usage_open`).
    open: Rc<RefCell<bool>>,
    /// The panel built by the caller while open. `None` while closed, so a
    /// closed popover never pays for the panel's layout or paint.
    panel: Option<Box<dyn Element>>,
    panel_size: Option<Vector2F>,
    /// The panel's origin relative to the trigger's top-left corner — negative
    /// in y, above the trigger.
    panel_origin: Vector2F,
    /// Where the panel was queued, in window coordinates. Presses are read
    /// against it, so the flipped fallback is what the hit-test sees.
    panel_rect: Option<RectF>,
    /// Whether the press that started on the pill is still down, so the toggle
    /// reads as a click on the pill rather than a press dragged in from
    /// elsewhere.
    pressed: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl UsagePopover {
    /// Wrap the footer pill. `panel` is the details surface, built by the
    /// caller only while the flag is open (`None` while closed).
    pub(super) fn new(
        trigger: Box<dyn Element>,
        open: Rc<RefCell<bool>>,
        panel: Option<Box<dyn Element>>,
    ) -> Self {
        Self {
            trigger,
            open,
            panel,
            panel_size: None,
            panel_origin: Vector2F::zero(),
            panel_rect: None,
            pressed: false,
            size: None,
            origin: None,
        }
    }

    fn is_open(&self) -> bool {
        *self.open.borrow()
    }

    fn trigger_bounds(&self) -> Option<RectF> {
        self.bounds()
    }

    fn panel_bounds(&self) -> Option<RectF> {
        self.panel_rect
    }

    fn close(&mut self) {
        *self.open.borrow_mut() = false;
        self.panel_rect = None;
    }
}

impl Element for UsagePopover {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let trigger_size = self.trigger.layout(constraint, ctx, app);
        self.size = Some(trigger_size);

        self.panel_size = None;
        self.panel_origin = Vector2F::zero();
        if let Some(panel) = self.panel.as_mut() {
            // DSH's dialog is `width: max-content` clamped to
            // `[min(300px, 100vw-24px), min(440px, 100vw-24px)]`. Goble's row
            // set (a label column and short counts) measures below the minimum,
            // so the minimum is what every ordinary pane gets; a pane narrower
            // than that gets the pane's own width rather than spilling past it.
            let available = if constraint.max.x.is_finite() {
                constraint.max.x
            } else {
                PANEL_MAX_WIDTH
            };
            let width = available.min(PANEL_MIN_WIDTH).max(0.0);
            let panel_size = panel.layout(
                SizeConstraint::new(vec2f(width, 0.0), vec2f(width, PANEL_MAX_HEIGHT)),
                ctx,
                app,
            );
            self.panel_size = Some(panel_size);
            // DSH anchors the dialog's left edge on the trigger's, then clamps
            // it inside the pane; Goble's footer pill is leading-aligned, so the
            // clamp only matters in a very narrow pane.
            let x = 0.0_f32.clamp(0.0, (available - panel_size.x).max(0.0));
            self.panel_origin = vec2f(x, -(panel_size.y + PANEL_GAP));
        }
        trigger_size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.trigger.paint(origin, ctx, app);

        let (Some(panel), Some(panel_size), Some(trigger_size)) =
            (self.panel.take(), self.panel_size, self.size)
        else {
            self.panel_rect = None;
            return;
        };
        // Above the trigger, as DSH anchors it. Clamped into the window's inset;
        // when that would cover the pill (a very short pane), the panel flips
        // below instead, DSH's own `--dsh-stat-opposite` fallback.
        let mut at = vec2f(origin.x + self.panel_origin.x, origin.y + self.panel_origin.y);
        if at.y < PANEL_WINDOW_INSET {
            at.y = PANEL_WINDOW_INSET;
        }
        if at.y + panel_size.y > origin.y {
            at.y = origin.y + trigger_size.y + PANEL_GAP;
        }
        self.panel_rect = Some(rectf(at.x, at.y, panel_size.x, panel_size.y));
        // The last layer draws it: this element's own paint pass is clipped to
        // the transcript viewport, and the panel belongs over the whole pane.
        app.hover_chips.borrow_mut().push(at, panel);
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
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        let Some(trigger_bounds) = self.trigger_bounds() else {
            return false;
        };
        let inside_trigger = match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => contains(trigger_bounds, *position),
            _ => false,
        };
        let inside_panel = self
            .panel_bounds()
            .is_some_and(|bounds| match event {
                DispatchedEvent::MouseDown { position, .. }
                | DispatchedEvent::MouseUp { position, .. }
                | DispatchedEvent::MouseMove { position } => contains(bounds, *position),
                _ => false,
            });

        match event {
            // The pill is not forwarded to: its `HoverRow` consumes the press
            // cycle itself, which would swallow the toggle. The popover owns the
            // gesture: a press that starts on the pill and is released on it
            // toggles, whether the panel is open or closed.
            DispatchedEvent::MouseDown { button, .. } if inside_trigger => {
                self.pressed = *button == BUTTON_PRIMARY;
                self.pressed
            }
            DispatchedEvent::MouseUp { button, .. } if inside_trigger => {
                let toggled = self.pressed && *button == BUTTON_PRIMARY;
                self.pressed = false;
                if toggled {
                    let now_open = !self.is_open();
                    *self.open.borrow_mut() = now_open;
                    if !now_open {
                        self.panel_rect = None;
                    }
                }
                toggled
            }
            // A press on the open panel is the panel's own: the table is
            // information, and the press must not reach the transcript's text
            // selection or a row behind it.
            DispatchedEvent::MouseDown { .. } | DispatchedEvent::MouseUp { .. } if inside_panel => {
                true
            }
            DispatchedEvent::MouseMove { .. } if inside_panel => true,
            // A primary press anywhere else closes the open panel and keeps its
            // own meaning (DSH closes on an outside pointerdown). A press that
            // is not on the pill leaves no pressed state behind.
            DispatchedEvent::MouseDown { button, .. } => {
                self.pressed = false;
                if self.is_open() && *button == BUTTON_PRIMARY {
                    self.close();
                }
                false
            }
            DispatchedEvent::MouseUp { .. } => {
                self.pressed = false;
                false
            }
            // Escape closes, and only while it is open: a closed footer leaves
            // the key to the surface behind it (the sub-agent child view's own
            // escape hook, say). The panel holds no keyboard focus of its own,
            // so nothing is trapped: the composer keeps the keys.
            DispatchedEvent::KeyDown { key, modifiers }
                if self.is_open()
                    && key == "Escape"
                    && !modifiers.ctrl
                    && !modifiers.command
                    && !modifiers.alt =>
            {
                self.close();
                true
            }
            // A panel anchored to a trigger that just scrolled away must not
            // float over the transcript: the wheel closes it and keeps its own
            // meaning.
            DispatchedEvent::Scroll { .. } if self.is_open() => {
                self.close();
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Empty, EventContext, HoverChipLayer, LayoutContext, PaintContext};
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::geometry::vec2f;

    fn trigger() -> Box<dyn Element> {
        Empty::new().with_size(vec2f(120.0, 28.0)).finish()
    }

    fn panel() -> Box<dyn Element> {
        Empty::new().with_size(vec2f(300.0, 140.0)).finish()
    }

    /// Lay a popover out at (0, 300), paint it, and report what it queued and
    /// where it put the panel.
    fn paint_frame(popover: &mut UsagePopover) -> (usize, Option<RectF>, Vector2F) {
        let app = AppContext::default();
        let size = popover.layout(
            SizeConstraint::loose(vec2f(600.0, 480.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::default();
        popover.paint(vec2f(0.0, 300.0), &mut ctx, &app);
        let queued = app.hover_chips.borrow().len();
        // Drain the layer, so the next frame starts empty like the app root's.
        let mut layer_ctx = PaintContext::default();
        HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut layer_ctx, &app);
        (queued, popover.panel_rect, size)
    }

    #[test]
    fn a_closed_popover_takes_no_panel_or_layer_space() {
        let open = Rc::new(RefCell::new(false));
        let mut popover = UsagePopover::new(trigger(), open, None);
        let (queued, rect, size) = paint_frame(&mut popover);
        assert_eq!(queued, 0, "a closed panel queues nothing");
        assert!(rect.is_none());
        assert_eq!(
            size,
            vec2f(120.0, 28.0),
            "the wrapper is the trigger's size"
        );
    }

    #[test]
    fn the_panel_is_queued_above_the_trigger() {
        let open = Rc::new(RefCell::new(true));
        let mut popover = UsagePopover::new(trigger(), open, Some(panel()));
        let (queued, rect, _) = paint_frame(&mut popover);
        assert_eq!(queued, 1, "the open panel queues itself for the last layer");
        let rect = rect.expect("the panel was queued");
        assert_eq!(rect.min_y(), 300.0 - 140.0 - PANEL_GAP);
        assert_eq!(rect.max_y(), 300.0 - PANEL_GAP);
        assert_eq!(rect.min_x(), 0.0);
        assert_eq!(rect.width(), 300.0);
    }

    #[test]
    fn a_short_pane_flips_the_panel_below_the_trigger() {
        let app = AppContext::default();
        let open = Rc::new(RefCell::new(true));
        let mut popover = UsagePopover::new(trigger(), open, Some(panel()));
        let _ = popover.layout(
            SizeConstraint::loose(vec2f(600.0, 480.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::default();
        // Ten points above the window's top inset: above would be clamped into
        // the pill, so the panel goes below instead.
        popover.paint(vec2f(0.0, 10.0), &mut ctx, &app);
        let rect = popover.panel_rect.expect("the panel was queued");
        assert_eq!(rect.min_y(), 10.0 + 28.0 + PANEL_GAP);
    }

    #[test]
    fn a_press_on_the_pill_toggles_the_open_flag() {
        let app = AppContext::default();
        let open = Rc::new(RefCell::new(false));
        let mut popover = UsagePopover::new(trigger(), open.clone(), None);
        let _ = popover.layout(
            SizeConstraint::loose(vec2f(600.0, 480.0)),
            &mut LayoutContext::default(),
            &app,
        );
        popover.paint(vec2f(0.0, 300.0), &mut PaintContext::default(), &app);
        let mut ctx = EventContext;
        let inside = vec2f(10.0, 310.0);
        assert!(popover.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: inside,
                button: BUTTON_PRIMARY
            },
            &mut ctx,
            &app
        ));
        assert!(popover.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: inside,
                button: BUTTON_PRIMARY
            },
            &mut ctx,
            &app
        ));
        assert!(*open.borrow(), "the press opens the panel");
        assert!(popover.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: inside,
                button: BUTTON_PRIMARY
            },
            &mut ctx,
            &app
        ));
        assert!(popover.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: inside,
                button: BUTTON_PRIMARY
            },
            &mut ctx,
            &app
        ));
        assert!(!*open.borrow(), "and closes it again");
    }

    #[test]
    fn an_open_panel_closes_on_escape_outside_press_or_scroll() {
        let app = AppContext::default();
        let open = Rc::new(RefCell::new(true));
        let mut popover = UsagePopover::new(trigger(), open.clone(), Some(panel()));
        let _ = popover.layout(
            SizeConstraint::loose(vec2f(600.0, 480.0)),
            &mut LayoutContext::default(),
            &app,
        );
        popover.paint(vec2f(0.0, 300.0), &mut PaintContext::default(), &app);
        let mut ctx = EventContext;

        let handled = popover.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: ModifiersState::default(),
            },
            &mut ctx,
            &app,
        );
        assert!(handled, "the open panel takes Escape");
        assert!(!*open.borrow(), "and closes itself");

        *open.borrow_mut() = true;
        let handled = popover.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(590.0, 470.0),
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        assert!(!handled, "the outside press keeps its own meaning");
        assert!(!*open.borrow(), "while closing the panel");

        *open.borrow_mut() = true;
        let handled = popover.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, -10.0),
                precise: false,
            },
            &mut ctx,
            &app,
        );
        assert!(!handled, "the wheel keeps its own meaning");
        assert!(!*open.borrow(), "and closes the panel");
    }

    #[test]
    fn a_closed_panel_leaves_escape_to_the_surface_behind_it() {
        let app = AppContext::default();
        let open = Rc::new(RefCell::new(false));
        let mut popover = UsagePopover::new(trigger(), open, None);
        let mut ctx = EventContext;
        assert!(!popover.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: ModifiersState::default(),
            },
            &mut ctx,
            &app,
        ));
    }
}
