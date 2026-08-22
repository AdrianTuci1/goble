# 04 — Conversation Sidebar (left)

## Structure

```
+------------------+
| [🔍 Search...] + |
+------------------+
| Agent Name      1h |
| last response...   |  <- hover -> [⋯]
+------------------+
| Agent Name    2:30 |
| another snippet... |
+------------------+
|        ...         |
+------------------+
| ⚡ Plugins         |
+------------------+
```

## Header

- Rounded search input with `search` icon and placeholder text.
- `+` / `new-conversation` icon button on the right of the search field.
- Height: ~48px, padding `md`.

## Conversation card

Element: `ConversationListItem` (`crates/goble-ui/src/elements/conversation_list_item.rs`).

- Left: `Avatar` or `Icon` (agent icon).
- Center column:
  - Name in `Text` (`ColorToken::Text`, medium weight).
  - Last response snippet in `Text` (`ColorToken::Muted`, truncated with ellipsis).
- Top-right: timestamp in `Text` (`ColorToken::Muted`, smaller size).
- Unread dot (`Accent`) on the right when unread.
- Rounded `Surface` background by default.
- Hover: `Hover` background; reveal a `dots-horizontal` icon on the far right.
- Selected: `Selected` background.

## Three-dots delete menu

- On hover of the card, show `dots-horizontal` icon.
- Clicking the dots opens a small floating menu with a `Delete` row (red `Error` text).
- Clicking Delete fires a callback supplied by the parent.

## Plugins footer

- Fixed at the bottom of the left sidebar.
- One row with a `plug`/`layers` icon and the label `Plugins`.
- Tappable; for MVP it can be a placeholder action.

## Files

- `crates/goble-ui/src/elements/conversation_list_item.rs` — new
- `crates/goble-ui/src/elements/sidebar.rs` — refactor to use it
- `crates/goble-ui/src/elements/search_input.rs` — reuse
- `crates/goble-ui/src/elements/icon_button.rs` — reuse
