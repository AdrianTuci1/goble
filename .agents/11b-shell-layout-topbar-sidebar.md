# 11b — Shell layout, topbar, and sidebar mode switcher

Part of: `11-warp-native-redesign-master.md`

## Goal
Build the main `ShellView` with a draggable macOS topbar and a mode-aware left sidebar, mirroring Warp's shell.

## Checklist

### Shell layout
- [ ] Create `goble-ui/src/views/shell_view.rs` with three zones: topbar, sidebar, content area.
- [ ] Use `Flex::row` for sidebar + content, `Flex::column` for topbar + body.
- [ ] Add optional `RightPanel` for details.
- [ ] Track shell state: `sidebar_collapsed`, `sidebar_mode`, `active_view`.

### Topbar
- [ ] Create `goble-ui/src/views/topbar.rs` using `Toolbar`.
- [ ] Left side: sidebar toggle button, agent-management button, threads shortcut.
- [ ] Right side: settings shortcut, macOS window controls placeholder.
- [ ] Drag handle: topbar background forwards drag events to platform window APIs on macOS.
- [ ] Double-click toggles maximize.

### Sidebar mode switcher
- [ ] Create `goble-ui/src/views/left_panel.rs`.
- [ ] Three mode buttons at top: Agent conversations, Threads, Drive.
- [ ] Active mode is highlighted.
- [ ] Collapsed mode shows only icons.

### Content area per mode
- [ ] **Agent conversations**: conversation list + `ChatView`.
- [ ] **Threads**: `ThreadSidebar` + `ThreadView` (reuse existing views).
- [ ] **Drive**: list of Plans / Rules / Workflows entries.

### State management
- [ ] Define `ShellAction` enum (toggle sidebar, set mode, open settings, open agent management).
- [ ] Dispatch actions from topbar and sidebar buttons.
- [ ] Shell re-renders on each action.

### Validation
- [ ] `cargo test -p goble-ui` passes with new layout tests.
- [ ] Native app shows the shell with a topbar and sidebar.
