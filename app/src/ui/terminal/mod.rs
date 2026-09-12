//! Terminal pane element.
//!
//! Paints a PTY-backed shell as its history: one section per command, each
//! headed by the directory it ran in and the time it took (the block view), with
//! the newest section sitting on the shared rich input bar the chat surface also
//! draws. A full-screen program owns its screen instead, so the alternate screen
//! — and a shell whose integration has not delimited a block yet — is painted as
//! the real cell grid the session reports. While the pane's harness is open it
//! draws the shared agent view instead — see [`build_terminal`].
//!
//! The pane owns no blocking I/O: every layout frame it lazily ensures a session
//! exists for the pane, tells it the grid size the pane measured and paints what
//! it reports back.
//!
//! One module per surface of the pane: the tree it builds (`build`), the keys
//! and pointer events it routes (`input`), its [`Element`] impl (`element`), the
//! conversation cards left in its block list (`cards`) and the probe that records
//! where the grid landed (`grid`). The block a pane's own command draws as
//! (`blocks`) is the same block the transcript draws. What the surfaces share —
//! the pane's own view, its callbacks and the grid metrics — stays here.

use std::cell::RefCell;
use std::rc::Rc;

use goble_terminal::blocks::BlockView;
use goble_terminal::MouseButton;
use goble_ui::elements::{Element, Point, TerminalBlockPlumbing};
use goble_ui::geometry::Vector2F;
use goble_ui::ScrollState;

use crate::terminal::TerminalRegistry;

use cards::CardRect;
use grid::GridGeometry;

mod blocks;
mod build;
mod cards;
mod element;
mod grid;
mod input;

#[cfg(test)]
mod tests;

pub use blocks::executed_command_block;
pub use build::build_terminal;

const FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.35;

struct TerminalView {
    terminal: Rc<RefCell<TerminalRegistry>>,
    pane_id: u64,
    cwd: String,
    active: bool,
    /// The filter this pane's block list is drawn through: the terminal, or one
    /// conversation's agent view.
    view: BlockView,
    /// The shared rich input bar pinned to the bottom of the pane, taken by the
    /// frame that draws it (the tree is rebuilt from scratch every frame).
    bar: Option<Box<dyn Element>>,
    /// Whether the bar's editor holds the keyboard, so the pane hands keys to
    /// the bar instead of encoding them for the shell. While they disagree the
    /// grid keeps the keys, which is what a click on the grid restores.
    composer_focused: bool,
    on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    on_route: Rc<RefCell<dyn FnMut(u64, String)>>,
    on_harness_mode: Rc<RefCell<dyn FnMut(u64, bool)>>,
    on_open_agent_view: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Where the grid ended up, written by the grid as it paints; a pointer
    /// event is turned into a cell against this, not against a second guess at
    /// the flex layout.
    geometry: Rc<RefCell<Option<GridGeometry>>>,
    /// The conversation cards drawn in the terminal view this frame, with where
    /// each landed, so a click on a card reopens its conversation.
    cards: Rc<RefCell<Vec<CardRect>>>,
    /// The scroll offset of the pane's command sections (its block view), owned
    /// by app state so a scrollback position the user chose survives the
    /// per-frame rebuild.
    scroll: Rc<RefCell<ScrollState>>,
    /// How the pane draws a terminal block: the app-owned per-block filter map,
    /// this pane's whole-history filter and the app's copy handler.
    plumbing: TerminalBlockPlumbing,
    /// The button held down, so motion is reported as a drag while it is.
    pressed: Option<MouseButton>,
    /// The last cell the pointer was over. A wheel event carries no position,
    /// and a release can arrive after the pointer left the pane.
    last_cell: Option<(usize, usize)>,
    /// Where the pointer was last seen, which is how a wheel event — which has
    /// no position of its own — is attributed to this pane.
    pointer: Option<Vector2F>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// The mouse button an event id means: the platform layer numbers them left,
/// right, then everything else as middle.
fn mouse_button(id: u32) -> Option<MouseButton> {
    match id {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Right),
        2 => Some(MouseButton::Middle),
        _ => None,
    }
}

