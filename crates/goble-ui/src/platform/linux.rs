//! Linux platform hooks for goble-ui.
//!
//! Currently the crate does not use native font APIs, so these re-export the
//! fallback text metrics. In the future this module can switch to
//! cosmic-text / fontdb / swash_rasterizer based measurement, following Warp.

pub use super::fallback::*;
