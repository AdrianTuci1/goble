//! A warp-new style context pill: an icon and a label that open a tray of
//! choices.
//!
//! One control, two homes: the agent keeps its pills in the composer footer at
//! the bottom of the pane, while a plain pty shows only its working-directory
//! and branch pills in the pane's topbar. Both build the same element so the
//! two surfaces cannot drift apart.
//!
//! The pill draws no disclosure chevron: the pill itself is the control that
//! opens the tray, and the whole card is its trigger.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Clipped, ComposerButton, ConstrainedBox, CrossAxisAlignment, Element, Flex, Icon,
    PopupMenu, PopupMenuItem, PopupMenuPosition, Text, Tooltip, TooltipPosition,
};
use crate::theme::ColorToken;

/// Height of a context pill. It matches the rich input's other footer
/// controls — the model, the attach button, the stop control — so one row of
/// them reads as one strip instead of a mix of heights.
pub const CONTEXT_PILL_HEIGHT: f32 = 24.0;

/// Which side of the trigger a pill's tray opens on. A pill at the bottom of
/// the window opens above itself; one in the topbar opens below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillTraySide {
    Above,
    Below,
}

/// A labelled context pill with an optional choice tray.
///
/// The pill is the card the footer control draws: an icon and a label, no
/// chevron. The tray opens from the pill itself.
pub struct ContextPill {
    /// The leading glyph, when the control draws one. A control whose word is
    /// enough (the approval mode) sets `None` and lets the label's colour carry
    /// the state.
    icon: Option<&'static str>,
    label: String,
    label_color: ColorToken,
    items: Vec<PopupMenuItem>,
    open: Option<Rc<RefCell<bool>>>,
    /// The app-owned keyboard highlight, shared with the tray's [`PopupMenu`]
    /// so Up/Down survives the per-frame rebuild.
    highlight: Option<Rc<RefCell<Option<usize>>>>,
    on_select: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    tooltip: String,
    tray_side: PillTraySide,
    /// When set, the label is clipped to this width (a long working-directory
    /// path narrows to fit instead of pushing the bar out of the pane).
    max_label_width: Option<f32>,
}

impl ContextPill {
    pub fn new(icon: &'static str, label: impl Into<String>) -> Self {
        Self {
            icon: Some(icon),
            label: label.into(),
            label_color: ColorToken::Muted,
            items: Vec::new(),
            open: None,
            highlight: None,
            on_select: None,
            tooltip: String::new(),
            tray_side: PillTraySide::Above,
            max_label_width: None,
        }
    }

    /// Draw the label alone, with no leading glyph. The word is the whole
    /// control; its colour says the state.
    pub fn without_icon(mut self) -> Self {
        self.icon = None;
        self
    }

    /// The label's colour. The glyph, when drawn, stays muted.
    pub fn with_label_color(mut self, color: ColorToken) -> Self {
        self.label_color = color;
        self
    }

    /// Attach the choice tray. Without items the pill is a plain label.
    pub fn with_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        on_select: F,
    ) -> Self {
        self.items = items;
        self.open = Some(open);
        self.on_select = Some(Rc::new(RefCell::new(on_select)));
        self
    }

    /// Share the tray's keyboard highlight with the app, like the open flag:
    /// the pill is rebuilt every frame, so the row Up/Down moved to has to live
    /// outside it.
    pub fn with_highlight_index(mut self, highlight: Rc<RefCell<Option<usize>>>) -> Self {
        self.highlight = Some(highlight);
        self
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = tooltip.into();
        self
    }

    pub fn with_tray_side(mut self, side: PillTraySide) -> Self {
        self.tray_side = side;
        self
    }

    pub fn with_max_label_width(mut self, width: f32) -> Self {
        self.max_label_width = Some(width);
        self
    }

    pub fn finish(self, app: &AppContext) -> Box<dyn Element> {
        let label = Text::new(self.label.clone())
            .with_theme_color(self.label_color, app)
            .with_font_size(11.0)
            .with_max_lines(1)
            .finish();
        // A long path narrows to the cap instead of pushing the bar out of the
        // pane; a short label just hugs its text.
        let label: Box<dyn Element> = match self.max_label_width {
            Some(width) => ConstrainedBox::new(Clipped::new(label).finish())
                .with_max_width(width)
                .finish(),
            None => label,
        };
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0);
        if let Some(icon) = self.icon {
            row = row.with_child(
                Icon::new(icon)
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        let row = row.with_child(label).finish();
        let trigger = Tooltip::new(
            ComposerButton::new(row)
                .with_height(CONTEXT_PILL_HEIGHT)
                .with_h_pad(8.0)
                .finish(),
            self.tooltip.clone(),
        )
        .with_position(match self.tray_side {
            PillTraySide::Above => TooltipPosition::Above,
            PillTraySide::Below => TooltipPosition::Below,
        })
        .finish();
        // A menu the host wired is a menu even before it has rows: a dropdown
        // whose content is read when it opens (the working directory, the
        // branch list) has nothing to show while it is closed, and gating the
        // menu on its rows would leave the pill with no way to open it.
        if self.on_select.is_none() {
            return trigger;
        }
        let mut menu = PopupMenu::new(trigger, self.items).with_position(match self.tray_side {
            PillTraySide::Above => PopupMenuPosition::Above,
            PillTraySide::Below => PopupMenuPosition::Below,
        });
        if let Some(open) = self.open {
            menu = menu.with_open(open);
        }
        if let Some(highlight) = self.highlight {
            menu = menu.with_highlight_index(highlight);
        }
        if let Some(cb) = self.on_select {
            menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
        }
        menu.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::geometry::vec2f;

    /// The pill's tray reads and writes the app's highlight cell, so Up/Down
    /// keeps its row across the per-frame rebuild.
    #[test]
    fn the_pill_tray_shares_the_apps_keyboard_highlight() {
        let app = AppContext::default();
        let open = Rc::new(RefCell::new(true));
        let highlight: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let picked = Rc::new(RefCell::new(None));
        let click = Rc::clone(&picked);
        let mut element = ContextPill::new("git-branch", "main")
            .with_menu(
                vec![
                    PopupMenuItem::new("main").selected(),
                    PopupMenuItem::new("dev"),
                ],
                Rc::clone(&open),
                move |index| *click.borrow_mut() = Some(index),
            )
            .with_highlight_index(Rc::clone(&highlight))
            .finish(&app);

        element.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        assert_eq!(
            *highlight.borrow(),
            Some(0),
            "the open tray starts on the item in force"
        );

        let mut ctx = EventContext::default();
        let mut press = |element: &mut Box<dyn Element>, key: &str| {
            element.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: ModifiersState::default(),
                },
                &mut ctx,
                &app,
            )
        };
        assert!(press(&mut element, "ArrowDown"), "the tray takes the arrow");
        assert_eq!(*highlight.borrow(), Some(1));
        assert!(press(&mut element, "Enter"), "and Enter");
        assert_eq!(*picked.borrow(), Some(1), "Enter takes the highlighted row");
        assert!(!*open.borrow(), "and closes the tray");
    }
}
