# 02 — Icon & Font System

## Icon system (no emoji)

### Asset source

SVG icons are copied from `~/Projects/warp-new/app/assets/bundled/svg/` into `crates/goble-ui/assets/icons/`.

Required icons for the four-section UI:

- `close`, `minimize`, `maximize`
- `search`
- `plus`, `new-conversation`
- `dots-horizontal`
- `trash` / `delete` (use an `x` icon if no trash exists)
- `threads`
- `inbox` / `mail` / `bell`
- `user`, `settings`
- `chevron-left`, `chevron-right`, `chevron-down`
- `send` / `arrow-right` (for composer send)
- `paperclip` / `plus` (for attach)
- `computer`, `monitor` (for Computer Use section)
- `check`, `x`, `circle`, `cancelled` (status dots)

### Runtime rendering

1. `IconAtlas` (`crates/goble-ui/src/platform/icon_atlas.rs`) loads each SVG as bytes at init.
2. `resvg` + `usvg` rasterize each SVG into a fixed-size RGBA image (e.g., 64x64 source).
3. Images are packed into one `wgpu::Texture` atlas using a simple shelf packer.
4. `Icon` asks the atlas for UV coordinates by name and emits a `RenderCommand::DrawIcon`.
5. `WgpuRenderEngine` samples the atlas texture and tints the sampled alpha with the requested color.

### `Icon` element

- `Icon::new("search")` resolves to atlas entry.
- `Icon::with_color(...)` tints the SVG; default is `ColorToken::Text`.
- `Icon::with_size(16.0)` scales the drawn quad.
- Remove the old `icon_glyph` emoji map entirely.

## Font system

- `TextAtlas` loads `Roboto-Regular.ttf`, `Roboto-Medium.ttf`, `Roboto-Bold.ttf`, and optionally `Hack-Regular.ttf`.
- A static `fontdue::Font` per weight is stored lazily with `OnceLock`.
- `TextAtlas::measure(text, font_size, weight)` returns real pixel width and line height.
- The heuristic `measure_text` fallback in `crates/goble-ui/src/platform/mod.rs` is replaced by real metrics when a bundled font is available.

## Files to change

- `crates/goble-ui/src/platform/icon_atlas.rs` — new
- `crates/goble-ui/src/platform/wgpu_render_engine.rs` — icon texture + shader
- `crates/goble-ui/src/elements/icon.rs` — atlas lookup
- `crates/goble-ui/src/platform/text_atlas.rs` — multi-weight fonts + measure
- `crates/goble-ui/src/platform/mod.rs` — better text measurement fallback
- `crates/goble-ui/Cargo.toml` — ensure `resvg`, `usvg`, `fontdue` are present
