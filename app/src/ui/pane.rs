use goble_ui::geometry::{rectf, RectF};
use goble_ui::theme::TabColor;
use goble_ui::Vector2F;

use super::SettingsCategory;

/// How a space arranges its leaves.
///
/// `Bsp` is the binary split tree every space has always had: each split has
/// its own direction and ratio and a divider the user can drag. `MasterStack`
/// draws the first leaf at ~60% of the width and stacks every other leaf in
/// the remaining column. `Floating` gives each leaf a rect of its own, in a
/// persisted z-order; a click raises the pane it lands on. `Carousel` lays
/// every leaf out as the same sized window in one horizontal row, like a
/// coverflow that pans rather than zooms: the focused pane sits centered and
/// on top, and clicking a neighbour brings it to the center.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    #[default]
    Bsp,
    MasterStack,
    Floating,
    Carousel,
}

impl LayoutKind {
    /// The next layout Cmd+Shift+L selects, in the order the cycle names them.
    pub fn cycle(self) -> Self {
        match self {
            Self::Bsp => Self::MasterStack,
            Self::MasterStack => Self::Floating,
            Self::Floating => Self::Carousel,
            Self::Carousel => Self::Bsp,
        }
    }

    /// The layout's name in the command palette and any report.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bsp => "BSP",
            Self::MasterStack => "Master stack",
            Self::Floating => "Floating",
            Self::Carousel => "Carousel",
        }
    }
}

/// A normalized rect in a space's pane area, `0..1` on both axes. Stored for
/// floating panes so a rect survives a restart independently of the window
/// size it was dragged in.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PaneRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Clamp the rect into the unit square, keeping a pane at least
    /// [`Self::MIN_W`] × [`Self::MIN_H`] of its area.
    pub fn clamped(self) -> Self {
        let w = self.w.clamp(Self::MIN_W, 1.0);
        let h = self.h.clamp(Self::MIN_H, 1.0);
        let x = self.x.clamp(0.0, (1.0 - w).max(0.0));
        let y = self.y.clamp(0.0, (1.0 - h).max(0.0));
        Self { x, y, w, h }
    }

    /// The rect in pixels inside an area of `area`.
    pub fn to_pixels(self, area: Vector2F) -> RectF {
        let rect = self.clamped();
        let max_w = area.x.max(1.0);
        let max_h = area.y.max(1.0);
        let w = (rect.w * max_w).max(Self::MIN_PX).min(max_w);
        let h = (rect.h * max_h).max(Self::MIN_PX).min(max_h);
        let x = (rect.x * max_w).clamp(0.0, (max_w - w).max(0.0));
        let y = (rect.y * max_h).clamp(0.0, (max_h - h).max(0.0));
        rectf(x, y, w, h)
    }

    /// The rect in normalized `0..1` coordinates, the space the layout math
    /// and the stored rect share.
    pub fn to_rect(self) -> RectF {
        let rect = self.clamped();
        rectf(rect.x, rect.y, rect.w, rect.h)
    }

    /// The rect after a pointer drag from [`FloatingDrag`], in normalized
    /// coordinates.
    pub fn dragged(self, mode: FloatingDragMode, delta: Vector2F, area: Vector2F) -> Self {
        let dx = delta.x / area.x.max(1.0);
        let dy = delta.y / area.y.max(1.0);
        let rect = match mode {
            FloatingDragMode::Move => Self {
                x: self.x + dx,
                y: self.y + dy,
                ..self
            },
            FloatingDragMode::Resize => Self {
                w: self.w + dx,
                h: self.h + dy,
                ..self
            },
        };
        rect.clamped()
    }

    const MIN_W: f32 = 0.2;
    const MIN_H: f32 = 0.2;
    const MIN_PX: f32 = 120.0;
}

/// One floating pane and the rect it occupies. The order of [`Space::floating`]
/// is the z-order: the last entry is drawn on top and receives pointer events
/// first.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FloatingPane {
    pub pane_id: u64,
    pub rect: PaneRect,
    /// A minimized window keeps its rect and its place in the z-order, but is
    /// drawn as a chip in the dock rather than over the pane area. The flag
    /// defaults to false so layouts saved before the dock existed load as
    /// ordinary windows.
    #[serde(default)]
    pub minimized: bool,
}

impl FloatingPane {
    pub fn new(pane_id: u64, rect: PaneRect) -> Self {
        Self {
            pane_id,
            rect,
            minimized: false,
        }
    }
}

/// Which drag a floating pane's pointer gesture is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatingDragMode {
    /// The header was pressed: move the pane.
    Move,
    /// The bottom-right grip was pressed: resize the pane.
    Resize,
}

/// A floating pane drag in progress. Owned by app state (not the per-frame
/// element) so the gesture survives the rebuild, like [`crate::state::PaneDrag`].
#[derive(Clone, Debug, PartialEq)]
pub struct FloatingDrag {
    pub pane_id: u64,
    pub mode: FloatingDragMode,
    /// Where the pointer went down.
    pub start: Vector2F,
    /// The pane's rect when the drag began.
    pub original: PaneRect,
    /// The space's pane area in pixels: it converts the pointer's pixels into
    /// the normalized rect that is persisted.
    pub area: Vector2F,
}

impl FloatingDrag {
    /// The pane's rect for a pointer at `position`.
    pub fn rect_at(&self, position: Vector2F) -> PaneRect {
        let delta = goble_ui::geometry::vec2f(position.x - self.start.x, position.y - self.start.y);
        self.original.dragged(self.mode, delta, self.area)
    }
}

/// Which kind of content a pane hosts.
///
/// Not `Copy`: a file view carries the file it shows, so the pane survives a
/// save/load with the same file open instead of coming back empty.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PaneKind {
    Chat,
    Terminal,
    /// A conversation running on a remote worker: a viewer session with no
    /// local shell.
    ///
    /// Nothing local backs the pane. There is no PTY to spawn, no local cwd to
    /// read and no shell to fall back to, so a dropped connection has nothing to
    /// degrade into — the pane draws the connection's own state instead (see
    /// [`crate::state::PaneAttach`]) and keeps its conversation, which the worker
    /// goes on running. The pane is built as a conversation surface, never as
    /// [`PaneKind::Terminal`]: `build_leaf` routes a viewer pane to
    /// [`crate::ui::worker::build_worker_pane`], which mounts no pty.
    ///
    /// `UiState::adopt_routing` is the one place a leaf becomes a viewer pane
    /// (a conversation whose routing resolves to a worker) and the one place it
    /// stops being one (the routing goes back to local — the pane then returns
    /// as a plain chat pane, never as a shell).
    Worker,
    /// A read-only view of one file, opened from the explorer tree or a search
    /// result.
    File { path: String },
    /// The settings surface: the page rail, the page and the footer. The page
    /// the pane shows rides in the leaf, so it comes back with the pane and
    /// travels with it when the pane is lifted into another space.
    Settings { page: SettingsCategory },
}

/// Which way a pane tree splits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SplitDir {
    /// First pane on the left / top; a horizontal split ("in 2").
    Horizontal,
    /// First pane on top / bottom; a vertical split ("down").
    Vertical,
}

/// The side of a pane a lifted pane would be inserted on: the edge of the pane
/// under the pointer. Read from where the pointer is relative to that pane's
/// center (warp-new's `calculate_pane_move_direction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl DropSide {
    /// The split a pane inserted on this side makes.
    pub fn split_dir(self) -> SplitDir {
        match self {
            DropSide::Left | DropSide::Right => SplitDir::Horizontal,
            DropSide::Top | DropSide::Bottom => SplitDir::Vertical,
        }
    }

    /// Whether the inserted pane becomes the split's first (left/top) child.
    pub fn leads(self) -> bool {
        matches!(self, DropSide::Left | DropSide::Top)
    }
}

/// The pane a lifted pane would land beside, and on which side of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneDropTarget {
    pub pane_id: u64,
    pub side: DropSide,
}

/// How far the pointer must leave a pane's center, as a share of the pane's own
/// width/height, before an edge is named: warp-new's `DRAG_SPLIT_THRESHOLD`.
/// Closer than this to the center the drop names no side at all, so a release
/// there cancels instead of splitting on a gesture the user did not finish.
pub const DROP_SIDE_THRESHOLD: f32 = 0.18;

/// The side of `pane` that `position` is on, or `None` while the pointer is
/// within [`DROP_SIDE_THRESHOLD`] of the pane's center.
pub fn drop_side_in(pane: RectF, position: Vector2F) -> Option<DropSide> {
    let center = pane.center();
    let dx = (position.x - center.x) / pane.width().max(1.0);
    let dy = (position.y - center.y) / pane.height().max(1.0);
    if dx.abs().max(dy.abs()) < DROP_SIDE_THRESHOLD {
        return None;
    }
    if dy.abs() > dx.abs() {
        Some(if dy > 0.0 { DropSide::Bottom } else { DropSide::Top })
    } else {
        Some(if dx > 0.0 { DropSide::Right } else { DropSide::Left })
    }
}

/// The pane under `position` and the side the pointer names on it, ignoring the
/// pane being dragged (it is not a target of its own move). `None` when the
/// pointer is over no other pane, or too close to one's center.
pub fn pane_drop_target(
    bounds: &std::collections::HashMap<u64, RectF>,
    position: Vector2F,
    dragged: u64,
) -> Option<PaneDropTarget> {
    bounds.iter().find_map(|(id, rect)| {
        if *id == dragged || !goble_ui::elements::interactive::contains(*rect, position) {
            return None;
        }
        drop_side_in(*rect, position).map(|side| PaneDropTarget {
            pane_id: *id,
            side,
        })
    })
}

/// The rectangle the drop ghost covers inside `pane` for `side`: the half of
/// the pane the moved pane would fill.
pub fn drop_ghost_rect(pane: RectF, side: DropSide) -> RectF {
    let half_w = pane.width() / 2.0;
    let half_h = pane.height() / 2.0;
    match side {
        DropSide::Left => rectf(pane.min_x(), pane.min_y(), half_w, pane.height()),
        DropSide::Right => rectf(pane.min_x() + half_w, pane.min_y(), half_w, pane.height()),
        DropSide::Top => rectf(pane.min_x(), pane.min_y(), pane.width(), half_h),
        DropSide::Bottom => rectf(pane.min_x(), pane.min_y() + half_h, pane.width(), half_h),
    }
}

/// A compass direction used to move focus between panes (Ctrl/Cmd+Arrow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavDir {
    Up,
    Down,
    Left,
    Right,
}

/// A pane in a space. A leaf hosts content; a `Split` divides into two panes.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Pane {
    Leaf { id: u64, kind: PaneKind },
    Split {
        id: u64,
        dir: SplitDir,
        ratio: f32,
        first: Box<Pane>,
        second: Box<Pane>,
    },
}

impl Pane {
    fn leaf(id: u64, kind: PaneKind) -> Self {
        Pane::Leaf { id, kind }
    }

    /// This pane's own id (leaf id or split id).
    pub fn id(&self) -> u64 {
        match self {
            Pane::Leaf { id, .. } | Pane::Split { id, .. } => *id,
        }
    }

    /// The id of the first (top/leftmost) leaf under this pane.
    pub fn first_leaf_id(&self) -> u64 {
        match self {
            Pane::Leaf { id, .. } => *id,
            Pane::Split { first, .. } => first.first_leaf_id(),
        }
    }

    /// The maximum pane id anywhere in the subtree.
    pub fn max_id(&self) -> u64 {
        match self {
            Pane::Leaf { id, .. } => *id,
            Pane::Split {
                id,
                first,
                second,
                ..
            } => (*id).max(first.max_id()).max(second.max_id()),
        }
    }

    /// Whether a leaf with id `target` exists in the subtree.
    pub fn contains_leaf(&self, target: u64) -> bool {
        match self {
            Pane::Leaf { id, .. } => *id == target,
            Pane::Split { first, second, .. } => first.contains_leaf(target) || second.contains_leaf(target),
        }
    }

    /// How many leaf panes (visible panes) the subtree holds.
    pub fn leaf_count(&self) -> usize {
        match self {
            Pane::Leaf { .. } => 1,
            Pane::Split { first, second, .. } => first.leaf_count() + second.leaf_count(),
        }
    }

    /// Every leaf in traversal order, with its kind. The master-stack layout
    /// reads the first of them as the master and the rest as the stack.
    pub fn leaves(&self, out: &mut Vec<(u64, PaneKind)>) {
        match self {
            Pane::Leaf { id, kind } => out.push((*id, kind.clone())),
            Pane::Split { first, second, .. } => {
                first.leaves(out);
                second.leaves(out);
            }
        }
    }

    /// Every file view in this subtree as `(pane id, path)`, in traversal order.
    /// What the surfaces that read the machine (the frame's file cache) look at.
    pub fn file_leaves(&self, out: &mut Vec<(u64, String)>) {
        match self {
            Pane::Leaf {
                id,
                kind: PaneKind::File { path },
            } => out.push((*id, path.clone())),
            Pane::Leaf { .. } => {}
            Pane::Split { first, second, .. } => {
                first.file_leaves(out);
                second.file_leaves(out);
            }
        }
    }

    fn is_leaf(&self, target: u64) -> bool {
        matches!(self, Pane::Leaf { id, .. } if *id == target)
    }

    /// The leaf that already sits immediately on `target`'s `side`, if it is a
    /// leaf. What makes a move a no-op: dropping a pane back onto the side of
    /// its own sibling would only rebuild the same split.
    fn sibling_on_side(&self, target: u64, side: DropSide) -> Option<u64> {
        let Pane::Split {
            dir, first, second, ..
        } = self
        else {
            return None;
        };
        if *dir == side.split_dir() {
            // `target` must be the child on the far side of the split: the near
            // child is then the pane already sitting where the move would land.
            let sibling = match side {
                DropSide::Left | DropSide::Top => second.is_leaf(target).then_some(first.as_ref()),
                DropSide::Right | DropSide::Bottom => {
                    first.is_leaf(target).then_some(second.as_ref())
                }
            };
            if let Some(sibling) = sibling {
                let id = sibling.id();
                return sibling.is_leaf(id).then_some(id);
            }
        }
        first
            .sibling_on_side(target, side)
            .or_else(|| second.sibling_on_side(target, side))
    }

    /// Replace the leaf `target` with a `Split` whose first child is `moved`
    /// when `moved_first` is set (and `target` otherwise). The target leaf keeps
    /// its own id and kind; the moved pane arrives as the very same leaf.
    fn insert_leaf_beside(
        &mut self,
        target: u64,
        dir: SplitDir,
        moved_first: bool,
        split_id: u64,
        moved: Pane,
    ) -> bool {
        match self {
            Pane::Leaf { id, kind } if *id == target => {
                let target_leaf = Pane::leaf(*id, kind.clone());
                let (first, second) = if moved_first {
                    (moved, target_leaf)
                } else {
                    (target_leaf, moved)
                };
                *self = Pane::Split {
                    id: split_id,
                    dir,
                    ratio: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            Pane::Split { first, second, .. } => {
                first.insert_leaf_beside(target, dir, moved_first, split_id, moved.clone())
                    || second.insert_leaf_beside(target, dir, moved_first, split_id, moved)
            }
            _ => false,
        }
    }

    /// The first leaf of kind [`PaneKind::Settings`] under this pane, in
    /// traversal order. The settings tab is found by kind — never by name or
    /// id — so closing the tab or lifting the pane into another space cannot
    /// make a remembered id stale, and a rename cannot hide the tab that
    /// exists. A layout that somehow holds two settings leaves answers with
    /// the first of them, which is what "one settings tab" means for reading;
    /// nothing rewrites the layout to make it so.
    pub fn settings_leaf(&self) -> Option<u64> {
        match self {
            Pane::Leaf {
                id,
                kind: PaneKind::Settings { .. },
            } => Some(*id),
            Pane::Leaf { .. } => None,
            Pane::Split { first, second, .. } => {
                first.settings_leaf().or_else(|| second.settings_leaf())
            }
        }
    }

    /// The kind of the leaf `target`, if this subtree has it.
    pub fn leaf_kind(&self, target: u64) -> Option<&PaneKind> {
        match self {
            Pane::Leaf { id, kind } if *id == target => Some(kind),
            Pane::Split { first, second, .. } => {
                first.leaf_kind(target).or_else(|| second.leaf_kind(target))
            }
            _ => None,
        }
    }

    /// Point the leaf `target` at different content. Returns whether the leaf
    /// was found.
    fn set_leaf_kind(&mut self, target: u64, kind: PaneKind) -> bool {
        match self {
            Pane::Leaf { id, kind: current } if *id == target => {
                *current = kind;
                true
            }
            Pane::Split { first, second, .. } => {
                first.set_leaf_kind(target, kind.clone()) || second.set_leaf_kind(target, kind)
            }
            _ => false,
        }
    }

    /// Replace the leaf `target` with a `Split` whose first child is the old
    /// leaf and whose second child is a fresh leaf of the same kind. Returns the
    /// new leaf id on success.
    fn split_leaf(&mut self, target: u64, dir: SplitDir, split_id: u64, second_id: u64) -> bool {
        match self {
            Pane::Leaf { id, kind } if *id == target => {
                let id = *id;
                let kind = kind.clone();
                *self = Pane::Split {
                    id: split_id,
                    dir,
                    ratio: 0.5,
                    first: Box::new(Pane::leaf(id, kind.clone())),
                    second: Box::new(Pane::leaf(second_id, kind)),
                };
                true
            }
            Pane::Split { first, second, .. } => {
                first.split_leaf(target, dir, split_id, second_id)
                    || second.split_leaf(target, dir, split_id, second_id)
            }
            _ => false,
        }
    }

    /// Like [`Pane::split_leaf`] but the fresh leaf has kind `second_kind`, so
    /// splitting into a terminal pane can use a different kind from the source.
    fn split_leaf_into(
        &mut self,
        target: u64,
        dir: SplitDir,
        split_id: u64,
        second_id: u64,
        second_kind: PaneKind,
    ) -> bool {
        match self {
            Pane::Leaf { id, kind } if *id == target => {
                let id = *id;
                let kind = kind.clone();
                *self = Pane::Split {
                    id: split_id,
                    dir,
                    ratio: 0.5,
                    first: Box::new(Pane::leaf(id, kind)),
                    second: Box::new(Pane::leaf(second_id, second_kind)),
                };
                true
            }
            Pane::Split { first, second, .. } => {
                first.split_leaf_into(target, dir, split_id, second_id, second_kind.clone())
                    || second.split_leaf_into(target, dir, split_id, second_id, second_kind)
            }
            _ => false,
        }
    }

    /// Point every split in this subtree at `dir`. A leaf is unchanged. The
    /// control surface's `layout.set` uses this to orient a space without
    /// touching which panes it holds or their ratios.
    fn set_all_dirs(&mut self, dir: SplitDir) {
        match self {
            Pane::Leaf { .. } => {}
            Pane::Split {
                dir: current,
                first,
                second,
                ..
            } => {
                *current = dir;
                first.set_all_dirs(dir);
                second.set_all_dirs(dir);
            }
        }
    }

    /// Update the ratio of the split node with id `target`.
    fn set_ratio(&mut self, target: u64, ratio: f32) -> bool {
        match self {
            Pane::Split { id, ratio: r, .. } if *id == target => {
                *r = ratio.clamp(0.05, 0.95);
                true
            }
            Pane::Split { first, second, .. } => {
                first.set_ratio(target, ratio) || second.set_ratio(target, ratio)
            }
            _ => false,
        }
    }

    /// The approximate normalized bounding rect (0..1 root space) of every leaf
    /// pane in the subtree, in traversal order. Used for spatial focus
    /// navigation (Ctrl/Cmd+Arrows), where the exact pixel geometry is not
    /// available at the tree level but the split ratios are enough to order
    /// leaves left-to-right / top-to-bottom.
    fn collect_regions(&self, rect: goble_ui::RectF, out: &mut Vec<(u64, goble_ui::RectF)>) {
        match self {
            Pane::Leaf { id, .. } => out.push((*id, rect)),
            Pane::Split {
                dir, ratio, first, second, ..
            } => {
                let (first_rect, second_rect) = match dir {
                    SplitDir::Horizontal => {
                        let split_x = rect.min_x() + rect.width() * *ratio;
                        (
                            goble_ui::rectf(
                                rect.min_x(),
                                rect.min_y(),
                                split_x - rect.min_x(),
                                rect.height(),
                            ),
                            goble_ui::rectf(
                                split_x,
                                rect.min_y(),
                                rect.max_x() - split_x,
                                rect.height(),
                            ),
                        )
                    }
                    SplitDir::Vertical => {
                        let split_y = rect.min_y() + rect.height() * *ratio;
                        (
                            goble_ui::rectf(
                                rect.min_x(),
                                rect.min_y(),
                                rect.width(),
                                split_y - rect.min_y(),
                            ),
                            goble_ui::rectf(
                                rect.min_x(),
                                split_y,
                                rect.width(),
                                rect.max_y() - split_y,
                            ),
                        )
                    }
                };
                first.collect_regions(first_rect, out);
                second.collect_regions(second_rect, out);
            }
        }
    }

    fn leaf_regions(&self) -> Vec<(u64, goble_ui::RectF)> {
        let mut out = Vec::new();
        self.collect_regions(goble_ui::rectf(0.0, 0.0, 1.0, 1.0), &mut out);
        out
    }

    /// Remove the leaf `target`, replacing its parent split with the surviving
    /// sibling. Returns the id of a leaf in the surviving side, or `None` when
    /// `target` is the root (nothing to remove).
    fn remove_leaf(&mut self, target: u64) -> Option<u64> {
        match self {
            Pane::Leaf { .. } => None,
            Pane::Split { first, second, .. } => {
                if first.is_leaf(target) {
                    let survivor = std::mem::replace(
                        second,
                        Box::new(Pane::leaf(0, PaneKind::Chat)),
                    );
                    *self = *survivor;
                    Some(self.first_leaf_id())
                } else if second.is_leaf(target) {
                    let survivor = std::mem::replace(
                        first,
                        Box::new(Pane::leaf(0, PaneKind::Chat)),
                    );
                    *self = *survivor;
                    Some(self.first_leaf_id())
                } else {
                    first.remove_leaf(target).or_else(|| second.remove_leaf(target))
                }
            }
        }
    }
}

/// A "space": a pane tree shown as a tab in the top bar (warp-new style).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Space {
    /// The label the tab draws. For a tab the user renamed by hand this is the
    /// name they typed; otherwise it is the label derived from what the tab
    /// holds — the focused pane's working directory for a terminal, the
    /// conversation's subject for an agent — kept current by
    /// [`crate::state::UiState::refresh_space_labels`].
    pub name: String,
    /// Whether `name` is the user's own — typed in the tab's inline rename —
    /// rather than a label derived from the tab's content. This is the one place
    /// "never named" is told apart from "named by the user": an explicit rename
    /// wins for good (no later path or conversation change takes it back), and a
    /// space this flag is false on keeps deriving its label. `#[serde(default)]`
    /// keeps older persisted layouts parseable: a name saved before this flag
    /// existed was the `Space N` counter, never a name the user chose.
    #[serde(default)]
    pub named: bool,
    pub root: Pane,
    /// The environment medium this space runs on (a [`goble_harness_types::MediumId`]
    /// string). A space created through the topbar "+" menu picks its default
    /// environment here; `#[serde(default)]` keeps older persisted layouts parseable.
    #[serde(default)]
    pub medium: String,
    /// The colour the tab is tinted with, or `None` for the theme's own
    /// surfaces. Chosen from the tab's right-click menu (see
    /// [`crate::ui::TabMenuAction`]) and saved beside the tab it belongs to, so
    /// it comes back with the layout; `#[serde(default)]` reads a layout saved
    /// before this existed as untinted.
    #[serde(default)]
    pub color: Option<TabColor>,
    /// How this space arranges its leaves: the binary tree, a master with a
    /// stack, or free-floating rects. `#[serde(default)]` reads every layout
    /// saved before this existed as the BSP tree it is.
    #[serde(default)]
    pub layout: LayoutKind,
    /// The floating panes, bottom to top. Empty for the other layouts; kept
    /// across a switch into BSP so a space returning to floating keeps the
    /// rects the user dragged.
    #[serde(default)]
    pub floating: Vec<FloatingPane>,
}

impl Space {
    /// A space with a fixed name (the user's own, or a fixture's).
    pub fn new(name: impl Into<String>, root: Pane) -> Self {
        Self {
            name: name.into(),
            named: true,
            root,
            medium: "local".to_string(),
            color: None,
            layout: LayoutKind::Bsp,
            floating: Vec::new(),
        }
    }

    /// A space nobody has named: its tab label is derived from what it holds,
    /// so opening a workspace on a directory names the tab after that directory
    /// and an agent workspace reads [`crate::state::NEW_AGENT_TAB_LABEL`] until
    /// its conversation has a subject.
    pub fn unnamed(root: Pane) -> Self {
        Self {
            name: String::new(),
            named: false,
            root,
            medium: "local".to_string(),
            color: None,
            layout: LayoutKind::Bsp,
            floating: Vec::new(),
        }
    }

    /// Set this space's default environment medium (used when the space is
    /// created through the topbar "+" menu).
    pub fn with_medium(mut self, medium: impl Into<String>) -> Self {
        self.medium = medium.into();
        self
    }

    /// Tint this space's tab with one of the preset colours.
    pub fn with_color(mut self, color: Option<TabColor>) -> Self {
        self.color = color;
        self
    }

    /// Split the leaf `target`, allocating fresh ids from `next_id`. Returns
    /// the id of the newly created pane (which becomes the active pane).
    pub fn split(&mut self, target: u64, dir: SplitDir, next_id: &mut u64) -> Option<u64> {
        let split_id = *next_id;
        *next_id += 1;
        let second_id = *next_id;
        *next_id += 1;
        if self.root.split_leaf(target, dir, split_id, second_id) {
            self.recompute_layout();
            Some(second_id)
        } else {
            None
        }
    }

    /// Split the leaf `target` into a `Split` whose new second leaf has
    /// `second_kind` (used to open a terminal pane off a chat pane).
    pub fn split_with_kind(
        &mut self,
        target: u64,
        dir: SplitDir,
        next_id: &mut u64,
        second_kind: PaneKind,
    ) -> Option<u64> {
        let split_id = *next_id;
        *next_id += 1;
        let second_id = *next_id;
        *next_id += 1;
        if self.root.split_leaf_into(target, dir, split_id, second_id, second_kind) {
            self.recompute_layout();
            Some(second_id)
        } else {
            None
        }
    }

    pub fn set_ratio(&mut self, split_id: u64, ratio: f32) -> bool {
        self.root.set_ratio(split_id, ratio)
    }

    /// Orient every split in this space along `dir`, leaving the panes and
    /// their ratios where they are.
    pub fn set_layout_direction(&mut self, dir: SplitDir) {
        self.root.set_all_dirs(dir);
    }

    /// Select a layout and bring its per-pane state up to date. Switching into
    /// [`LayoutKind::Floating`] gives every leaf a rect; switching into
    /// [`LayoutKind::Bsp`] keeps the tree the space already had, so no pane is
    /// lost by changing how it is arranged.
    ///
    /// Minimizing is a floating-layout gesture: leaving the floating layout
    /// brings every docked window back, so no pane is missing from a layout
    /// that has no dock to find it in.
    pub fn set_layout(&mut self, layout: LayoutKind) {
        self.layout = layout;
        if layout != LayoutKind::Floating {
            for pane in &mut self.floating {
                pane.minimized = false;
            }
        }
        self.recompute_layout();
    }

    /// Recompute the layout after a split, a close or a space switch: floating
    /// panes get a rect if they have none and stale entries are dropped, while
    /// the BSP and master-stack layouts derive their regions from the tree and
    /// need no stored geometry.
    pub fn recompute_layout(&mut self) {
        if self.layout != LayoutKind::Floating {
            let mut ids = Vec::new();
            self.root.leaves(&mut ids);
            self.floating
                .retain(|pane| ids.iter().any(|(id, _)| *id == pane.pane_id));
            return;
        }
        let mut leaves = Vec::new();
        self.root.leaves(&mut leaves);
        self.floating
            .retain(|pane| leaves.iter().any(|(id, _)| *id == pane.pane_id));
        for (index, (id, _)) in leaves.iter().enumerate() {
            if self.floating.iter().any(|pane| pane.pane_id == *id) {
                continue;
            }
            self.floating.push(FloatingPane::new(*id, default_floating_rect(index)));
        }
    }

    /// The rect of the floating pane `pane_id`, if it is floating.
    pub fn floating_rect(&self, pane_id: u64) -> Option<PaneRect> {
        self.floating
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .map(|pane| pane.rect)
    }

    /// Move or resize the floating pane `pane_id`. The rect is clamped into the
    /// space so a drag can never park a pane where it cannot be reached.
    pub fn set_floating_rect(&mut self, pane_id: u64, rect: PaneRect) {
        if let Some(pane) = self.floating.iter_mut().find(|pane| pane.pane_id == pane_id) {
            pane.rect = rect.clamped();
        }
    }

    /// Put the floating pane `pane_id` on top: a click on it raises it.
    pub fn raise_pane(&mut self, pane_id: u64) {
        if let Some(index) = self.floating.iter().position(|pane| pane.pane_id == pane_id) {
            let pane = self.floating.remove(index);
            self.floating.push(pane);
        }
    }

    /// Hide the floating pane `pane_id` into the dock, keeping its rect and its
    /// place in the z-order so restoring it puts it back where it was.
    pub fn minimize_pane(&mut self, pane_id: u64) {
        if let Some(pane) = self
            .floating
            .iter_mut()
            .find(|pane| pane.pane_id == pane_id)
        {
            pane.minimized = true;
        }
    }

    /// Bring the minimized floating pane `pane_id` back onto the pane area and
    /// raise it, so the window the dock was clicked for is the one in front.
    pub fn restore_pane(&mut self, pane_id: u64) {
        if let Some(pane) = self
            .floating
            .iter_mut()
            .find(|pane| pane.pane_id == pane_id)
        {
            pane.minimized = false;
        }
        self.raise_pane(pane_id);
    }

    /// Whether the floating pane `pane_id` is currently docked.
    pub fn pane_minimized(&self, pane_id: u64) -> bool {
        self.floating
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .is_some_and(|pane| pane.minimized)
    }

    /// The docks of this space, in z-order: the windows a minimized chip stands
    /// for. Empty outside the floating layout, where no window is docked.
    pub fn minimized_panes(&self) -> Vec<(u64, PaneRect)> {
        if self.layout != LayoutKind::Floating {
            return Vec::new();
        }
        self.floating
            .iter()
            .filter(|pane| pane.minimized && self.root.contains_leaf(pane.pane_id))
            .map(|pane| (pane.pane_id, pane.rect))
            .collect()
    }

    /// The leaves that are on screen in the space's current layout: every leaf
    /// except the windows docked in the floating layout.
    pub fn visible_leaf_ids(&self) -> Vec<u64> {
        let mut leaves = Vec::new();
        self.root.leaves(&mut leaves);
        leaves
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| !self.pane_minimized(*id))
            .collect()
    }

    /// The normalized region every leaf occupies under the space's current
    /// layout, in draw order (floating panes are bottom to top). Spatial focus
    /// navigation reads this, and it is the one place the layouts' geometry is
    /// defined.
    pub fn layout_regions(&self) -> Vec<(u64, goble_ui::RectF)> {
        match self.layout {
            LayoutKind::Bsp => self.root.leaf_regions(),
            LayoutKind::MasterStack => {
                let mut leaves = Vec::new();
                self.root.leaves(&mut leaves);
                master_stack_regions(&leaves)
            }
            LayoutKind::Floating => {
                // The stored order is the z-order, so the returned order is
                // what the layer draws: bottom first. A docked window is not
                // on the pane area, so focus navigation must skip it.
                self.floating
                    .iter()
                    .filter(|pane| !pane.minimized && self.root.contains_leaf(pane.pane_id))
                    .map(|pane| (pane.pane_id, pane.rect.to_rect()))
                    .collect()
            }
            LayoutKind::Carousel => {
                let mut leaves = Vec::new();
                self.root.leaves(&mut leaves);
                let focused = leaves.first().map(|(id, _)| *id).unwrap_or_default();
                carousel_regions(&leaves, focused)
            }
        }
    }

    /// The kind of the leaf `target` (see [`Pane::leaf_kind`]).
    pub fn leaf_kind(&self, target: u64) -> Option<&PaneKind> {
        self.root.leaf_kind(target)
    }

    /// The first settings leaf in this space's tree (see [`Pane::settings_leaf`]).
    pub fn settings_leaf(&self) -> Option<u64> {
        self.root.settings_leaf()
    }

    /// Point the leaf `target` at different content (see [`Pane::set_leaf_kind`]).
    pub fn set_leaf_kind(&mut self, target: u64, kind: PaneKind) -> bool {
        self.root.set_leaf_kind(target, kind)
    }

    /// Close the leaf `target`, returning the id of the pane to focus next.
    pub fn close(&mut self, target: u64) -> Option<u64> {
        let next = self.root.remove_leaf(target);
        if next.is_some() {
            self.floating.retain(|pane| pane.pane_id != target);
            self.recompute_layout();
        }
        next
    }

    /// Move the leaf `pane_id` beside the leaf `target_id`, on `side`: the pane
    /// leaves its place in the tree and arrives as a new split of the target,
    /// carrying its own id and kind, so its session follows it. The split it
    /// left collapses onto its surviving sibling the way closing it would.
    ///
    /// Returns whether the tree changed. A move onto the side its own sibling
    /// already occupies is a no-op — rebuilding that split would only reset its
    /// ratio — as is a move onto itself or to a pane this space does not hold.
    pub fn move_pane(
        &mut self,
        pane_id: u64,
        target_id: u64,
        side: DropSide,
        next_id: &mut u64,
    ) -> bool {
        if pane_id == target_id {
            return false;
        }
        let Some(kind) = self.root.leaf_kind(pane_id).cloned() else {
            return false;
        };
        if self.root.leaf_kind(target_id).is_none() {
            return false;
        }
        if self.root.sibling_on_side(target_id, side) == Some(pane_id) {
            return false;
        }
        if self.root.remove_leaf(pane_id).is_none() {
            return false;
        }
        let split_id = *next_id;
        *next_id += 1;
        let inserted = self.root.insert_leaf_beside(
            target_id,
            side.split_dir(),
            side.leads(),
            split_id,
            Pane::leaf(pane_id, kind),
        );
        if inserted {
            self.recompute_layout();
        }
        inserted
    }

    /// Find the leaf pane spatially adjacent to `from` in the given `dir`,
    /// using approximate normalized leaf regions. Returns `None` when there is
    /// no leaf in that direction (e.g. `from` is on the window edge). Ties are
    /// broken by distance along the perpendicular axis so the nearest
    /// same-row/same-column pane wins.
    pub fn navigate(&self, from: u64, dir: NavDir) -> Option<u64> {
        // A carousel is one ordered row, not a grid: Left/Up walks toward the
        // front of the row and Right/Down toward the back, wrapping at either
        // end so the row can be cycled without stopping at an edge.
        if self.layout == LayoutKind::Carousel {
            let mut leaves = Vec::new();
            self.root.leaves(&mut leaves);
            if leaves.len() < 2 {
                return None;
            }
            let index = leaves.iter().position(|(id, _)| *id == from)?;
            let step = match dir {
                NavDir::Left | NavDir::Up => leaves.len() - 1,
                NavDir::Right | NavDir::Down => 1,
            };
            return Some(leaves[(index + step) % leaves.len()].0);
        }
        let regions = self.layout_regions();
        let current = regions.iter().find_map(|(id, r)| (*id == from).then_some(*r))?;

        let center_x = |r: &goble_ui::RectF| r.min_x() + r.width() * 0.5;
        let center_y = |r: &goble_ui::RectF| r.min_y() + r.height() * 0.5;

        let mut best: Option<(u64, f32, f32)> = None;
        for (id, r) in &regions {
            if *id == from {
                continue;
            }
            let (primary, secondary) = match dir {
                NavDir::Up => {
                    if r.max_y() <= current.min_y() {
                        (
                            current.min_y() - r.max_y(),
                            (center_x(r) - center_x(&current)).abs(),
                        )
                    } else {
                        continue;
                    }
                }
                NavDir::Down => {
                    if r.min_y() >= current.max_y() {
                        (
                            r.min_y() - current.max_y(),
                            (center_x(r) - center_x(&current)).abs(),
                        )
                    } else {
                        continue;
                    }
                }
                NavDir::Left => {
                    if r.max_x() <= current.min_x() {
                        (
                            current.min_x() - r.max_x(),
                            (center_y(r) - center_y(&current)).abs(),
                        )
                    } else {
                        continue;
                    }
                }
                NavDir::Right => {
                    if r.min_x() >= current.max_x() {
                        (
                            r.min_x() - current.max_x(),
                            (center_y(r) - center_y(&current)).abs(),
                        )
                    } else {
                        continue;
                    }
                }
            };
            let better = match best {
                None => true,
                Some((_, bp, bs)) => {
                    primary < bp - 1e-6
                        || ((primary - bp).abs() < 1e-6 && secondary < bs - 1e-6)
                }
            };
            if better {
                best = Some((*id, primary, secondary));
            }
        }
        best.map(|(id, _, _)| id)
    }
}

/// The master-stack regions of `leaves`: the first pane takes [`MASTER_RATIO`]
/// of the width, and every remaining pane gets an equal share of a column on
/// the right, top to bottom.
pub fn master_stack_regions(leaves: &[(u64, PaneKind)]) -> Vec<(u64, goble_ui::RectF)> {
    const MASTER_RATIO: f32 = 0.6;
    let mut out = Vec::new();
    let Some(((master, _), rest)) = leaves.split_first() else {
        return out;
    };
    out.push((*master, rectf(0.0, 0.0, MASTER_RATIO, 1.0)));
    if rest.is_empty() {
        return out;
    }
    let height = 1.0 / rest.len() as f32;
    for (index, (id, _)) in rest.iter().enumerate() {
        out.push((
            *id,
            rectf(MASTER_RATIO, index as f32 * height, 1.0 - MASTER_RATIO, height),
        ));
    }
    out
}

/// The size of every carousel window, as a share of the pane area. The size is
/// constant across the row: stepping through the carousel pans it sideways
/// without changing a window's width or height.
pub const CAROUSEL_WINDOW_WIDTH: f32 = 0.58;
pub const CAROUSEL_WINDOW_HEIGHT: f32 = 0.84;
/// How far each carousel step moves a window sideways. The step is narrower
/// than the window, so the row overlaps and the centered window reads as the
/// one in front while each neighbour keeps a visible, clickable edge.
const CAROUSEL_STEP: f32 = 0.16;

/// The carousel regions of `leaves`, centered on `focused`: the focused window
/// in the middle, every other window the same size at the same y, panned left
/// or right by its distance from the focused one. The returned order is the
/// draw order — farthest windows first, the focused window last and therefore on
/// top.
///
/// Positions are clamped into the unit square, so a long row stacks its outer
/// windows at the edges instead of parking them where they cannot be clicked.
pub fn carousel_regions(
    leaves: &[(u64, PaneKind)],
    focused: u64,
) -> Vec<(u64, goble_ui::RectF)> {
    if leaves.is_empty() {
        return Vec::new();
    }
    let focus_index = leaves
        .iter()
        .position(|(id, _)| *id == focused)
        .unwrap_or(0);
    let w = CAROUSEL_WINDOW_WIDTH.min(1.0);
    let h = CAROUSEL_WINDOW_HEIGHT.min(1.0);
    let y = (1.0 - h) / 2.0;
    let mut placed: Vec<(u64, goble_ui::RectF, f32)> = leaves
        .iter()
        .enumerate()
        .map(|(index, (id, _))| {
            let offset = index as f32 - focus_index as f32;
            let distance = offset.abs();
            let x = (0.5 - w / 2.0 + offset * CAROUSEL_STEP).clamp(0.0, (1.0 - w).max(0.0));
            (*id, rectf(x, y, w, h), distance)
        })
        .collect();
    // Stable sort: two neighbours at the same distance keep their row order,
    // and the focused window (distance zero) lands last.
    placed.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    placed.into_iter().map(|(id, rect, _)| (id, rect)).collect()
}

/// The rect a floating pane gets before the user moves it: a cascade so two
/// panes opened in a row do not land exactly on top of each other.
pub fn default_floating_rect(index: usize) -> PaneRect {
    let step = 0.04 * (index % 8) as f32;
    PaneRect::new(0.08 + step, 0.08 + step, 0.62, 0.55)
}
