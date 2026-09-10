/// Placeholder for per-platform application lifecycle hooks.
///
/// This mirrors the `platform::app` module in Warp's octomusui and can be
/// extended with window creation, menu integration, and clipboard handling.
#[derive(Default, Clone)]
pub struct AppContext;

impl AppContext {
    pub fn new() -> Self {
        Self
    }
}
