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

/// The window two presses on one point are a single gesture in, counted
/// press-to-press so the time a press is held does not come out of it. The same
/// 400 ms the app's workspace tabs use (`MULTI_CLICK_INTERVAL_MS`) and the
/// terminal's own selection counts with.
pub const MULTI_CLICK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(400);

/// How far the pointer may travel between the two presses of a double click and
/// still be the same gesture, in logical pixels.
pub const MULTI_CLICK_SLOP: f32 = 4.0;

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
    /// The second release of a multi-click run, reported after the
    /// [`Self::MouseUp`] that ends it.
    ///
    /// It is its own variant rather than a count carried on the press, so a
    /// caller that knows no multi-click — a test, a scripted dispatch — keeps
    /// building the same press and release it always did. The release comes
    /// first on purpose: the row under the pointer has already run its own
    /// click when this lands, which is what lets a control that opens on a
    /// click close again on the double click.
    MouseDoubleClick {
        position: crate::geometry::Vector2F,
        button: u32,
    },
    KeyDown {
        key: String,
        modifiers: ModifiersState,
    },
    KeyUp {
        key: String,
        modifiers: ModifiersState,
    },
    /// The keyboard modifiers changed without a key carrying the change.
    ///
    /// A modifier key reports itself here and not as a [`Self::KeyDown`]: press
    /// and release Command and there is no key event at all, only this. The app
    /// reads it to know the chord modifier is held (the workspace tabs advertise
    /// their shortcut numbers while it is), which no key press can tell it.
    Modifiers {
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
