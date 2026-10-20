//! Terminal palette and color resolution.
//!
//! The browser client mirrors the native app's terminal-first theme: a
//! near-black canvas, lifted panels, and a warm peach accent. Colors are plain
//! data here; the canvas renderer turns them into CSS strings.

use vt100::Color;

/// An sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `#rrggbb`, the form the 2D canvas accepts.
    #[must_use]
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Mix toward `other` by `amount` in `0..=255`.
    #[must_use]
    pub fn blend(self, other: Rgb, amount: u8) -> Rgb {
        let mix = |a: u8, b: u8| -> u8 {
            let amount = u16::from(amount);
            ((u16::from(a) * (255 - amount) + u16::from(b) * amount) / 255) as u8
        };
        Rgb::new(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }
}

/// The 16 ANSI slots plus UI chrome colors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub background: Rgb,
    pub panel: Rgb,
    pub foreground: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub border: Rgb,
    pub selection: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// Default dark theme, shared with the native app's terminal identity.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            background: Rgb::new(0x0d, 0x0f, 0x12),
            panel: Rgb::new(0x15, 0x18, 0x1d),
            foreground: Rgb::new(0xc8, 0xcc, 0xd4),
            muted: Rgb::new(0x7b, 0x84, 0x94),
            accent: Rgb::new(0xf2, 0xb3, 0x8a),
            border: Rgb::new(0x26, 0x2b, 0x33),
            selection: Rgb::new(0x2d, 0x39, 0x4b),
            cursor: Rgb::new(0xf2, 0xb3, 0x8a),
            ansi: [
                Rgb::new(0x1c, 0x20, 0x27),
                Rgb::new(0xe0, 0x6c, 0x75),
                Rgb::new(0x98, 0xc3, 0x79),
                Rgb::new(0xe5, 0xc0, 0x7b),
                Rgb::new(0x61, 0xaf, 0xef),
                Rgb::new(0xc6, 0x78, 0xdd),
                Rgb::new(0x56, 0xb6, 0xc2),
                Rgb::new(0xc8, 0xcc, 0xd4),
                Rgb::new(0x5c, 0x63, 0x70),
                Rgb::new(0xff, 0x7b, 0x86),
                Rgb::new(0xb4, 0xe3, 0x91),
                Rgb::new(0xf0, 0xd0, 0x8a),
                Rgb::new(0x82, 0xc4, 0xff),
                Rgb::new(0xd9, 0x9b, 0xf0),
                Rgb::new(0x7b, 0xd6, 0xe0),
                Rgb::new(0xee, 0xf1, 0xf6),
            ],
        }
    }

    /// Resolve a cell foreground. `bold` promotes the first eight ANSI slots
    /// to their bright counterparts, as classic terminals do.
    #[must_use]
    pub fn foreground_for(&self, color: Color, bold: bool) -> Rgb {
        match color {
            Color::Default => self.foreground,
            Color::Idx(index) => {
                let index = if bold && index < 8 { index + 8 } else { index };
                self.index(index)
            }
            Color::Rgb(r, g, b) => Rgb::new(r, g, b),
        }
    }

    /// Resolve a cell background. Bold does not brighten backgrounds.
    #[must_use]
    pub fn background_for(&self, color: Color) -> Rgb {
        match color {
            Color::Default => self.background,
            other => self.foreground_for(other, false),
        }
    }

    /// 256-color lookup: 16 palette entries, a 6×6×6 cube, then a 24-step
    /// grayscale ramp.
    #[must_use]
    pub fn index(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                let offset = index - 16;
                let steps = [0u8, 95, 135, 175, 215, 255];
                let r = steps[usize::from(offset / 36) % 6];
                let g = steps[usize::from(offset / 6) % 6];
                let b = steps[usize::from(offset % 6)];
                Rgb::new(r, g, b)
            }
            _ => {
                let level = 8 + 10 * (index - 232);
                Rgb::new(level, level, level)
            }
        }
    }

    /// Dim text, used for the SGR dim attribute.
    #[must_use]
    pub fn dimmed(&self, color: Rgb) -> Rgb {
        color.blend(self.background, 110)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_has_a_warm_accent() {
        let theme = Theme::dark();
        assert_eq!(theme.accent, Rgb::new(0xf2, 0xb3, 0x8a));
        assert_eq!(theme.background, Rgb::new(0x0d, 0x0f, 0x12));
        assert_eq!(theme.panel, Rgb::new(0x15, 0x18, 0x1d));
        assert_eq!(theme.ansi.len(), 16);
    }

    #[test]
    fn default_colors_use_theme_colors() {
        let theme = Theme::dark();
        assert_eq!(
            theme.foreground_for(Color::Default, false),
            theme.foreground
        );
        assert_eq!(theme.background_for(Color::Default), theme.background);
    }

    #[test]
    fn indexed_palette_colors() {
        let theme = Theme::dark();
        assert_eq!(theme.index(1), theme.ansi[1]);
        assert_eq!(theme.index(15), theme.ansi[15]);
    }

    #[test]
    fn bold_promotes_low_indexes_only() {
        let theme = Theme::dark();
        assert_eq!(theme.foreground_for(Color::Idx(1), true), theme.ansi[9]);
        assert_eq!(theme.foreground_for(Color::Idx(9), true), theme.ansi[9]);
        assert_eq!(theme.background_for(Color::Idx(1)), theme.ansi[1]);
    }

    #[test]
    fn color_cube_edges() {
        let theme = Theme::dark();
        assert_eq!(theme.index(16), Rgb::new(0, 0, 0));
        assert_eq!(theme.index(21), Rgb::new(0, 0, 255));
        assert_eq!(theme.index(196), Rgb::new(255, 0, 0));
        assert_eq!(theme.index(231), Rgb::new(255, 255, 255));
    }

    #[test]
    fn grayscale_ramp() {
        let theme = Theme::dark();
        assert_eq!(theme.index(232), Rgb::new(8, 8, 8));
        assert_eq!(theme.index(255), Rgb::new(238, 238, 238));
    }

    #[test]
    fn truecolor_passes_through() {
        let theme = Theme::dark();
        assert_eq!(
            theme.foreground_for(Color::Rgb(1, 2, 3), false),
            Rgb::new(1, 2, 3)
        );
        assert_eq!(theme.background_for(Color::Rgb(4, 5, 6)), Rgb::new(4, 5, 6));
    }

    #[test]
    fn hex_formatting() {
        assert_eq!(Rgb::new(0, 10, 255).hex(), "#000aff");
    }

    #[test]
    fn blend_moves_toward_the_target() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(255, 255, 255);
        assert_eq!(a.blend(b, 0), a);
        assert_eq!(a.blend(b, 255), b);
        assert_eq!(a.blend(b, 128), Rgb::new(128, 128, 128));
    }

    #[test]
    fn dim_moves_text_toward_the_background() {
        let theme = Theme::dark();
        let dim = theme.dimmed(theme.foreground);
        assert!(dim.r < theme.foreground.r);
        assert!(dim.r > theme.background.r);
    }
}
