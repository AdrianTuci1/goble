# 04 — Conversation Sidebar (left)

## Three views behind one toolbelt (R7, current)

The sidebar is three views, and a toolbelt at its top selects which one is
showing. The tabs are icon-only, 24×24 (a 14 px icon in a 5 px padding), and the
active one carries the `Selected` band:

| Tab | Icon | View |
| --- | --- | --- |
| `Agents` (default) | `message-chat-square` | the conversation search, the `Starred` section, the folder sections |
| `Explorer` | `folder-closed` | the active pane's working directory as a real file tree |
| `Search` | `search` | the query field and what it matches in the files under that directory |

The view is app state (`UiSnapshot::sidebar_view`), and entering a view rewinds
that view's own list (`on_select_sidebar_view` resets the `ScrollState` of the
list it is about to show), so a view always opens at the top while the other
views keep the offset they had.

## The surface is flat (R6)

The sidebar draws no rounded corner anywhere, carries no separator rule, and its
search field has no magnifier. Nothing here is a rounded widget: the column is a
flat band of rows inside the app's `Surface`, delimited by spacing alone.

| Surface | Element | Shape now |
| --- | --- | --- |
| Toolbelt tab | `HoverRow` (paint-time hover + `Selected`) | square band |
| Search field | `SearchInput::with_compact(true).with_icon(false).with_extra_height(3.0)` | square, no `search` icon, `3 px` taller above and below its text (`xs` = 4, so `7 px` of vertical padding at the default density) |
| "New conversation" row | `HoverButton::with_corner_radius(0.0)` | square hover highlight |
| `+` box | `Container` (`SurfaceRaised`, no radius) | square |
| Section header (`Starred`, each folder) | `HoverRow` | square band |
| Conversation card | `ConversationListItem` root `Container` | square band, full sidebar width |
| Card's environment badge | `Container` (`SurfaceRaised`, no radius) | square |
| Card's star mark | `Icon` (`star-filled`) after the subject | square, no background |
| Card's 3-dot control | `TopbarButton::with_corner_radius(0.0)` | square hover fill |
| Card's star / delete rows | `Button::with_corner_radius(0.0)` | square |
| Explorer row | `HoverRow` | square band |
| Search result row | `HoverRow` | square band |
| "View all (N)" / "Show less" | `Button::with_corner_radius(0.0)` | square fill |

`SearchInput`'s `with_icon(false)` and `with_extra_height(px)`, and the
`with_corner_radius(radius)` knob on `HoverButton`, `Button` and `TopbarButton`,
are the opt-ins that make this possible without changing those elements for the
rest of the app: their default is unchanged (the icon is drawn, the height is
the text's, the radius is the theme's or half of it).

`HoverRow` (`crates/goble-ui/src/elements/hover_row.rs`) is what makes the
toolbelt, the section headers, the tree and the results work: it reads the
frame's cursor while *painting*, so any number of rows highlight without a
host-owned hover flag however the tree is rebuilt between frames.

Locked by tests — `elements::search_input::tests::
the_sidebar_search_is_a_taller_square_field_without_a_magnifier` (and its
counterpart `the_search_field_keeps_its_magnifier_by_default`, so the flag is a
real switch), and `ui::sidebar::sidebar_surface_tests::
the_sidebar_is_flat_square_and_free_of_separators_and_the_magnifier`, which
mounts the `RootView` over a real store in four hover states and asserts that
the band left of the workspace draws no rounded fill, no hairline spanning the
sidebar (a rule), and no `search` icon beyond the toolbelt's global-search tab,
while still drawing the field's own border and the runs that make those
assertions mean something. The rule that tells a separator from a control's edge
is itself tested (`the_separator_rule_reports_a_rule_and_not_a_field_border`).

## Structure (Agents)

```
+--------------------+
| [] [] []           |  toolbelt: agents, explorer, search
| [Search]           |
| New conversation   |
| v Starred       1  |
|   Ada           1h |
| v General       2  |
|   Coder         1h |
|   Ops           2h |
+--------------------+
```

## Header

- Toolbelt: the three tabs above.
- Square search field, placeholder `Search`, no magnifier; a few pixels taller
  than its text so it reads as a field.
- The "New conversation" row below it: a `+` box and the label.

## Sections

Every section is a header that toggles it. The collapsed keys live in app state
(`UiState::collapsed_sections`): `"starred"` for the pinned section,
`format!("folder:{name}")` for a conversation folder. A collapsed section keeps
its header — which says how many rows are hidden — and drops its cards.

- `Starred` sits first and lists the starred conversations of *every*
  environment, whichever folder they belong to. A section with nothing in it is
  not drawn at all.
- Then one section per conversation folder, in insertion order, filtered by the
  selected medium's routing (Local / Remote) the way it always was. `General` is
  the default folder of a conversation that was never filed one.

Locked by tests: `a_collapsed_section_keeps_its_header_and_drops_its_cards`,
`a_starred_conversation_is_drawn_in_the_starred_section_above_the_folders`,
`a_section_key_toggles_only_that_section`.

## Conversation card

Element: `ConversationListItem` (`crates/goble-ui/src/elements/conversation_list_item.rs`).

- Left: `Avatar` or `Icon` (agent icon).
- Center column:
  - Name in `Text` (`ColorToken::Text`, medium weight), with a `star-filled`
    mark right after it when the conversation is starred.
  - The working directory the conversation runs in, under the name.
  - Last response snippet in `Text` (`ColorToken::Muted`, truncated with ellipsis).
- Top-right: timestamp in `Text` (`ColorToken::Muted`, smaller size).
- Unread dot (`Accent`) on the right when unread.
- Square `Surface` background by default (hover: `Hover`, selected: `Selected`) —
  a full-width band, not a rounded card.
- Hover: reveal a `dots-horizontal` icon on the far right.

## Card menu (star + delete)

- On hover of the card, show the `dots-horizontal` icon.
- Clicking the dots opens a row under the card: `Star` / `Unstar` (a `star` /
  `star-filled` icon and its label) and `Delete agent` (red `Error` text).
- `Star` toggles the conversation in `UiState::starred_conversations` and closes
  the menu — the card's mark and the `Starred` section are where the choice shows.
- `Delete agent` fires the parent's delete callback.

Locked by tests: `ui::sidebar::sidebar_action_tests::starring_a_conversation_toggles_it`,
`elements::conversation_list_item::tests::
the_menus_star_row_stars_the_conversation_and_closes_the_menu`.

## Project explorer (Explorer tab)

`app/src/ui/explorer.rs`. The tree is the active pane's working directory, read
one level at a time:

- `ExplorerCache` holds the listings of the directories that are open, so a frame
  that opens nothing reads nothing. Closing the explorer drops everything; a
  directory closed and opened again is read again, which is how a file created in
  the meantime appears.
- `children(dir)` reads `read_dir`, skips hidden entries and orders directories
  first, then files, each case-insensitively alphabetical.
- A row is indented 16 px per level, then a 12 px chevron (`chevron-down` when
  open, `chevron-right` closed) in a 16 px slot — a file holds the same slot
  empty, so every name lines up in one column at every depth — then a 16 px
  `folder`/`folder-closed`/file-type icon (`file_icon_name`, the extension's
  language or a plain document), then the name at 14 px over a `HoverRow` with a
  tooltip carrying the path.
- Clicking a directory expands or collapses it (`UiState::explorer_expanded`).
  It does **not** move the pane's working directory.
- Clicking a file opens it in a **file-view pane** beside the active one, in the
  tab on screen (`app/src/ui/file_view.rs`, `PaneKind::File { path }`). A pane
  already showing a file takes the newly clicked file over rather than splitting
  again, so browsing a directory with the file view focused walks through files
  in one pane. A file view keeps no conversation and no working directory of its
  own, so the tree (and the search root) keep following the last pane that had
  one.
- Expanded by `expanded_folders`-style state and a `Scrollable` whose offset
  lives in `UiState::explorer_scroll`.

## File view (a `File` pane leaf)

`app/src/ui/file_view.rs`. The text of one file, opened from the explorer tree
or a search result:

- `FileCache` (owned by the root, beside `ExplorerCache`) holds the file each
  open view read; a frame re-reads a file only when its length or modification
  time moved, so a file being written under the view updates it without a read
  per frame. A view that closes drops its file from the cache.
- `read` refuses what it should not draw: past `MAX_FILE_BYTES` (4 MB) the pane
  reports the size, a NUL in the first 8 KB or a UTF-8 failure reads as "not
  text", and anything that is not a readable file reads as "cannot be read".
- At most `MAX_LINES` (2000) lines are drawn and the title says how many the
  file has in total, so the view never grows without a bound.
- The pane wears the same topbar as its siblings (close button, app tray) and
  adds a title row: the file-type icon, the file's name (the full path on its
  hover) and the line count. The lines are monospace under a padded line-number
  gutter, scrolled by the pane's own offset (`UiState::file_scroll`, a plain
  offset — a file opens at its first line, not at its end).

## Global search (Search tab)

`app/src/ui/global_search.rs`. A real recursive content search, run when the
query changes and never per frame, over the active pane's working directory
(`UiState::active_working_directory`, so a file view never moves the search root
off the pane the user was working in):

- ripgrep is preferred, never required: `ripgrep_binary()` searches `PATH` first
  and then the directories Homebrew, Cargo and the system use, because a
  Finder-launched app inherits launchd's `PATH`, not the shell's. `rg --null
  --line-number --no-heading --color never --smart-case --max-count 10
  --max-columns 400 -e <escaped> -- <root>`, and the query is escaped to a
  literal string, so a search for `fn main(` is a search for that text.
- With no `rg` on the machine the search runs in-process over the same
  directory: a `walkdir` walk that never descends into `SKIPPED_DIRS` (.git,
  target, node_modules, build outputs), reads no file past `MAX_FILE_BYTES`, and
  matches each line as a plain substring (case-insensitively unless the query
  has an uppercase letter, ripgrep's own smart-case rule), with the same
  per-file and total caps.
- Exit code 1 ("searched, nothing matched") is an answer, not a failure; the one
  error the view reports instead of results is a root it cannot read.
- The rows are grouped: a file's header row (its icon and name), then one row per
  matched line (line number and text). Clicking a row opens that file in a
  file-view pane, like the explorer.
- The summary line reads "Searching…" / "No results found." / "N results in M
  files", or the error.

## Files

- `crates/goble-ui/src/elements/conversation_list_item.rs` — the card and its menu
- `crates/goble-ui/src/elements/hover_row.rs` — the row the toolbelt, the section
  headers, the tree and the results are built from
- `app/src/ui/sidebar.rs` — the toolbelt, the agents view, the section headers
- `app/src/ui/explorer.rs` — the tree and its per-directory cache
- `app/src/ui/file_view.rs` — the pane that shows one file, and its file cache
- `app/src/ui/global_search.rs` — the ripgrep search (with its in-process
  fallback) and the results view
- `app/src/ui/types.rs` — `SidebarView`, `ExplorerRow`, `SearchRow`
- `crates/goble-ui/src/elements/search_input.rs` — the field
- `crates/goble-ui/src/elements/hover_button.rs` — the "New conversation" row
