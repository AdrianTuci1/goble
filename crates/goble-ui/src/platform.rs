/// Placeholder for native windowing integration with `winit`.
///
/// A full implementation will create a `winit` event loop, handle DPI scaling,
/// and forward input events to the element tree.
#[derive(Default)]
pub struct WindowContext;

impl WindowContext {
    pub fn new() -> Self {
        Self
    }
}
