//! A warp-new style context pill: an icon, a label and a chevron that opens a
//! tray of choices.
//!
//! One control, two homes: the agent keeps its pills in the composer footer at
//! the bottom of the pane, while a plain pty shows only its working-directory
//! and branch pills in the pane's topbar. Both build the same element so the
//! two surfaces cannot drift apart.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Clipped, ComposerButton, ConstrainedBox, CrossAxisAlignment, Element, Flex, Icon,
    PopupMenu, PopupMenuItem, PopupMenuPosition, Text, Tooltip, TooltipPosition,
};
use crate::theme::ColorToken;

/// Height of a context pill. Callers lay their bar out around it (the pane
/// header pads itself out to the shared topbar height around this).
pub const CONTEXT_PILL_HEIGHT: f32 = 28.0;

/// Which side of the trigger a pill's tray opens on. A pill at the bottom of
/// the window opens above itself; one in the topbar opens below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillTraySide {
    Above,
    Below,
}

/// A labelled context pill with an optional choice tray.
pub struct ContextPill {
    icon: &'static str,
    label: String,
    items: Vec<PopupMenuItem>,
    open: Option<Rc<RefCell<bool>>>,
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
            icon,
            label: label.into(),
            items: Vec::new(),
            open: None,
            on_select: None,
            tooltip: String::new(),
            tray_side: PillTraySide::Above,
            max_label_width: None,
        }
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
            .with_theme_color(ColorToken::Muted, app)
            .with_font_size(12.0)
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
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(
                Icon::new(self.icon)
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(label)
            .with_child(
                Icon::new("chevron-down")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish();
        let trigger = Tooltip::new(
            ComposerButton::new(row)
                .with_height(CONTEXT_PILL_HEIGHT)
                .finish(),
            self.tooltip.clone(),
        )
        .with_position(match self.tray_side {
            PillTraySide::Above => TooltipPosition::Above,
            PillTraySide::Below => TooltipPosition::Below,
        })
        .finish();
        if self.items.is_empty() {
            return trigger;
        }
        let mut menu = PopupMenu::new(trigger, self.items).with_position(match self.tray_side {
            PillTraySide::Above => PopupMenuPosition::Above,
            PillTraySide::Below => PopupMenuPosition::Below,
        });
        if let Some(open) = self.open {
            menu = menu.with_open(open);
        }
        if let Some(cb) = self.on_select {
            menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
        }
        menu.finish()
    }
}
