# 07 — Topbar

## Structure

```
+-------------------------------------------------------------+
| [● ○ ○]  Goble                         [threads] [inbox] [user] |
+-------------------------------------------------------------+
```

## Left side

- macOS traffic lights (close / minimize / maximize).
- Window title text `Goble` or app name.

## Right side controls

From left to right:

1. **Threads toggle** — `threads` icon. Active state uses `Accent` background or color.
2. **Inbox** — `bell` or `inbox` icon; optional `Badge` dot for unread.
3. **User / Settings** — `user` avatar or `settings` icon; opens a small menu with settings / profile.

All are `IconButton`s with hover backgrounds.

## Wiring

- `TitleBar` receives callbacks from `ShellState`:
  - `on_toggle_threads()`
  - `on_open_inbox()`
  - `on_open_user_settings()`
- `ShellView` stores `threads_active` and toggles the center content between `ChatView` and a `ThreadsView`.

## Files

- `crates/goble-ui/src/elements/titlebar.rs`
- `crates/goble-ui/src/elements/icon_button.rs`
- `crates/goble-ui/src/elements/shell.rs`
