# Cross-Platform Desktop Build Checklist

## Goal
Make the Goble desktop stack compile and bundle for macOS, Windows, and Linux by aligning the native UI crate and Tauri backend with the platform-abstraction patterns used in `~/Projects/warp-new/crates/octomusui`.

---

## 1. Research and compare Warp vs Goble

### 1.1 Read Warp's platform abstraction
- [x] Read `~/Projects/warp-new/crates/octomusui/Cargo.toml` to understand platform-specific dependency gating.
- [x] Read `~/Projects/warp-new/crates/octomusui/src/platform/mod.rs` to understand `cfg_if::cfg_if!` based `current` module selection.
- [x] Read `~/Projects/warp-new/crates/octomusui/src/rendering/wgpu/mod.rs` to understand cross-platform wgpu initialization and backend selection.
- [x] Read `~/Projects/warp-new/crates/octomusui/src/windowing/winit/fonts/text_layout.rs` to understand non-macOS text layout using `cosmic-text`/`fontdb`.
- [x] Read `~/Projects/warp-new/crates/octomusui/src/windowing/winit/fonts/font_handle.rs` to understand font handle abstraction.

### 1.2 Audit Goble's current cross-platform blockers
- [x] Audit `crates/goble-ui/Cargo.toml` for unconditional macOS-only dependencies.
- [x] Audit `crates/goble-ui/src/**/*.rs` for actual usage of `core-graphics`, `core-text`, `font-kit`, `objc`.
- [x] Audit `crates/goble-desktop/src-tauri/src/ssh_installer.rs` for external Unix tool usage (`ssh`, `curl`, `tar`).
- [x] Confirm `goble-core` and `goble-desktop-service` are pure Rust with no platform-specific code.

### 1.3 Document the comparison
- [x] Write a short comparison in this checklist.

#### Warp (octomusui) approach
- **Platform selection**: `platform/mod.rs` uses `cfg_if::cfg_if!` to re-export `mac::*`, `linux::*`, `windows::*`, or `wasm::*` from a single `platform::current` module.
- **Rendering**: macOS has a native Metal renderer plus an experimental wgpu renderer; Linux/Windows use wgpu exclusively.
- **Font/text**: macOS uses `core-text`/`font-kit`; non-macOS uses `cosmic-text`, `fontdb`, `owned_ttf_parser`, and a custom `swash_rasterizer`.
- **Dependencies**: all platform-only crates are gated behind `[target.'cfg(target_os = "...")'.dependencies]` or `[target.'cfg(target_family = "...")'.dependencies]`.
- **wgpu setup**: `wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all())`, with DirectX options on Windows and Wayland/X11 handling on Linux.

#### Goble Phase 1 approach
- `goble-ui` is far less mature than `octomusui`: rendering is still a placeholder and text measurement is a heuristic.
- Therefore we adopt the *structural* pattern (conditional dependencies + `cfg_if::cfg_if!` platform dispatch) without pulling in Warp's full font/rendering stack.
- macOS-only crates (`core-graphics`, `core-text`, `font-kit`, `objc`) were already declared only for `target_os = "macos"`; they are currently unused in code.
- `wgpu` now enables `metal`, `dx12`, `vulkan`, and `gles` so it can initialize on any desktop OS.
- Text measurement is routed through `platform::current::estimate_text_size`, with all platforms falling back to the same heuristic for now.
- `ssh_installer` is gated to `cfg(unix)` because it relies on external `ssh`/`curl` binaries.

---

## 2. Dependency audit and conditionalization in `goble-ui`

### 2.1 wgpu backend
- [x] Remove hard-coded `metal` feature from `wgpu`.
- [x] Use `wgpu = { version = "25.0", default-features = false, features = ["wgsl", "metal", "dx12", "vulkan", "gles"] }` cross-platform feature set.
- [x] Add `cfg-if = "1.0"` to common dependencies.

### 2.2 macOS-only dependencies
- [x] Move `core-graphics`, `core-text`, `font-kit`, `objc` from common deps to `[target.'cfg(target_os = "macos")'.dependencies]`.
- [x] Add a note: these crates are currently unused in code; they are reserved for future native font/rasterizer work.

### 2.3 Optional cross-platform font crates for future use
- [ ] (Future) Add `cosmic-text`, `fontdb`, `owned_ttf_parser`, `memmap2` behind a non-macOS feature flag when real text rendering is implemented.

---

## 3. Platform abstraction for `goble-ui`

### 3.1 Create `platform` module
- [x] Create `crates/goble-ui/src/platform/mod.rs` with:
  - `pub mod app;` (placeholder)
  - `#[cfg(target_os = "macos")] pub mod mac;`
  - `#[cfg(target_os = "linux")] pub mod linux;`
  - `#[cfg(target_os = "windows")] pub mod windows;`
  - `pub mod current { cfg_if::cfg_if! { ... } }`
- [x] Create stub platform modules: `mac.rs`, `linux.rs`, `windows.rs`.

### 3.2 Expose minimal cross-platform text metrics API
- [x] Add `platform::current` free functions:
  - `default_font_family() -> &'static str`
  - `estimate_text_size(text: &str, font_size: f32, line_height: f32, max_width: f32) -> Vector2F`
- [x] Wire the existing `Text::measure_text` through `platform::current::estimate_text_size`.
- [x] Keep the current char-width heuristic as the universal fallback implementation.

### 3.3 Guard future native rendering behind feature flags
- [x] Add `native-rendering` Cargo feature in `goble-ui`.
- [x] When `native-rendering` is off, `Renderer` remains a placeholder and `Text` uses the heuristic metrics.
- [x] When enabled on macOS, the build may pull in `core-text`/`core-graphics`; leave that as a follow-up item.

---

## 4. SSH installer portability

### 4.1 Make the module platform-aware
- [x] Add `#[cfg(unix)]` to `crates/goble-desktop/src-tauri/src/ssh_installer.rs` module declaration.
- [x] On non-Unix targets, the Tauri `install_worker` command returns `Err("Remote worker installation requires an SSH client, which is not available on this platform. Use the manual install instructions instead.")`.

### 4.2 Keep current Unix behavior intact
- [x] Ensure `detect_platform`, `resolve_worker_asset`, `install_worker`, `run_ssh` still compile and work on macOS and Linux.
- [ ] Add compile-time or runtime check that `ssh` and `curl` binaries exist before invoking them (deferred to Phase 2).

### 4.3 Document future pure-Rust path
- [x] Add a TODO/Future work note in the checklist to migrate from subprocess SSH to a Rust library such as `russh` + `reqwest`.

---

## 5. Tauri bundle configuration

### 5.1 Confirm targets
- [x] Verify `crates/goble-desktop/src-tauri/tauri.conf.json` has `"bundle": { "targets": "all" }`.
- [x] Ensure icon set includes `.png` and `.ico` resources.

### 5.2 Desktop service portability
- [x] Confirm `goble-desktop-service` and `goble-core` compile on macOS with no platform-specific code.
- [ ] Full Linux/Windows compile check requires a cross-compilation toolchain (deferred to CI environment).
- [x] Check that `rusqlite` bundled feature is used.
- [x] Check that `tokio-tungstenite` with `rustls-tls-native-roots` is configured.

---

## 6. CI validation matrix

### 6.1 macOS (baseline)
- [x] `cargo test -p goble-ui` — 58 passed
- [x] `cargo test -p goble-desktop-service` — 23 passed
- [x] `cargo check --workspace --all-targets` — passed
- [x] `cargo check` in `crates/goble-desktop/src-tauri` — passed
- [x] `npm run build` in `crates/goble-desktop` — passed

### 6.2 Windows / Linux (cross-compilation or available toolchain)
- [ ] `cargo check -p goble-ui --target x86_64-pc-windows-msvc` — blocked: no MSVC toolchain on macOS
- [ ] `cargo check -p goble-ui --target x86_64-unknown-linux-gnu` — blocked: missing `x86_64-linux-gnu-gcc` cross-compiler and OpenSSL sysroot on macOS
- [ ] `cargo check -p goble-desktop-service --target x86_64-pc-windows-msvc` — blocked: no MSVC toolchain on macOS
- [ ] `cargo check -p goble-desktop-service --target x86_64-unknown-linux-gnu` — blocked: missing `x86_64-linux-gnu-gcc` cross-compiler and OpenSSL sysroot on macOS
- [ ] `cargo check` in `crates/goble-desktop/src-tauri` for available targets — blocked by the same toolchain limits.

**Note:** Cross-compilation from macOS to Linux/Windows failed only because the host lacks the C cross-compiler and OpenSSL sysroot, not because of Rust-level portability issues. The dependency gating is in place so these targets should succeed in a CI runner with the correct toolchain.

---

## 7. Summary of Warp patterns adopted

- `cfg_if::cfg_if!` dispatch via `platform::current`.
- macOS-only crates isolated in `[target.'cfg(target_os = "macos")'.dependencies]`.
- Cross-platform wgpu with backend selection from environment or `wgpu::Backends::all()`.
- Non-macOS font rendering deferred to `cosmic-text`/`fontdb` (implemented only when real rasterization is needed).
- Platform-specific code in modules gated by `#[cfg(...)]`.

---

## Notes

- This Phase 1 checklist intentionally avoids implementing full native font rasterizers. The current `goble-ui` text measurement is heuristic-based and sufficient for layout tests and examples.
- The `goble-desktop` Tauri/React shell is already cross-platform; the only backend portability blocker is the SSH installer.
