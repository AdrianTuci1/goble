/// Which kind of content a pane hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PaneKind {
    Chat,
    Terminal,
}

/// Which way a pane tree splits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SplitDir {
    /// First pane on the left / top; a horizontal split ("in 2").
    Horizontal,
    /// First pane on top / bottom; a vertical split ("down").
    Vertical,
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

    fn is_leaf(&self, target: u64) -> bool {
        matches!(self, Pane::Leaf { id, .. } if *id == target)
    }

    /// Replace the leaf `target` with a `Split` whose first child is the old
    /// leaf and whose second child is a fresh leaf of the same kind. Returns the
    /// new leaf id on success.
    fn split_leaf(&mut self, target: u64, dir: SplitDir, split_id: u64, second_id: u64) -> bool {
        match self {
            Pane::Leaf { id, kind } if *id == target => {
                let id = *id;
                let kind = *kind;
                *self = Pane::Split {
                    id: split_id,
                    dir,
                    ratio: 0.5,
                    first: Box::new(Pane::leaf(id, kind)),
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
                let kind = *kind;
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
                first.split_leaf_into(target, dir, split_id, second_id, second_kind)
                    || second.split_leaf_into(target, dir, split_id, second_id, second_kind)
            }
            _ => false,
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

/// A "space": a named pane tree shown as a tab in the top bar (warp-new style).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Space {
    pub name: String,
    pub root: Pane,
    /// The environment medium this space runs on (a [`goble_harness_types::MediumId`]
    /// string). A space created through the topbar "+" menu picks its default
    /// environment here; `#[serde(default)]` keeps older persisted layouts parseable.
    #[serde(default)]
    pub medium: String,
}

impl Space {
    pub fn new(name: impl Into<String>, root: Pane) -> Self {
        Self {
            name: name.into(),
            root,
            medium: "local".to_string(),
        }
    }

    /// Set this space's default environment medium (used when the space is
    /// created through the topbar "+" menu).
    pub fn with_medium(mut self, medium: impl Into<String>) -> Self {
        self.medium = medium.into();
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
            Some(second_id)
        } else {
            None
        }
    }

    pub fn set_ratio(&mut self, split_id: u64, ratio: f32) -> bool {
        self.root.set_ratio(split_id, ratio)
    }

    /// Close the leaf `target`, returning the id of the pane to focus next.
    pub fn close(&mut self, target: u64) -> Option<u64> {
        self.root.remove_leaf(target)
    }

    /// Find the leaf pane spatially adjacent to `from` in the given `dir`,
    /// using approximate normalized leaf regions. Returns `None` when there is
    /// no leaf in that direction (e.g. `from` is on the window edge). Ties are
    /// broken by distance along the perpendicular axis so the nearest
    /// same-row/same-column pane wins.
    pub fn navigate(&self, from: u64, dir: NavDir) -> Option<u64> {
        let regions = self.root.leaf_regions();
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
