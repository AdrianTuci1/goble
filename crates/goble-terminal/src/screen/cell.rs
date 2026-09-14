use alacritty_terminal::vte::ansi::NamedColor;

/// A colour a cell can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenColor {
    Named(NamedColor),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl ScreenColor {
    /// Whether this is one of the terminal's three default colours, which have
    /// no fixed RGB value: the renderer takes them from its theme rather than
    /// the palette.
    pub fn is_default(&self) -> bool {
        matches!(
            self,
            ScreenColor::Named(
                NamedColor::Foreground
                    | NamedColor::BrightForeground
                    | NamedColor::Background
                    | NamedColor::Cursor
                    | NamedColor::DimForeground
            )
        )
    }
}

/// Underline style, as set by `SGR 4:n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// Cell attributes the renderer needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: Underline,
    pub underline_color: Option<ScreenColor>,
    pub inverse: bool,
    pub hidden: bool,
    pub strikeout: bool,
    /// The cell holds the first half of a double-width glyph.
    pub wide: bool,
    /// The cell is the right half of a double-width glyph.
    pub wide_spacer: bool,
    /// A double-width glyph did not fit at the end of the row.
    pub leading_wide_spacer: bool,
    /// The row continues on the next one (soft wrap).
    pub wrapline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCell {
    pub ch: char,
    pub fg: ScreenColor,
    pub bg: ScreenColor,
    pub attrs: CellAttrs,
    /// Combining marks attached to this cell.
    pub zerowidth: Vec<char>,
    pub hyperlink: Option<String>,
}

impl Default for ScreenCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: ScreenColor::Named(NamedColor::Foreground),
            bg: ScreenColor::Named(NamedColor::Background),
            attrs: CellAttrs::default(),
            zerowidth: Vec::new(),
            hyperlink: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenLine {
    pub cells: Vec<ScreenCell>,
}

impl ScreenLine {
    /// The row's text with trailing blanks removed.
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.cells.len());
        for cell in &self.cells {
            let skip = cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer;
            if !skip {
                out.push(cell.ch);
            }
            for mark in &cell.zerowidth {
                out.push(*mark);
            }
        }
        out.truncate(out.trim_end().len());
        out
    }

    pub fn is_blank(&self) -> bool {
        self.cells
            .iter()
            .all(|c| c.ch == ' ' && c.zerowidth.is_empty())
    }
}
