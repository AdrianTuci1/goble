# Agent 08 — UI Architecture (inspired by octomusui_core)

## Goal
Replace the placeholder `goble-ui` renderer with a minimal but professional
Rust-native WGPU UI framework, then build the **Execution View** that streams
live traces from `goblin-worker` over WebSocket.

## Why octomusui_core?
`octomusui_core` is Warp's retained-mode UI layer:
- `Element` trait with `layout`, `after_layout`, `paint`, `dispatch_event`.
- `Presenter` builds a `Scene` from the element tree.
- `Scene` stores layers of rects, glyphs, images, icons.
- `rendering` turns the scene into GPU commands.
- `platform` / `windowing` translate `winit` events into `Event`s.

We copy the *shape*, not the code. We keep dependencies minimal and only
implement what Goble needs: dark monochrome UI, lists, scroll, text, cards.

## New crate layout in `goble-ui`

```
crates/goble-ui/src
  lib.rs
  theme.rs          # design tokens (existing, expanded)
  scene.rs          # Scene, Layer, Rect, Glyph, Border, Fill, ClipBounds
  elements/
    mod.rs          # Element trait, Point, Axis, Padding, Margin, Fill helpers
    stack.rs        # vertical/horizontal stack container
    rect.rs         # colored/bordered rectangle
    text.rs         # single-line and wrapped text
    scrollable.rs   # clip + translate children
    list.rs         # virtual / simple list
    button.rs       # clickable rect + text
    card.rs         # rounded panel with optional header
  presenter.rs      # layout pass + paint pass -> Scene
  rendering/
    mod.rs          # scene -> wgpu render pass
    text_cache.rs   # glyphon / fontdue atlas cache
    shaders/        # wgsl for rect, text, maybe rounded corners
  platform.rs       # winit event -> Event mapping
  window.rs         # WindowState: surface, event loop bridge
  components/
    mod.rs
    badge.rs
    sidebar.rs
    trace_row.rs
    trace_detail.rs
  views/
    mod.rs
    execution_view.rs   # root view for agent traces
    mcp_view.rs         # optional: browse installed MCP servers
  app.rs            # AppContext + top-level event loop
  assets/
    fonts/          # Inter or JetBrains Mono
    icons/          # tiny SVG/PNG icon set
```

## Core abstractions

### `Element` trait
```rust
pub trait Element {
    fn layout(&mut self, constraint: SizeConstraint, ctx: &mut LayoutContext) -> Vector2F;
    fn after_layout(&mut self, ctx: &mut AfterLayoutContext);
    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext);
    fn dispatch_event(&mut self, event: &Event, ctx: &mut EventContext) -> bool;
    fn size(&self) -> Option<Vector2F>;
    fn origin(&self) -> Option<Point>;
}
```
- Containers call `layout` on children, accumulate size, store child origins.
- `PaintContext` provides `scene.push_rect(...)`, `scene.push_glyph(...)`.
- `EventContext` carries mouse position, focus, and a callback channel.

### `Scene`
- Layers with clip bounds.
- Primitives: `Rect`, `Glyph`, `Image`.
- Hit-map: simple AABB tree or vector search (good enough for MVP).
- Coordinate system: points, logical px, scale factor applied at render time.

### `Presenter`
- Owns the root `Box<dyn Element>`.
- On resize / state change:
  1. `layout(root, SizeConstraint::new(width, height))`
  2. `after_layout`
  3. `paint(root, origin(0,0))` into a fresh `Scene`
- On event: top-down `dispatch_event`, respecting z-index / overlay layers.

### Rendering
- Reuse existing `Renderer` WGPU setup.
- Add a `render_scene(&Scene)` method that draws:
  1. Solid / gradient rects (instanced quad with per-instance color + radius).
  2. Textured quads for glyphs using a glyph atlas.
- Keep shader pipeline simple; rounded corners can be SDF in fragment shader.

### Text
- Use `fontdue` or `cosmic-text` for layout and rasterization.
- Glyph cache as a `wgpu::Texture` atlas updated incrementally.
- Default fonts: Inter UI, JetBrains Mono for logs.

## Execution View features
- Left sidebar: agent list, status badges (Idle/Running/Success/Error).
- Main area: trace tree (collapsible), log lines, timestamps.
- Live updates via WebSocket (`/ws` on worker) proxied through desktop.
- Toolbar: connect/disconnect, refresh, run demo agent.
- Dark theme, monospace logs, color-coded log levels.

## WebSocket bridge
- `goble-desktop` opens a paired WebSocket to `goblin-worker`.
- Incoming `WorkerMessage::TraceUpdate` and `AgentFinished` update shared state.
- `goble-ui` reads the state each frame and rebuilds the view.

## Phases
1. **Scene + primitives**: `scene.rs`, `Fill`, `Rect`, `Border`, `Layer`.
2. **Elements**: `stack`, `rect`, `text`, `scrollable`.
3. **Presenter + WGPU rendering**: convert scene to render pass.
4. **Platform + window**: wire winit events to element tree.
5. **Components**: `button`, `card`, `badge`, `trace_row`.
6. **Execution view**: list agents, render traces, scroll logs.
7. **WebSocket integration**: desktop feeds state into view model.

## Dependencies to add
- `fontdue` — font parsing / rasterization
- `etagere` or simple atlas — glyph texture packing
- `image` — already in workspace
- `taffy` (optional later) — flex layout if we don't write our own

## Rejected alternatives
- `egui` / `iced`: immediate-mode or high-level; doesn't match the
  retained professional look of octomusui and is harder to customize deeply.
- Copying `octomusui_core` crates verbatim: too many internal Warp deps.

## Success criteria
- `goble-ui` compiles and a headless window test can layout + paint a scene.
- Execution view renders a mock trace tree with logs and scrolls.
- WebSocket state updates refresh the view without jank.
