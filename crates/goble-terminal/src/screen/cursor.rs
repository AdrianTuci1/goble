/// The shape the application asked the cursor to take (`DECSCUSR`, `OSC 50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    /// A block drawn as an outline, which is what `DECSCUSR 0` asks for.
    HollowBlock,
    Beam,
    Underline,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    /// Row within the viewport.
    pub line: usize,
    pub column: usize,
    pub visible: bool,
    pub shape: CursorShape,
}
