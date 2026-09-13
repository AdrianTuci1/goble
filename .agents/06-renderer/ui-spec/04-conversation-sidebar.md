# 04 — Conversation Sidebar (left)

## The surface is flat (R6, current)

The sidebar draws no rounded corner anywhere, carries no separator rule, and its
search field has no magnifier. Nothing here is a rounded widget: the column is a
flat band of rows inside the app's `Surface`, delimited by spacing alone.

| Surface | Element | Shape now |
| --- | --- | --- |
| Search field | `SearchInput::with_compact(true).with_icon(false).with_extra_height(3.0)` | square, no `search` icon, `3 px` taller above and below its text (`xs` = 4, so `7 px` of vertical padding at the default density) |
| "New conversation" row | `HoverButton::with_corner_radius(0.0)` | square hover highlight |
| `+` box | `Container` (`SurfaceRaised`, no radius) | square |
| Conversation card | `ConversationListItem` root `Container` | square band, full sidebar width |
| Card's environment badge | `Container` (`SurfaceRaised`, no radius) | square |
| Card's 3-dot control | `TopbarButton::with_corner_radius(0.0)` | square hover fill |
| Card's delete row | `Button::with_corner_radius(0.0)` | square |
| "View all (N)" / "Show less" | `Button::with_corner_radius(0.0)` | square fill |

`SearchInput`'s `with_icon(false)` and `with_extra_height(px)`, and the
`with_corner_radius(radius)` knob on `HoverButton`, `Button` and `TopbarButton`,
are the opt-ins that make this possible without changing those elements for the
rest of the app: their default is unchanged (the icon is drawn, the height is
the text's, the radius is the theme's or half of it).

Locked by tests — `elements::search_input::tests::
the_sidebar_search_is_a_taller_square_field_without_a_magnifier` (and its
counterpart `the_search_field_keeps_its_magnifier_by_default`, so the flag is a
real switch), and `ui::sidebar::sidebar_surface_tests::
the_sidebar_is_flat_square_and_free_of_separators_and_the_magnifier`, which
mounts the `RootView` over a real store in four hover states and asserts that
the band left of the workspace draws no rounded fill, no hairline spanning the
sidebar (a rule), and no `search` icon, while still drawing the field's own
border and the runs that make those assertions mean something. The rule that
tells a separator from a control's edge is itself tested
(`the_separator_rule_reports_a_rule_and_not_a_field_border`).

Known, not fixed: the `CONVERSATIONS` section caption is not drawn at all —
`Label::paint` (`crates/goble-ui/src/elements/label.rs`) records its origin and
emits no `DrawText`, so every `Label` in the app is invisible. Changing that is
its own item; the sidebar's layout already reserves the caption's height.

## Structure

```
+------------------+
| [Search]           |
| New conversation   |
| CONVERSATIONS      |
| Agent Name      1h |
| last response...   |
+------------------+
```

## Header

- Square search field, placeholder `Search`, no magnifier; a few pixels taller
  than its text so it reads as a field.
- The "New conversation" row below it: a `+` box and the label.

## Conversation card

Element: `ConversationListItem` (`crates/goble-ui/src/elements/conversation_list_item.rs`).

- Left: `Avatar` or `Icon` (agent icon).
- Center column:
  - Name in `Text` (`ColorToken::Text`, medium weight).
  - Last response snippet in `Text` (`ColorToken::Muted`, truncated with ellipsis).
- Top-right: timestamp in `Text` (`ColorToken::Muted`, smaller size).
- Unread dot (`Accent`) on the right when unread.
- Square `Surface` background by default (hover: `Hover`, selected: `Selected`) —
  a full-width band, not a rounded card.
- Hover: reveal a `dots-horizontal` icon on the far right.

## Three-dots delete menu

- On hover of the card, show `dots-horizontal` icon.
- Clicking the dots opens a small floating menu with a `Delete` row (red `Error` text).
- Clicking Delete fires a callback supplied by the parent.

## Files

- `crates/goble-ui/src/elements/conversation_list_item.rs` — the card
- `app/src/ui/sidebar.rs` — the column (search, "New conversation", the list, "View all")
- `crates/goble-ui/src/elements/search_input.rs` — the field
- `crates/goble-ui/src/elements/hover_button.rs` — the "New conversation" row
