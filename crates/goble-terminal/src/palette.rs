//! Resolving a cell colour to RGB.
//!
//! The emulator reports a colour the way the application wrote it — a name, a
//! palette index, or a literal RGB triple — and the renderer needs bytes. The
//! translation is terminal semantics, not look and feel, so it lives here
//! rather than in the widget layer: the 16-colour table, the 6×6×6 cube and the
//! 24-step grey ramp are fixed by the `xterm` palette, while the three default
//! colours are policy the application sets from its theme.
//!
//! Bold-as-bright, blink and the underline style are *not* here: they are
//! attributes, and the renderer applies them.

use alacritty_terminal::vte::ansi::NamedColor;

use crate::screen::{ScreenCell, ScreenColor};

/// The 16 ANSI colours as `xterm` defines them, normal then bright.
pub const ANSI_16: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

/// How much of its brightness a dimmed colour keeps.
const DIM: u16 = 2;

/// A terminal palette: the 16 colours plus the three defaults that have no
/// palette index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub foreground: (u8, u8, u8),
    pub background: (u8, u8, u8),
    pub cursor: (u8, u8, u8),
    pub ansi: [(u8, u8, u8); 16],
}

impl Default for Palette {
    fn default() -> Self {
        Self::xterm()
    }
}

impl Palette {
    /// The `xterm` defaults: a light grey on black.
    pub const fn xterm() -> Self {
        Self {
            foreground: (229, 229, 229),
            background: (0, 0, 0),
            cursor: (229, 229, 229),
            ansi: ANSI_16,
        }
    }

    /// Re-colour the three defaults, leaving the palette alone.
    pub const fn with_defaults(
        mut self,
        foreground: (u8, u8, u8),
        background: (u8, u8, u8),
        cursor: (u8, u8, u8),
    ) -> Self {
        self.foreground = foreground;
        self.background = background;
        self.cursor = cursor;
        self
    }

    /// Resolve a colour the emulator reported.
    pub fn rgb(&self, color: ScreenColor) -> (u8, u8, u8) {
        match color {
            ScreenColor::Rgb(r, g, b) => (r, g, b),
            ScreenColor::Indexed(index) => self.indexed(index),
            ScreenColor::Named(named) => self.named(named),
        }
    }

    /// Resolve a palette index: the 16 colours, then the 6×6×6 cube, then the
    /// 24 greys, which is what the upper 240 indices mean.
    pub fn indexed(&self, index: u8) -> (u8, u8, u8) {
        match index {
            0..=15 => self.ansi[index as usize],
            16..=231 => {
                let index = index - 16;
                // Each channel steps through 0, 95, 135, 175, 215, 255.
                let level = |value: u8| -> u8 {
                    match value {
                        0 => 0,
                        n => 55 + 40 * n,
                    }
                };
                (level(index / 36), level((index % 36) / 6), level(index % 6))
            }
            _ => {
                // 232..=255 are greys from 8 to 238 in steps of 10.
                let level = 8 + 10 * (index - 232);
                (level, level, level)
            }
        }
    }

    fn named(&self, named: NamedColor) -> (u8, u8, u8) {
        let (index, dim) = match named {
            NamedColor::Black | NamedColor::DimBlack => (0, named.dim_variant()),
            NamedColor::Red | NamedColor::DimRed => (1, named.dim_variant()),
            NamedColor::Green | NamedColor::DimGreen => (2, named.dim_variant()),
            NamedColor::Yellow | NamedColor::DimYellow => (3, named.dim_variant()),
            NamedColor::Blue | NamedColor::DimBlue => (4, named.dim_variant()),
            NamedColor::Magenta | NamedColor::DimMagenta => (5, named.dim_variant()),
            NamedColor::Cyan | NamedColor::DimCyan => (6, named.dim_variant()),
            NamedColor::White | NamedColor::DimWhite => (7, named.dim_variant()),
            NamedColor::BrightBlack => (8, false),
            NamedColor::BrightRed => (9, false),
            NamedColor::BrightGreen => (10, false),
            NamedColor::BrightYellow => (11, false),
            NamedColor::BrightBlue => (12, false),
            NamedColor::BrightMagenta => (13, false),
            NamedColor::BrightCyan => (14, false),
            NamedColor::BrightWhite => (15, false),
            NamedColor::Foreground | NamedColor::BrightForeground => return self.foreground,
            NamedColor::Background => return self.background,
            NamedColor::Cursor => return self.cursor,
            NamedColor::DimForeground => return dimmed(self.foreground),
        };
        let color = self.ansi[index];
        if dim {
            dimmed(color)
        } else {
            color
        }
    }

    /// The foreground and background a cell paints with, after the attributes
    /// that swap or hide them.
    ///
    /// Inverse swaps the two; a hidden cell paints its text in the background
    /// colour, which is how a password stays on screen as blanks; dim darkens
    /// the text without touching the background.
    pub fn cell_colors(&self, cell: &ScreenCell) -> ((u8, u8, u8), (u8, u8, u8)) {
        let mut foreground = self.rgb(cell.fg);
        let mut background = self.rgb(cell.bg);
        if cell.attrs.inverse {
            std::mem::swap(&mut foreground, &mut background);
        }
        if cell.attrs.dim {
            foreground = dimmed(foreground);
        }
        if cell.attrs.hidden {
            foreground = background;
        }
        (foreground, background)
    }
}

fn dimmed(color: (u8, u8, u8)) -> (u8, u8, u8) {
    let channel = |value: u8| ((value as u16 * DIM) / 3) as u8;
    (channel(color.0), channel(color.1), channel(color.2))
}

/// `NamedColor` has no predicate for it; the dim variants are the low eight
/// plus the dim foreground, which is exactly the set below
/// `DimBlack`..`BrightForeground`.
trait IsDim {
    fn dim_variant(self) -> bool;
}

impl IsDim for NamedColor {
    fn dim_variant(self) -> bool {
        matches!(
            self,
            NamedColor::DimBlack
                | NamedColor::DimRed
                | NamedColor::DimGreen
                | NamedColor::DimYellow
                | NamedColor::DimBlue
                | NamedColor::DimMagenta
                | NamedColor::DimCyan
                | NamedColor::DimWhite
                | NamedColor::DimForeground
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::CellAttrs;

    fn cell(fg: ScreenColor, bg: ScreenColor) -> ScreenCell {
        ScreenCell {
            fg,
            bg,
            ..ScreenCell::default()
        }
    }

    #[test]
    fn named_colours_use_the_ansi_table() {
        let palette = Palette::xterm();
        assert_eq!(
            palette.rgb(ScreenColor::Named(NamedColor::Red)),
            (205, 0, 0)
        );
        assert_eq!(
            palette.rgb(ScreenColor::Named(NamedColor::BrightRed)),
            (255, 0, 0)
        );
        assert_eq!(
            palette.rgb(ScreenColor::Named(NamedColor::Foreground)),
            palette.foreground
        );
        assert_eq!(
            palette.rgb(ScreenColor::Named(NamedColor::Background)),
            palette.background
        );
    }

    #[test]
    fn the_first_sixteen_indices_are_the_ansi_table() {
        let palette = Palette::xterm();
        for index in 0..16u8 {
            assert_eq!(palette.indexed(index), ANSI_16[index as usize]);
        }
    }

    #[test]
    fn the_cube_ends_are_right() {
        let palette = Palette::xterm();
        assert_eq!(palette.indexed(16), (0, 0, 0));
        assert_eq!(palette.indexed(21), (0, 0, 255));
        assert_eq!(palette.indexed(231), (255, 255, 255));
    }

    #[test]
    fn the_greys_run_from_eight_to_two_hundred_and_thirty_eight() {
        let palette = Palette::xterm();
        assert_eq!(palette.indexed(232), (8, 8, 8));
        assert_eq!(palette.indexed(255), (238, 238, 238));
    }

    #[test]
    fn truecolor_is_verbatim() {
        let palette = Palette::xterm();
        assert_eq!(palette.rgb(ScreenColor::Rgb(1, 2, 3)), (1, 2, 3));
    }

    #[test]
    fn inverse_swaps_the_two_colours() {
        let palette = Palette::xterm();
        let mut c = cell(
            ScreenColor::Named(NamedColor::Red),
            ScreenColor::Rgb(0, 0, 0),
        );
        c.attrs.inverse = true;
        assert_eq!(palette.cell_colors(&c), ((0, 0, 0), (205, 0, 0)));
    }

    #[test]
    fn a_hidden_cell_paints_its_text_in_the_background_colour() {
        let palette = Palette::xterm();
        let mut c = cell(
            ScreenColor::Named(NamedColor::White),
            ScreenColor::Rgb(10, 20, 30),
        );
        c.attrs.hidden = true;
        let (foreground, background) = palette.cell_colors(&c);
        assert_eq!(foreground, background);
        assert_eq!(background, (10, 20, 30));
    }

    #[test]
    fn dim_darkens_the_text_only() {
        let palette = Palette::xterm();
        let mut c = cell(ScreenColor::Rgb(90, 60, 30), ScreenColor::Rgb(1, 1, 1));
        c.attrs.dim = true;
        assert_eq!(palette.cell_colors(&c), ((60, 40, 20), (1, 1, 1)));
    }

    #[test]
    fn a_named_dim_colour_is_already_dimmed() {
        let palette = Palette::xterm();
        let plain = palette.rgb(ScreenColor::Named(NamedColor::Red));
        let dim = palette.rgb(ScreenColor::Named(NamedColor::DimRed));
        assert!(dim.0 < plain.0 && dim.1 <= plain.1 && dim.2 <= plain.2);
    }

    #[test]
    fn defaults_can_be_re_coloured() {
        let palette = Palette::xterm().with_defaults((1, 2, 3), (4, 5, 6), (7, 8, 9));
        assert_eq!(
            palette.rgb(ScreenColor::Named(NamedColor::Foreground)),
            (1, 2, 3)
        );
        assert_eq!(
            palette.rgb(ScreenColor::Named(NamedColor::Background)),
            (4, 5, 6)
        );
        assert_eq!(
            palette.rgb(ScreenColor::Named(NamedColor::Cursor)),
            (7, 8, 9)
        );
        // The palette itself is untouched.
        assert_eq!(palette.indexed(1), ANSI_16[1]);
    }

    #[test]
    fn attributes_are_read_from_the_cell() {
        let palette = Palette::xterm();
        let plain = cell(
            ScreenColor::Named(NamedColor::Foreground),
            ScreenColor::Named(NamedColor::Background),
        );
        let (fg, bg) = palette.cell_colors(&plain);
        assert_eq!(fg, palette.foreground);
        assert_eq!(bg, palette.background);
        assert!(!CellAttrs::default().dim);
    }
}
