//! Responsive layout: the window narrow enough that the panes can no longer
//! keep their minimum width.
//!
//! This is Goble's port of DeepSeek Harness's breakpoint behaviour. DSH
//! (`packages/client/ui-layout/src/client/columns.ts` + `stores.ts`) solves a
//! three-column frame every frame from the measured viewport:
//!
//! - `SIDEBAR_AUTO_COLLAPSE = 1024` — below the "LG" breakpoint the sidebar
//!   auto-collapses; a manual toggle below it re-expands it over the squeezed
//!   centre (`narrowExpanded`), and crossing the breakpoint in either direction
//!   drops that override so the wide state is the user's preference again;
//! - `CENTER_MIN = 400` — the centre column is protected: the right panel
//!   shrinks and then loses its track before the centre may fall below it;
//! - `SIDEBAR_DRAWER_BREAKPOINT = 768` — below it side surfaces become drawer
//!   overlays instead of taking space.
//!
//! Goble has no second grid column to drop: its "right panel" is the agent
//! panel sheet and the agent dashboard, which are overlays that replace or
//! cover the panes. The corresponding contract is:
//!
//! 1. the pane tree has a minimum width ([`PANE_MIN_WIDTH`] per pane, plus the
//!    panel-area insets and the split gutter), and the splits stop a resize at
//!    it (`SplitNode::with_min_pane_px`);
//! 2. when the window is narrower than the panes need WITH the sidebar, the
//!    sidebar auto-collapses (DSH's 1024 floor, earlier for a wide pane tree);
//! 3. when it is still narrower (below the panes' own minimum, or below DSH's
//!    768 drawer breakpoint), the secondary surfaces — the agent panel sheet
//!    and the agent dashboard — leave the view, and if even the collapsed frame
//!    cannot give every pane its minimum, only the active pane is drawn;
//! 4. everything restores in reverse as the window grows, with a small
//!    hysteresis so a drag across the breakpoint does not flap.
//!
//! The decisions are computed once per frame from the frame's own width (see
//! `RootView::layout`), so a resize settles in the frame that caused it.

use crate::ui::panes::{pane_min_width, PANE_AREA_INSET, PANE_INSET};
use crate::ui::LayoutKind;

use super::UiState;

/// The narrowest a pane may become, in pixels: the owner's minimum. DeepSeek
/// Harness protects a 400 pt conversation centre; Goble's panes carry a full
/// composer and header controls, so 360 pt keeps the rich input usable while
/// still letting a two-pane split live beside the sidebar on a normal window.
pub const PANE_MIN_WIDTH: f32 = 360.0;

/// Viewport width below which the sidebar auto-collapses, DSH's deepsuite "LG"
/// breakpoint. It is a floor: a pane tree that needs more width than this
/// collapses the sidebar earlier (see [`UiState::responsive_breakpoint`]).
pub const SIDEBAR_AUTO_COLLAPSE: f32 = 1024.0;

/// Viewport width below which the secondary panels leave the view, DSH's
/// drawer breakpoint. Below it DSH stops giving the side surfaces a grid track;
/// Goble hides the agent panel sheet and the dashboard so the panes keep the
/// whole frame.
pub const SECONDARY_PANEL_BREAKPOINT: f32 = 768.0;

/// The dead band around a breakpoint, so dragging the window edge across it
/// does not flap between the collapsed and expanded layout.
pub const RESPONSIVE_HYSTERESIS: f32 = 8.0;

/// What the responsive layout decides for one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Responsive {
    /// The window is below the sidebar breakpoint: the sidebar is auto-collapsed
    /// (unless the user re-expanded it below the breakpoint).
    pub sidebar_collapsed: bool,
    /// The window is below the second breakpoint, or the panes cannot keep
    /// their minimum width: the agent panel sheet and the dashboard leave the
    /// view.
    pub secondary_hidden: bool,
    /// Even with the sidebar gone the window cannot give every pane its
    /// minimum width: only the active pane is drawn (it stays at its minimum).
    pub focused_only: bool,
}

impl Responsive {
    /// Whether the sidebar is drawn this frame: the user's preference and the
    /// responsive collapse together.
    pub fn sidebar_shown(&self, sidebar_visible: bool) -> bool {
        sidebar_visible && !self.sidebar_collapsed
    }
}

/// The minimum width of one pane's slot: the pane's own minimum plus the frame
/// gutter it keeps inside its slot on both sides.
pub const fn pane_slot_min_width() -> f32 {
    PANE_MIN_WIDTH + 2.0 * PANE_INSET
}

impl UiState {
    /// The minimum width the active space's panes need, including the panel
    /// area's own insets. The window must be at least this wide for every pane
    /// to keep [`PANE_MIN_WIDTH`].
    pub fn panes_min_width(&self) -> f32 {
        let slot_min = pane_slot_min_width();
        let panes = match self.spaces.get(self.active_space) {
            Some(space) => match space.layout {
                LayoutKind::Bsp => pane_min_width(&space.root, slot_min),
                // Master-stack shows the master and the stack side by side;
                // floating windows overlap and a carousel shows one window
                // centred, so one pane's minimum is the floor there.
                LayoutKind::MasterStack => {
                    let mut leaves = Vec::new();
                    space.root.leaves(&mut leaves);
                    if leaves.len() > 1 {
                        2.0 * slot_min
                    } else {
                        slot_min
                    }
                }
                LayoutKind::Floating | LayoutKind::Carousel => slot_min,
            },
            None => slot_min,
        };
        panes + 2.0 * PANE_AREA_INSET
    }

    /// The sidebar auto-collapse breakpoint for the current pane tree: the
    /// window width at which the sidebar stops fitting beside the panes, with
    /// DSH's `SIDEBAR_AUTO_COLLAPSE` as the floor. A wide pane tree therefore
    /// collapses the sidebar earlier than a single pane does.
    pub fn responsive_breakpoint(&self) -> f32 {
        let sidebar = if self.sidebar_visible {
            self.sidebar_width
        } else {
            0.0
        };
        (self.panes_min_width() + sidebar).max(SIDEBAR_AUTO_COLLAPSE)
    }

    /// Update the responsive decisions from the measured window width.
    ///
    /// Called once per frame with the frame's own width, before the snapshot is
    /// taken, so a resize settles in the frame that caused it. Everything that
    /// reads the layout afterwards (the sidebar, the secondary panels, the pane
    /// tree) reads [`UiState::responsive`].
    pub fn update_responsive(&mut self, window_width: f32) {
        if !window_width.is_finite() || window_width <= 0.0 {
            return;
        }
        self.responsive_width = window_width;
        self.settings_overlay_wide = window_width >= 900.0;
        let breakpoint = self.responsive_breakpoint();
        // Hysteresis: once collapsed, stay collapsed until the window is
        // clearly wider than the breakpoint; once expanded, collapse only when
        // clearly narrower. Crossing the breakpoint in either direction also
        // drops the manual re-expand override, exactly like DSH's
        // `setViewportWidth`.
        let narrow = if self.responsive_narrow {
            window_width < breakpoint + RESPONSIVE_HYSTERESIS
        } else {
            window_width < breakpoint - RESPONSIVE_HYSTERESIS
        };
        if narrow != self.responsive_narrow {
            self.sidebar_narrow_expanded = false;
            self.responsive_narrow = narrow;
        }
        let sidebar_collapsed = narrow && !self.sidebar_narrow_expanded;
        // The width left for the panes with the sidebar's effective visibility
        // (a narrow re-expand takes its width back from the panes).
        let sidebar_width = if self.sidebar_visible && !sidebar_collapsed {
            self.sidebar_width
        } else {
            0.0
        };
        let available = window_width - sidebar_width;
        let panes_min = self.panes_min_width();
        // Second step: the secondary surfaces leave the view below DSH's
        // drawer breakpoint, or as soon as the panes can no longer keep their
        // minimum beside/under them.
        let secondary_hidden = window_width < SECONDARY_PANEL_BREAKPOINT || available < panes_min;
        // Last resort: the panes themselves keep their minimum by drawing only
        // the active one over the whole pane area.
        let focused_only = available < panes_min;
        self.responsive = Responsive {
            sidebar_collapsed,
            secondary_hidden,
            focused_only,
        };
    }

    /// The user's manual toggle at a narrow width: it flips the re-expand
    /// override over the squeezed panes instead of rewriting the sidebar
    /// preference, so growing the window restores the pre-squeeze layout. At a
    /// wide width it is the ordinary visibility toggle.
    pub fn toggle_sidebar_responsive(&mut self) {
        if self.responsive_narrow {
            self.sidebar_narrow_expanded = !self.sidebar_narrow_expanded;
            // Re-solve now: the sidebar's width returns to (or leaves) the
            // panes immediately, so the panes' fallback is right in the frame
            // the click paints.
            let width = self.responsive_width;
            if width > 0.0 {
                self.update_responsive(width);
            }
        } else {
            self.sidebar_visible = !self.sidebar_visible;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Pane, PaneKind, Space, SplitDir};

    fn leaf(id: u64) -> Box<Pane> {
        Box::new(Pane::Leaf {
            id,
            kind: PaneKind::Chat,
        })
    }

    fn two_panes() -> UiState {
        let mut state = UiState::mock();
        state.spaces = vec![Space::unnamed(Pane::Split {
            id: 9,
            dir: SplitDir::Horizontal,
            ratio: 0.5,
            first: leaf(7),
            second: leaf(8),
        })];
        state.active_space = 0;
        state.active_pane_id = 7;
        state
    }

    fn one_pane() -> UiState {
        let mut state = UiState::mock();
        state.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Chat,
        })];
        state.active_space = 0;
        state.active_pane_id = 7;
        state
    }

    /// The pane tree's minimum is the sum of its side-by-side panes (plus the
    /// panel area's insets) and the widest of its stacked ones.
    #[test]
    fn the_pane_tree_minimum_is_summed_across_side_by_side_panes() {
        let mut state = one_pane();
        let one = state.panes_min_width();
        assert_eq!(one, pane_slot_min_width() + 2.0 * PANE_AREA_INSET);

        state = two_panes();
        assert_eq!(
            state.panes_min_width(),
            2.0 * pane_slot_min_width() + 2.0 * PANE_AREA_INSET
        );

        // Stacked panes do not add width: the widest child is the floor.
        state.spaces[0].root = Pane::Split {
            id: 9,
            dir: SplitDir::Vertical,
            ratio: 0.5,
            first: leaf(7),
            second: leaf(8),
        };
        assert_eq!(state.panes_min_width(), one);
    }

    /// A one-pane window keeps the sidebar well past the DSH LG breakpoint; at
    /// exactly the breakpoint it is still expanded (DSH collapses below it).
    #[test]
    fn the_sidebar_follows_the_breakpoint_without_a_manual_override() {
        let mut state = one_pane();
        state.update_responsive(1600.0);
        assert!(!state.responsive.sidebar_collapsed);
        assert!(!state.responsive.secondary_hidden);
        assert!(!state.responsive.focused_only);
        assert!(state.responsive.sidebar_shown(state.sidebar_visible));

        // The LG breakpoint is the floor: at 1024 the sidebar still shows.
        state.update_responsive(SIDEBAR_AUTO_COLLAPSE);
        assert!(!state.responsive.sidebar_collapsed);

        // Below it the sidebar collapses; the panes keep the whole width.
        state.update_responsive(SIDEBAR_AUTO_COLLAPSE - 40.0);
        assert!(state.responsive.sidebar_collapsed);
        assert!(!state.responsive.sidebar_shown(true));
        assert!(!state.responsive.secondary_hidden);
        assert!(!state.responsive.focused_only);
    }

    /// A wide pane tree collapses the sidebar BEFORE the LG breakpoint: the
    /// breakpoint is the measured need (panes + sidebar), not a fixed number.
    #[test]
    fn a_wide_pane_tree_collapses_the_sidebar_earlier() {
        let mut state = two_panes();
        let breakpoint = state.panes_min_width() + state.sidebar_width;
        assert!(breakpoint > SIDEBAR_AUTO_COLLAPSE);
        assert_eq!(state.responsive_breakpoint(), breakpoint);

        // Above the measured need: everything stays.
        state.update_responsive(breakpoint + 40.0);
        assert!(!state.responsive.sidebar_collapsed);
        // Just below it: the sidebar gives up its width first.
        state.update_responsive(breakpoint - 40.0);
        assert!(state.responsive.sidebar_collapsed);
        assert!(!state.responsive.focused_only);
        // The panes fit again with the whole width.
        assert!(!state.responsive.secondary_hidden);
    }

    /// Below the second breakpoint the secondary panels leave the view; below
    /// the panes' own minimum only the active pane is left.
    #[test]
    fn the_second_breakpoint_hides_the_secondary_panels_then_focuses_a_pane() {
        let mut state = two_panes();
        // DSH's drawer breakpoint hides the secondary panels.
        state.update_responsive(SECONDARY_PANEL_BREAKPOINT - 20.0);
        assert!(state.responsive.sidebar_collapsed);
        assert!(state.responsive.secondary_hidden);
        assert!(!state.responsive.focused_only, "two panes still fit 748 px");

        // Below the panes' own minimum even the whole width cannot hold both.
        let panes_min = state.panes_min_width();
        state.update_responsive(panes_min - 20.0);
        assert!(state.responsive.focused_only);
        assert!(state.responsive.secondary_hidden);
    }

    /// A manual toggle below the breakpoint re-expands the sidebar over the
    /// squeezed panes without rewriting the preference; the next breakpoint
    /// crossing drops the override and the wide state restores the preference.
    #[test]
    fn a_manual_toggle_wins_until_the_next_breakpoint_crossing() {
        let mut state = one_pane();
        state.update_responsive(1600.0);
        assert!(!state.responsive.sidebar_collapsed);

        // Narrow: auto-collapsed. The toggle re-expands it.
        state.update_responsive(900.0);
        assert!(state.responsive.sidebar_collapsed);
        state.toggle_sidebar_responsive();
        assert!(!state.responsive.sidebar_collapsed);
        assert!(state.responsive.sidebar_shown(state.sidebar_visible));
        assert!(state.sidebar_visible, "the preference is untouched");

        // Staying inside the breakpoint keeps the override.
        state.update_responsive(880.0);
        assert!(!state.responsive.sidebar_collapsed);

        // Growing past the breakpoint is a crossing: the override is dropped,
        // and the wide layout shows the preference again.
        state.update_responsive(1600.0);
        assert!(!state.sidebar_narrow_expanded);
        assert!(!state.responsive.sidebar_collapsed);
        assert!(state.responsive.sidebar_shown(state.sidebar_visible));

        // Narrowing again crosses the other way: the override is gone, so the
        // sidebar auto-collapses again.
        state.update_responsive(900.0);
        assert!(state.responsive.sidebar_collapsed);
        assert!(!state.sidebar_narrow_expanded);
    }

    /// A user who hid the sidebar keeps it hidden across widths; the responsive
    /// collapse never turns a hidden sidebar back on.
    #[test]
    fn a_hidden_sidebar_stays_hidden() {
        let mut state = one_pane();
        state.sidebar_visible = false;
        state.update_responsive(900.0);
        assert!(!state.responsive.sidebar_shown(false));
        state.update_responsive(1600.0);
        assert!(!state.responsive.sidebar_shown(false));
    }

    /// The breakpoint has hysteresis: the window must move past the dead band
    /// before the layout flaps between collapsed and expanded.
    #[test]
    fn the_breakpoint_has_a_dead_band() {
        let mut state = one_pane();
        let breakpoint = state.responsive_breakpoint();
        // Start wide, land exactly on the dead band's inner edge: still wide.
        state.update_responsive(breakpoint + 1.0);
        state.update_responsive(breakpoint - 1.0);
        assert!(!state.responsive.sidebar_collapsed, "inside the dead band");
        // Cross the dead band: collapse.
        state.update_responsive(breakpoint - RESPONSIVE_HYSTERESIS - 1.0);
        assert!(state.responsive.sidebar_collapsed);
        // Coming back up needs the same margin to restore.
        state.update_responsive(breakpoint + 1.0);
        assert!(state.responsive.sidebar_collapsed);
        state.update_responsive(breakpoint + RESPONSIVE_HYSTERESIS + 1.0);
        assert!(!state.responsive.sidebar_collapsed);
    }
}
