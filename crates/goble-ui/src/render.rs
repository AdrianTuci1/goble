/// Placeholder for the wgpu-based 2D renderer.
///
/// A full implementation will maintain a `wgpu::Device`, `Queue`, `Surface`,
/// and a simple 2D pipeline for solid rectangles, rounded corners, text, and icons.
#[derive(Default)]
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }
}
