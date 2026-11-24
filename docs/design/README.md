# Goble × Penpot design source

This folder holds the Penpot-ready design source for Goble.

## Files

- `goble-tokens.json` — DTCG design tokens (colors, spacing, radius, typography, density, shadows). Import it into Penpot via the **Tokens** panel.

## Workflow

1. Import `goble-tokens.json` into a new Penpot file.
2. Build / adjust components and layouts visually in Penpot using those tokens.
3. Export the final token set or share the Penpot link so the code can be updated to match.

## Current source of truth (code)

Tokens mirror:

- `crates/goble-ui/src/theme.rs` (Rust native UI theme — current source of
  truth for the wgpu native app)

## Status

Penpot MCP server is configured in `~/.grok/config.toml`. The Penpot UI/plugin
side must also be open and connected to a file for MCP write operations to
work.

Ready-to-use scripts for this project are in `docs/design/scripts/`:

- `upload-tokens.js` — uploads `goble-tokens.json` into a Penpot token set.
- `create-layout.js` — builds the `Goble Native App` main page (topbar,
  agent sidebar overlay, chat content, composer). The + button in the sidebar
  creates a new agent. Chat messages are formatted like an agentic TUI: the
  user's prompt is echoed with a `❯` prefix and the agent's reply is plain
  text — no avatar, author name, timestamp or bubble. The chat-history panel
  open state is created as a separate component (`History panel open`), and
  the routines sidebar is created as a separate component
  (`Chat sidebar open`), so neither is embedded in the layout.
- `create-settings-view.js` — builds the `Goble Settings` page. The LLM page
  shows a header with an "Add model" button and a list of model cards.
  The "Add model" overlay and the catalog of settings row types are created
  as separate components on the same page.
- `create-threads-view.js` — builds the `Goble Threads` page. The layout
  matches `threads_container.rs`: a topbar, a collapsible `ThreadSidebar`
  on the left (Channels, Direct messages, Chats), a vertical divider, and
  a `ThreadView` on the right. The thread header is a full-width flat bar
  (surface background, not a floating card) and the composer spans the full
  thread width; messages keep the Slack-style grouped format (avatar +
  author + timestamp).
- `create-chat-custom-renders.js` — adds the custom conversation renders as
  separate components on the `Goble Native App` page (outside the shell, at
  x=1500): `AskUserCard` (agent question + quick replies + credential fields),
  `TerminalBlock`, `ToolCall card`, `Queued prompt card`, and a
  `Credential composer` variant.

Run them via `penpot__execute_code` once Penpot is connected. The generic
`penpot` skill describes the Penpot workflow without project-specific code.

## Design tokens & icons sheet

The native app's visual language is defined once in
`crates/goble-ui/src/theme.rs` (the Rust source of truth) and mirrored as DTCG
tokens in `goble-tokens.json`. This is the contract the Settings overlay, the
topbar/chrome and the diff reviewer compose from — no page invents its own
values.

### Colour tokens

Sixteen semantic slots, resolved per theme (`dark`, `light`, `midnight`). Code
uses the slot (`ColorToken::Surface`), never a literal, so a preset swap
repaints every surface.

| Token | Purpose |
| --- | --- |
| `Bg` | Window/app background behind the panes |
| `Surface` | Cards, panes, the topbar strip, the Settings sheet |
| `SurfaceRaised` | Popovers, menus, floating panels, side sheets |
| `Border` | 1 pt casing around panes/cards/popovers |
| `Text` | Primary labels and transcript text |
| `Muted` | Secondary text, hints, timestamps, placeholders |
| `Hover` | Row/control hover band |
| `Selected` | Selected row/tab/toggle fill |
| `Focus` | Text-field caret and focus ring |
| `Accent` | Primary action fill, active state, slider fill |
| `Selection` | Text-selection band (accent mixed toward the surface) |
| `Success` / `Warning` / `Error` | Status dots, validation, destructive actions |
| `Path` | File paths in tool rows (warm orange) |
| `Command` | Shell commands in tool rows (yellow) |
| `Badge` | Numeric badge fill |
| `DiffInsertBg` / `DiffDeleteBg` | Added/removed change bands in diffs |

Token values mirror the JSON: dark `bg #0e0e0e`, `surface #161616`, `border
#262626`, `text #e8e8e8`, `muted #8e8e8e`; light `bg #f5f5f5`, `surface
#ffffff`, `border #e0e0e0`, `text #1f1f1f`, `muted #6f6f6f`. Accent presets
(`blue`, `green`, `purple`, `orange`) come from `Accent`, with status colours
kept separate (`status.success/warning/error/badge`).

### Typography

System UI + mono, four weights, six sizes. The app pairs 14.5 pt medium for the
panel title with 12–13 pt body/row text and 11 pt hints/key chips.

| Role | Size / weight |
| --- | --- |
| `xs` | 11 pt regular — hints, key chips, metadata |
| `sm` | 12 pt regular/medium — row labels, menu items, chips |
| `base` | 14 pt regular — body, transcript |
| `md` | 15 pt medium — panel title, page title |
| `lg` | 18 pt semibold — page headings |
| `xl` | 24 pt semibold — empty-state headings |

Weights: regular 400, medium 500, semibold 600, bold 700. Families: system
(`-apple-system`, `Segoe UI`, `Roboto`), mono (`ui-monospace`, `SFMono`,
`Menlo`, `Consolas`), serif reserved for prose. Native chrome uses the system
family; terminals, diff bodies and code blocks use mono.

### Spacing

A 4 pt base scale: `xs 4`, `sm 8`, `md 12`, `lg 16`, `xl 24`. Component
rhythm in the app: 2 pt control gaps inside a cluster, 8 pt between controls,
12 pt card padding, 16 pt main-section inset, 20–28 pt page gutters. Density
multipliers: compact 0.85, default 1.0, spacious 1.15.

### Radii

`sharp 0`, `default 8`, `rounded 14`. In practice: 6 pt key chips and small
controls, 8 pt buttons/segmented segments/inputs, 10–12 pt cards and popovers,
14 pt the Settings sheet and floating file panel, full pill (height/2) for tab
chips and the composer.

### Elevation

`sm 0 1px 2px rgba(0,0,0,.15)`, `md 0 4px 12px rgba(0,0,0,.25)`. Elevation is
used only for floating surfaces (menus, popovers, the floating file window);
panes and the Settings sheet rely on their `Border` casing instead.

### Controls sheet

All Settings pages are composed from the `goble-ui` settings primitives, never
ad-hoc widgets:

| Primitive | Used for |
| --- | --- |
| `SettingsSection` / `SettingsCard` | Section heading + grouped card |
| `SettingsRow` | Label, hint, status, reset and trailing control |
| `SettingsToggle` | Boolean switches (with disabled/focus states) |
| `SettingsSelect` | App-owned dropdown; open flag keyed by settings path |
| `SettingsSegmented` | Queue / Send Immediately, two-or-more-way writes |
| `SettingsSlider` | Numeric ranges (volume, font size) |
| `SettingsTextField` | Text/hex entry, per-field drafts |
| `SettingsSecretField` | Write-only secrets with configured placeholder |
| `SettingsButton` / `SettingsKeyChip` | Row actions, shortcut key caps |
| `SettingsNotice` | Inline info/warning/error band |
| `ReviewTab` | Diff reviewer header: file selector, +A/−D, split/unified, wrap, tools |
| `FileDiffView` | Unified/split diff body, wrapped or clipped, change bands |
| `PopupMenu` | More/Overview/tab menus, painted in the app's top layer |

Icons are 24×24 vector glyphs from `crates/goble-ui/assets/icons`, drawn at
15.5 pt in the topbar (30 pt hit box), 16 pt in rows, 13–14 pt inline in text
and key chips. Stroke weight is constant across sizes; the icon name is the
semantic id used by code (`ellipsis`, `flat-list`, `terminal`, `files`,
`search`, `plus`, `fullscreen-enter`, `panel-left-outline`,
`panel-right-outline`, …).

### Previews

Every side-panel tab owns a content topbar under the general bar: Terminal
(`zsh` + rail toggle), Task Output (`Background Task Output` + copy/rail
toggle), Side Question (`New side question`), Files (path), Agent and Overview;
Terminal and Task Output share the inner Terminals/Background Tasks rail with a
full-height 1 px separator. The floating file panel is movable by its titlebar,
resizes from the bottom-right grip, raises on click, and keeps its dock toggle.
Previews: `side-terminal-*`, `side-task-output-*`, `side-question-*`,
`floating-file-*`, `floating-file-small-*`.

`docs/design/previews/` holds the headless-rendered screenshots used to check
parity with the reference photos in `.refs/t3-new/`: shell (wide/narrow, dark
and light), topbar with More open (plain, Split flyout, Copy flyout) and
Overview open, the empty side panel's Start surface with its `+`, the Terminal
tab, the Side Question conversation, multiple tabs with `+` beside them and a
reordered strip, the tight-width overflow strip, the split mid-resize state
with the 1 pt inter-panel hairline, a stacked split's gutter band and close X,
the file explorer opened from Files, the terminal shell (an interactive
`user@host ~ %` prompt), the composer's project picker above the input, the Conversations heading with its
"+" and the shared project menu, the composer's model and permission trays, Settings
(with an open select above later rows, integrations, General, Appearance with its in-field swatch,
Appearance with the colour palette open, Shortcuts, narrow; no descriptions,
floating close x), the diff reviewer, the floating file window and the
changed-files card. Next to them: the DSH-style right-aligned user bubble and
inline image message (`chat-parity-*`), the slash menu floating above an
unchanged composer (`slash-menu-*`), the Review tab beside the main chat
(`split-review-*`), the file viewer with Review beside it
(`split-file-viewer-*`) and the narrow split (`split-review-narrow-*`). The
review surface has no inner header band: directly under the one general topbar
its file column sits on the left as a flat list (per-extension letter badge,
bold filename, muted tail-truncated path, counts, fold chevron, thin row
dividers and inline stacked diff; no per-row outer border) and the "Files
Changed" flat rail on the right at about 40% of the panel width (no per-row
border). The general topbar ends in the tab chips plus maximize and the side-panel
toggle; the review's own 1 px-separated row below it carries the kebab, search
and round menu. The
chat's changed-files card is the simpler T3 model: one collapsible bordered
card on the chat background, "N files changed" + counts + chevron and a
Review button, expanding to full-path rows with counts flush right; on narrow
widths only the path shrinks, with a leading ellipsis.

The topbar Search opens the centered spotlight (`spotlight-*`): scope pill,
query field, sort/filter menu and real conversation rows. `spotlight-compare-*`
puts the supplied reference and the Goble render side by side at the same full
window scale. Files open in the right panel's Files tab (`side-files-*`), not a
main pane; the left sidebar carries Projects/Conversations only and no file
tree (`sidebar-no-files-*`). The review surface follows `review_target.webp`:
full-width file cards with a 1 px rounded Border, the real per-extension file
icon, bold name, left-truncated path, counts and a trailing chevron; the
expanded file shows its inline diff inside the card with line numbers,
full-width add/del bands and syntax colours; a fixed 260 pt Files Changed rail
sits on the right. The review topbar ends in the More kebab and the round panel
menu, and a 1 px rule separates panel 1 from panel 2; tabs and title centre on
the 44 pt bar. `review-compare-*` is the same-scale side-by-side. The side
panel launcher follows `side_panel_launcher_target.webp`: capsule search with
a blue focus ring, hover-card rows (Review/Open File/Terminal/Browser/Side
Question/Plugins), a Background Tasks count pill with four rows and See all;
`launcher-compare-*` is the same-scale comparison. The tab-strip
evidence shows multiple chips, a hovered chip with its close X and the pinned
`+` menu (`side-tabs-hover-*`, `side-tabs-add-*`). The shell material (topbar, sidebar) is the grey `Surface`; the content
material (`ContentBg`, the shell Bg pushed toward black) is shared by the
panes, cards, workspace sheet, chat lane, spotlight and settings sheet. There
is no border, rounded casing, shadow or 4 px band around content; the only
separation is the full-height 1 px main/side hairline. The topbar's two panel
toggles use DSH's own `panel-left-outline` glyph and its mirrored right-hand
twin. The active conversation title is a rounded pill that turns into an inline
editor with a selected value and caret on click; Enter commits through the
durable rename path, Escape cancels, and a hover tooltip reads "Edit
Conversation Title". Narrow Settings keeps page navigation in one floating
hamburger instead of the rail. The Settings sheet uses a full-height
secondary rail (small muted heading, primary-text menu labels, no search field,
no user footer, no row/section rules) and long 120 pt Interface Layout tiles
with an 8 pt gap.
