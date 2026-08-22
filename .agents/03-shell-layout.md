# 03 — Shell Layout

## Four sections

```
+-------------------------------------------------------------+
|  Topbar (38px)                                              |
|  [● ○ ○]  Goble          [threads] [inbox] [user/settings]  |
+----------+--------------------------------+----------+---------+
|          |                                |          |         |
| Left     |  Chat / Threads content        | Right    |         |
| Sidebar  |                                | chat-    |         |
| 260px    |                                | sidebar  |         |
|          |                                | 280px    |         |
|          |                                |          |         |
+----------+--------------------------------+----------+---------+
```

## Responsibilities

- `ShellView` (`crates/goble-ui/src/elements/shell.rs`) owns the outer flex row.
- It holds the state for:
  - selected conversation / thread
  - left sidebar always visible (for MVP)
  - right chat-sidebar visible/hidden
  - threads mode active/inactive
- `TitleBar` is rendered above the row.

## Sizes

- Topbar height: `38px`.
- Left sidebar width: `260px`.
- Right chat-sidebar width: `280px`.
- Center content fills remaining width.

## Files

- `crates/goble-ui/src/elements/shell.rs`
- `crates/goble-ui/src/elements/titlebar.rs`
- `crates/goble-ui/src/views/chat_view.rs`
- `crates/goble-ui/src/views/thread_view.rs`
- `crates/goble-desktop-native/src/app.rs`
