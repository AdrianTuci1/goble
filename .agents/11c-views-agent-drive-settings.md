# 11c — Views: agent management, drive, and settings

Part of: `11-warp-native-redesign-master.md`

## Goal
Implement the three major view families: agent management panel, Drive sections, and a tabbed settings view.

## Checklist

### Agent management panel
- [ ] Create `goble-ui/src/views/agent_management_view.rs`.
- [ ] Model layer: map `goble-desktop-service` executions / traces to `AgentRun` entries.
- [ ] Header: search input + status filter buttons (All / Working / Done / Failed).
- [ ] Run cards: icon, title, status badge, timestamp, harness, artifacts.
- [ ] Details panel on the right: run id, agent, tools, recent tool results, open trace.

### Drive views
- [ ] Create `goble-ui/src/views/drive_panel.rs`.
- [ ] Sections: Plans, Rules, Workflows.
- [ ] Each section lists items from `DesktopState`.
- [ ] Selecting an item opens its detail view or editor placeholder.

### Settings view
- [ ] Extend existing `SettingsView` with all tabs (Profile, Keys, Appearance, Notifications, Shortcuts, Local archive, Agents, Compute, Mobile, Updates, Cluster).
- [ ] Wire Cluster tab using existing `ClusterInstallCard` equivalent or build a new one.
- [ ] Deep-link tabs via an enum rather than strings.

### Validation
- [ ] `cargo test -p goble-ui` passes.
- [ ] Native app can open each major view and filters work.
