pub mod app;

#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

pub mod current {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            pub use super::mac::*;
        } else if #[cfg(target_os = "linux")] {
            pub use super::linux::*;
        } else if #[cfg(target_os = "windows")] {
            pub use super::windows::*;
        } else {
            pub use super::fallback::*;
        }
    }
}

pub mod fallback {
    //! Fallback implementations for unknown targets.
    //! These are the same heuristic-based metrics used everywhere until a native backend is added.

    use crate::geometry::Vector2F;

    pub fn default_font_family() -> &'static str {
        "system-ui"
    }

    pub fn estimate_text_size(
        text: &str,
        font_size: f32,
        line_height: f32,
        max_width: f32,
    ) -> Vector2F {
        const APPROX_CHAR_WIDTH_RATIO: f32 = 0.55;
        if text.is_empty() {
            return crate::geometry::vec2f(0.0, font_size * line_height);
        }
        let char_width = font_size * APPROX_CHAR_WIDTH_RATIO;
        let full_width = text.chars().count() as f32 * char_width;
        if full_width <= max_width || max_width.is_infinite() || max_width <= 0.0 {
            return crate::geometry::vec2f(full_width, font_size * line_height);
        }
        let chars_per_line = (max_width / char_width).max(1.0) as usize;
        let total_chars = text.chars().count();
        let raw_lines = (total_chars + chars_per_line - 1) / chars_per_line.max(1);
        let line_count = raw_lines.max(1);
        let width = (chars_per_line as f32 * char_width).min(full_width);
        crate::geometry::vec2f(width, font_size * line_height * line_count as f32)
    }
}
