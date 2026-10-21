/// OS-level keyboard modifier state attached to key events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifiersState {
    pub alt: bool,
    pub ctrl: bool,
    pub command: bool,
    pub shift: bool,
}

impl ModifiersState {
    pub fn none() -> Self {
        Self::default()
    }
}

/// The pointer buttons the platform reports, in the order every element that
/// asks about one reads them. A left press is the primary button — the one a
/// click is made of — a right press is the one a context menu opens on, and
/// anything else is neither.
pub const BUTTON_PRIMARY: u32 = 0;
pub const BUTTON_SECONDARY: u32 = 1;

/// An OS-level event wrapped for dispatch in the element tree.
#[derive(Clone, Debug)]
pub enum DispatchedEvent {
    MouseDown {
        position: crate::geometry::Vector2F,
        button: u32,
    },
    MouseUp {
        position: crate::geometry::Vector2F,
        button: u32,
    },
    MouseMove {
        position: crate::geometry::Vector2F,
    },
    KeyDown {
        key: String,
        modifiers: ModifiersState,
    },
    KeyUp {
        key: String,
        modifiers: ModifiersState,
    },
    /// The window gained or lost the OS keyboard focus. A terminal can be asked
    /// to report this to the program that is running (mode 1004), which is how
    /// a TUI tells its user's absence from their presence.
    Focus {
        gained: bool,
    },
    Scroll {
        delta: crate::geometry::Vector2F,
        /// Whether the delta is a trackpad's precise pixel movement rather than
        /// a wheel's line notch. A precise delta carries the distance the
        /// fingers travelled, so a region that moves in whole rows can carry
        /// the fraction over to the next event instead of rounding every
        /// pixel up to a row.
        precise: bool,
    },
}

impl DispatchedEvent {
    /// A scroll delta with no gesture information, the shape a test or a caller
    /// that is not a platform wheel stream uses: a wheel notch, not a trackpad.
    pub fn scroll(delta: crate::geometry::Vector2F) -> Self {
        Self::Scroll {
            delta,
            precise: false,
        }
    }
}
