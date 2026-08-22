# 08 — Threads Toggle / View

## Behavior

- The topbar `threads` button toggles the center content between the normal **Chat view** and a **Threads view**.
- When active, the `threads` icon uses `Accent` color / background.

## Threads view content

Simple list layout:

- Header: `Threads` title.
- Scrollable list of thread items.
- Each item shows:
  - title
  - last message snippet
  - status icon (`check`, `x`, `circle` for stopped / running / errored)
  - timestamp

For MVP reuse `SidebarMenuItem` or a minimal `ThreadListItem` with status icons.

## Files

- `crates/goble-ui/src/views/threads_container.rs`
- `crates/goble-ui/src/views/thread_list_view.rs`
- `crates/goble-ui/src/views/thread_view.rs`
