# 10 — wgpu/winit platform layer

**Status:** `[x]` working
**Owns:** the low-level rendering platform
**Depends on:** [`README.md`](README.md)

## Components (`goble-ui/src/platform`)

| Module | Role |
| --- | --- |
| `app.rs` | app/event loop glue, `run_with_root` |
| `window.rs` | winit window + surface |
| `wgpu_render_engine.rs` | wgpu pipeline, frame present |
| `text_atlas.rs` | glyph atlas from `fontdue` metrics (Roboto + Hack) |
| `icon_atlas.rs` | SVG icons baked into an atlas |
| `linux.rs` / `mac.rs` / `windows.rs` | platform specifics (backends, features) |

## Responsibilities

- Create a window, pick a backend (Metal/Vulkan/Gles), size the swapchain, and present the painted scene.
- Own the text + icon atlases so glyph/icon rendering is cheap (no per-frame layout work in the hot path).
- Expose `Element` layout/paint/event dispatch (in `goble-ui`), which the hot UI (see [`../06-renderer/renderer-architecture.md`](../06-renderer/renderer-architecture.md)) builds against.

## Constraints

- The element tree is rebuilt each frame; the platform layer must tolerate that (no stale atlas references after a rebuild).
- Keep resize + DPI change cheap.
- The ABI (`goble-ui`) is what the executable links against; the hot crate (`goble-ui-hot`) is the thing swapped live.

## Tasks

- [ ] Confirm all three backends + DPI resize are covered by tests/handling.
- [ ] Profile the atlas construction (text/icon) path to keep startup + scroll cheap.
