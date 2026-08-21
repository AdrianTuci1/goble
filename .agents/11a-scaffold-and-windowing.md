# 11a — Scaffold and windowing backend

Part of: `11-warp-native-redesign-master.md`

## Goal
Create the native Rust app crate and a complete winit + wgpu windowing backend so `goble-ui` elements render and receive input at full fidelity.

## Checklist

### Crate scaffold
- [ ] Create `crates/goble-desktop-native/Cargo.toml` depending on `goble-ui`, `goble-desktop-service`, `goble-core`, `anyhow`, `tokio`.
- [ ] Add the crate to the workspace or build it standalone.
- [ ] Create `src/main.rs` → `app::run()` and `src/app.rs`.
- [ ] Boot `tokio` runtime, `Store`, `DesktopState`, and `CollectingEventBus`.

### Windowing backend
- [ ] Implement a complete winit-based `Window` and `EventLoop` in `goble-ui/src/platform`.
- [ ] Translate winit events into `DispatchedEvent` (MouseDown/Up/Move, KeyDown/Up, Scroll, Resize).
- [ ] Implement a full wgpu renderer supporting rectangles, rounded corners, borders, text, icons, and images.
- [ ] Add text measurement using core-text on macOS and font-kit fallback.
- [ ] Add icon rasterization via resvg.
- [ ] Implement damage regions / full redraw strategy.
- [ ] Support macOS window drag via empty topbar areas using `winit` / platform APIs.
- [ ] Support double-click maximize and window controls.

### Validation
- [ ] `cargo check -p goble-desktop-native` passes.
- [ ] `cargo run -p goble-desktop-native --bin goble-desktop` opens a window or runs headless without crashing.
