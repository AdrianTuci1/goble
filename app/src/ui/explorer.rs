//! The sidebar's project explorer: the active pane's working directory as a real
//! file tree.
//!
//! Reading is warp-new's: one directory level at a time, read when the directory
//! is opened and remembered until it is closed again. Nothing is read per frame,
//! so a tree left open costs no disk at all.

use std::collections::{HashMap, HashSet};

use goble_ui::elements::{
    file_icon_name, AppContext, Axis, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets,
    Element, Empty, Expanded, Flex, HoverRow, Icon, MainAxisSize, Scrollable, Text, Tooltip,
    TooltipPosition,
};
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::vec2f;

use super::{ExplorerRow, UiActions, UiSnapshot};

/// How far one level of the tree indents its rows, in pixels. The same 16pt per
/// level warp-new indents by, and the width of each of the row's two leading
/// icon slots, so the two columns line up at every depth.
const INDENT: f32 = 16.0;
/// The file-name size. warp-new draws its tree items at 14pt, larger than the
/// sidebar's own list rows: the tree is the one place a sidebar row is a file.
const ITEM_FONT_SIZE: f32 = 14.0;
/// The space between a row's columns: the chevron to the icon, the icon to the
/// name (warp-new's own 4 and 8).
const CHEVRON_GAP: f32 = 4.0;
const ICON_GAP: f32 = 8.0;
/// The chevron glyph itself, inside the 16pt slot it shares with the icon of
/// the column beside it.
const CHEVRON_SIZE: f32 = 12.0;
/// The row's own vertical padding, above and below the 16pt icon slot.
const ITEM_PADDING: f32 = 4.0;
/// The highlight's corner radius, the reference tree's own 4pt.
const ROW_RADIUS: f32 = 4.0;

/// One entry of a directory listing.
#[derive(Clone, Debug)]
struct Entry {
    name: String,
    path: String,
    is_dir: bool,
}

/// The children of `dir`: directories first, then files, each alphabetical, with
/// hidden entries left out.
fn children(dir: &str) -> Vec<Entry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        out.push(Entry {
            name,
            is_dir: path.is_dir(),
            path: path.to_string_lossy().to_string(),
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// The entries of every directory the open tree has read, so a frame that opens
/// nothing reads nothing.
#[derive(Default)]
pub struct ExplorerCache {
    /// The root the listings describe. A different root drops them all.
    root: String,
    listings: HashMap<String, Vec<Entry>>,
}

impl ExplorerCache {
    /// The rows of `root` with the directories in `expanded` open. Only an open
    /// explorer reads the disk, and only for directories it has not read since
    /// they were opened; closing the explorer drops everything, so it is fresh
    /// the next time it is opened. A directory closed and opened again is read
    /// again, which is how a file created in the meantime appears.
    pub fn rows(
        &mut self,
        root: &str,
        expanded: &HashSet<String>,
        open: bool,
    ) -> Vec<ExplorerRow> {
        if !open {
            self.root.clear();
            self.listings.clear();
            return Vec::new();
        }
        if self.root != root {
            self.root = root.to_string();
            self.listings.clear();
        }
        let mut rows = Vec::new();
        self.flatten(root, 0, expanded, &mut rows);
        self.listings
            .retain(|path, _| path == root || expanded.contains(path));
        rows
    }

    fn flatten(
        &mut self,
        dir: &str,
        depth: usize,
        expanded: &HashSet<String>,
        rows: &mut Vec<ExplorerRow>,
    ) {
        if !self.listings.contains_key(dir) {
            self.listings.insert(dir.to_string(), children(dir));
        }
        let entries = match self.listings.get(dir) {
            Some(entries) => entries.clone(),
            None => Vec::new(),
        };
        for entry in entries {
            let is_open = entry.is_dir && expanded.contains(&entry.path);
            rows.push(ExplorerRow {
                path: entry.path.clone(),
                name: entry.name,
                depth,
                is_dir: entry.is_dir,
                expanded: is_open,
            });
            if is_open {
                self.flatten(&entry.path, depth + 1, expanded, rows);
            }
        }
    }
}

/// The explorer: which directory the tree shows, then the tree itself.
///
/// A directory row opens and closes it. A file row opens that file in a pane of
/// its own beside the active one (the reference tool's editor pane), leaving
/// whatever the user was typing in the input alone.
pub(crate) fn build_explorer(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_toggle_dir = actions.on_toggle_explorer_dir.clone();
    let on_open_file = actions.on_explorer_file_click.clone();

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        // Rows sit flush: the reference tree's list has no spacing between its
        // items, the rows' own padding is what separates them.
        .with_spacing(0.0);
    if state.explorer_root.is_empty() {
        list = list.with_child(hint(app, "No working directory yet."));
    } else if state.explorer_rows.is_empty() {
        list = list.with_child(hint(app, "This directory has nothing to show."));
    }
    for row in &state.explorer_rows {
        let path = row.path.clone();
        // The reference tree gives a row one colour: its chevron, its icon and
        // its name are all the 60% font colour at rest and the main 90% one
        // while the pointer is over the row. Hover itself is known only while
        // the previous frame paints, so this is the row the pointer reached one
        // frame ago — the move that reached it already asked for that frame.
        let hovered = state.explorer_hover.borrow().as_deref() == Some(row.path.as_str());
        let row_color = if hovered {
            ColorToken::Text
        } else {
            ColorToken::Muted
        };
        let mut line = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(0.0);
        // The reference tree's row recipe: a spacer for the depth, the chevron's
        // 16pt slot, the gap after it, the icon's 16pt slot, the gap after that,
        // then the name. Every column is a fixed box so the icon and the name
        // line up at every depth, whether the row is a file or a directory.
        if row.depth > 0 {
            line = line.with_child(fixed(row.depth as f32 * INDENT));
        }
        // The chevron is the row's own control: a directory shows which way it
        // opens, a file keeps the slot (empty) so every name lines up in one
        // column at every depth.
        line = line.with_child(if row.is_dir {
            // The glyph inset to the middle of the slot, as the reference tree
            // draws it inside its own 16x16 box.
            Container::new(
                Icon::new(if row.expanded {
                    "chevron-down"
                } else {
                    "chevron-right"
                })
                .with_size(CHEVRON_SIZE)
                .with_theme_color(row_color, app)
                .finish(),
            )
            .with_padding(EdgeInsets::uniform((INDENT - CHEVRON_SIZE) / 2.0))
            .finish()
        } else {
            fixed(INDENT)
        });
        line = line.with_child(fixed(CHEVRON_GAP));
        line = line.with_child(
            ConstrainedBox::new(
                Icon::new(if row.is_dir {
                    // One folder glyph for both states, as the reference tool's
                    // tree draws it: the chevron beside it is what says whether
                    // the directory is open, and a second folder shape in
                    // another drawing style would make the tree read as two
                    // icon sets.
                    "folder"
                } else {
                    file_icon_name(&row.name)
                })
                .with_size(16.0)
                .with_theme_color(row_color, app)
                .finish(),
            )
            .with_width(INDENT)
            .with_height(INDENT)
            .finish(),
        );
        line = line.with_child(fixed(ICON_GAP));
        line = line.with_child(
            // A directory's name is drawn like a file's: the reference tool
            // gives folders no colour of their own, so the row's three parts
            // share one colour and the icons carry the folder/file difference.
            Text::new(row.name.clone())
                .with_theme_color(row_color, app)
                .with_font_size(ITEM_FONT_SIZE)
                .with_max_lines(1)
                .finish(),
        );

        let hint = row.path.clone();
        let on_toggle_dir = on_toggle_dir.clone();
        let on_open_file = on_open_file.clone();
        let is_dir = row.is_dir;
        let row = HoverRow::new(line.finish())
            .with_padding(EdgeInsets::new(8.0, ITEM_PADDING, 8.0, ITEM_PADDING))
            .with_corner_radius(ROW_RADIUS)
            .with_hover_key(state.explorer_hover.clone(), path.clone())
            .with_on_click(move || {
                if is_dir {
                    (on_toggle_dir.borrow_mut())(path.clone());
                } else {
                    (on_open_file.borrow_mut())(path.clone());
                }
            })
            .finish();
        list = list.with_child(
            Tooltip::new(row, hint)
                .with_position(TooltipPosition::Below)
                .finish(),
        );
    }

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    // Which directory is on screen, above the tree: the tree is the active
    // pane's, and the pane can move under it.
    if !state.explorer_root.is_empty() {
        column = column.with_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.0)
                .with_child(
                    Icon::new("folder")
                        .with_size(12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new(crate::state::display_path(&state.explorer_root))
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(11.0)
                        .with_max_lines(1)
                        .finish(),
                )
                .finish(),
        );
    }
    column = column.with_child(
        Expanded::new(
            Scrollable::new(list.finish(), Axis::Vertical)
                .with_state(state.explorer_scroll.clone())
                .finish(),
        )
        .finish(),
    );
    column.finish()
}

/// An invisible box `width` wide: a row's column that draws nothing (the
/// indent, a gap, the chevron slot of a file). Sized on the `Empty` itself
/// because a `ConstrainedBox` around one measures the empty box — 0 wide — and
/// would collapse the column it was meant to hold open.
fn fixed(width: f32) -> Box<dyn Element> {
    Empty::new().with_size(vec2f(width, 0.0)).finish()
}

/// A muted line that says why the tree has nothing to show.
fn hint(app: &AppContext, text: &str) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    Container::new(
        Text::new(text)
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(xs))
    .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_explorer_reads_nothing_and_a_root_change_drops_the_listings() {
        let home = tempfile::tempdir().expect("temp home");
        std::fs::create_dir(home.path().join("src")).unwrap();
        std::fs::write(home.path().join("Cargo.toml"), "[package]").unwrap();
        let root = home.path().to_string_lossy().to_string();

        let mut cache = ExplorerCache::default();
        assert!(cache.rows(&root, &HashSet::new(), false).is_empty());

        let rows = cache.rows(&root, &HashSet::new(), true);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, vec!["src", "Cargo.toml"], "directories first");
        assert!(rows.iter().all(|row| row.depth == 0));

        // Opening `src` draws its children one level in; an empty directory adds
        // no rows, and the rows say it is open.
        let mut expanded = HashSet::new();
        expanded.insert(rows[0].path.clone());
        let again = cache.rows(&root, &expanded, true);
        assert_eq!(again.len(), rows.len());
        assert!(again[0].expanded, "the open directory says so");

        // A different root forgets the previous tree entirely.
        let other = tempfile::tempdir().expect("temp home");
        std::fs::write(other.path().join("notes.md"), "hi").unwrap();
        let rows = cache.rows(&other.path().to_string_lossy(), &HashSet::new(), true);
        assert_eq!(rows.len(), 1, "only the new root's entries: {rows:?}");
        assert_eq!(rows[0].name, "notes.md");
        assert!(!rows[0].is_dir);
    }

    #[test]
    fn a_hidden_entry_is_not_a_row_and_an_unreadable_directory_shows_nothing() {
        let home = tempfile::tempdir().expect("temp home");
        std::fs::create_dir(home.path().join(".git")).unwrap();
        std::fs::write(home.path().join(".gitignore"), "target").unwrap();

        let mut cache = ExplorerCache::default();
        let rows = cache.rows(&home.path().to_string_lossy(), &HashSet::new(), true);
        assert!(rows.is_empty(), "hidden entries are left out: {rows:?}");

        assert!(
            cache
                .rows("/definitely/not/a/directory", &HashSet::new(), true)
                .is_empty(),
            "a directory that cannot be read shows nothing rather than a failure"
        );
    }

    /// Opening a subdirectory draws its children one level in, and closing it
    /// takes them away again.
    #[test]
    fn expanding_a_directory_draws_its_children_under_it() {
        let home = tempfile::tempdir().expect("temp home");
        std::fs::create_dir_all(home.path().join("src").join("ui")).unwrap();
        std::fs::write(home.path().join("src").join("main.rs"), "fn main() {}").unwrap();
        let root = home.path().to_string_lossy().to_string();

        let mut cache = ExplorerCache::default();
        let mut expanded = HashSet::new();
        expanded.insert(home.path().join("src").to_string_lossy().to_string());
        let rows = cache.rows(&root, &expanded, true);
        let drawn: Vec<(String, usize)> = rows
            .iter()
            .map(|row| (row.name.clone(), row.depth))
            .collect();
        assert_eq!(
            drawn,
            vec![
                ("src".to_string(), 0),
                ("ui".to_string(), 1),
                ("main.rs".to_string(), 1),
            ]
        );

        expanded.clear();
        let rows = cache.rows(&root, &expanded, true);
        assert_eq!(rows.len(), 1, "the closed directory hides its children");
    }
}
