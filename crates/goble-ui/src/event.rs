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
    },
    KeyUp {
        key: String,
    },
    Scroll {
        delta: crate::geometry::Vector2F,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ModifiersState;
