# 10 — Platform & performance

**Status:** `[x]` platform works (wgpu/winit); perf budget to set
**Owns:** the wgpu/winit platform layer and the performance budget
**Depends on:** [`../06-renderer/README.md`](../06-renderer/README.md), [`../01-vision/README.md`](../01-vision/README.md)

## What this is

Cross-cutting: the low-level platform (window, swapchain, wgpu render engine, text/icon atlas, glyph/metrics) and the performance envelope we commit to. We write the rendering from scratch, so this layer must be efficient and predictable.

## Principles

- **No UI libraries.** The rendering stack is ours (`goble-ui/src/platform/*`): winit window, wgpu render engine, text atlas (`fontdue` metrics + Roboto), icon atlas (SVG). Design direction/tokens/icons come from `~/Projects/warp-new` (`app/src/themes/`, `app/assets/bundled/svg/`), and `warp-new/crates/octomusui` is the reference for this platform layer. This is the "maximum performance" mandate — see [`README.md`](../README.md).
- **The element tree is rebuilt every frame**; the renderer draws immediate-mode style from app state. Keep rebuild + paint cheap and avoid per-frame allocations that show up under load.
- **Remote streams must not block the frame loop** (see [`../06-renderer/remote-terminal-renderer.md`](../06-renderer/remote-terminal-renderer.md)) — decode on a worker thread, present on the main thread.

## Docs

- [`wgpu-renderer-platform.md`](wgpu-renderer-platform.md) — the platform layer components.
- [`performance.md`](performance.md) — the performance budget and measurement approach.

## Related

- [`../06-renderer/README.md`](../06-renderer/README.md) — the renderer subsystems built on this platform.
- [`../01-vision/README.md`](../01-vision/README.md) — why we own the renderer.
