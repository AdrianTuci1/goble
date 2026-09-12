use alacritty_terminal::grid::Dimensions;

/// Terminal size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ScreenSize {
    pub columns: usize,
    pub screen_lines: usize,
}

impl ScreenSize {
    pub const fn new(columns: usize, screen_lines: usize) -> Self {
        Self {
            columns,
            screen_lines,
        }
    }
}

impl Dimensions for ScreenSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}
