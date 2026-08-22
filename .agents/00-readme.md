# Goble Native Desktop UI — Design Document Graph

This folder contains the design specs for the real layered Goble native desktop UI built in Rust (`goble-ui` + `goble-desktop-native`).

The implementation is driven by the master plan in this conversation:

- Plan ID: `1914c07e-9ab5-4627-b674-35b8f70ad7b6`
- Branch: `feature/agent-guide-ui`

## Document graph

```mermaid
flowchart TB
  readme["00-readme.md"] --> tokens["01-design-tokens.md"]
  readme --> assets["02-icon-font-system.md"]
  readme --> shell["03-shell-layout.md"]
  shell --> sidebar["04-conversation-sidebar.md"]
  shell --> chat["05-chat-view.md"]
  shell --> right["06-chat-sidebar.md"]
  shell --> topbar["07-topbar.md"]
  topbar --> threads["08-threads-toggle.md"]
  sidebar --> tests["09-testing-checklist.md"]
  chat --> tests
  right --> tests
  topbar --> tests
  threads --> tests
  tokens --> sidebar
  tokens --> chat
  tokens --> right
  tokens --> topbar
  assets --> sidebar
  assets --> chat
  assets --> right
  assets --> topbar
  assets --> threads
```

## Four main UI sections

1. **Topbar** — macOS traffic lights + title on the left; `threads` toggle, `inbox`, `user settings` on the right.
2. **Left sidebar — conversation list** — search + create button, scrollable conversation cards, fixed `Plugins` footer.
3. **Chat view** — header with conversation name + right-sidebar toggle, message bubbles, bottom composer.
4. **Right chat-sidebar** — `Computer Use` preview and `Routines` list.

## Principles

- Build natively in Rust; no emoji icons.
- All icons come from real SVG assets copied from `~/Projects/warp-new/app/assets/bundled/svg/` and rendered through an `IconAtlas`.
- Text uses the bundled Roboto family with real `fontdue` metrics.
- All colors, spacing, radii, and fonts are taken from `goble-ui/src/theme.rs` tokens.
- Components paint hover / selected backgrounds from `InteractiveState`.

## Status legend

Use inline checkmarks in each doc while implementing:

- `[ ]` not started
- `[~]` in progress
- `[x]` done / validated
