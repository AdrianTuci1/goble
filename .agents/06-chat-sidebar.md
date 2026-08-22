# 06 — Right Chat Sidebar

## Structure

```
+------------------+
| Computer Use     |
| +-------------+  |
| | preview     |  |
| +-------------+  |
| (active now)     |
+------------------+
| Routines      +  |
+------------------+
| ● Morning social |
|   Every day 8 AM |
+------------------+
| ● Outbound weekly|
|   Fridays 10 AM  |
+------------------+
```

## Computer Use section

- Header row: title `Computer Use`.
- Preview card: a rounded rectangle showing a computer/monitor image or placeholder.
- Status text below: `Active` / `Inactive` (`Success` or `Muted`).
- If no image asset exists, use a dark `SurfaceRaised` panel with a `computer` icon.

## Routines section

- Header row: title `Routines` + `+` icon button to add.
- List of `RoutineItem`s:
  - Leading small `circle` or `check` dot (`Accent` / `Muted`).
  - Title in `Text`.
  - Schedule description in `Muted` below.
- Hover reveals subtle `Hover` background.

## Visibility

- Hidden by default; toggled from the chat header or topbar.
- Width: `280px`, background `Surface`, left border `Border`.

## Files

- `crates/goble-ui/src/elements/right_panel.rs` — refactor to become this sidebar
- `crates/goble-ui/src/elements/routines_panel.rs` — new
