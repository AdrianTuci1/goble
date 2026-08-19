# 001 — Copiază/adaptă nucleul generic de UI din `warp-new`

## Context
`~/Projects/warp-new` folosește un framework de UI nativ Rust intern numit `octomusui` / `octomusui_core`.
Vrem să păstrăm *ideea* și *API-ul* acestui framework în `goble-ui`, dar nu să copiem tot codul. Nucleul generic este încâlcit cu dependențe AGPL/specifice Warp (`octomus_util`, `markdown_parser`, `settings_value`, logica de text avansată etc.), deci îl vom reimplementa minimal, inspirat de Warp.

## Obiective
1. Studiază fișierele cheie din `octomusui_core`/`octomusui`:
   - `crates/octomusui_core/src/elements/mod.rs` — trait `Element`, `Point`, `Fill`, `Border`, `Margin`, `Padding`.
   - `crates/octomusui_core/src/elements/flex/mod.rs` — `Flex`, `MainAxisAlignment`, `CrossAxisAlignment`.
   - `crates/octomusui_core/src/elements/stack/mod.rs` — `Stack`, `Positioned`, `OffsetPositioning`.
   - `crates/octomusui_core/src/elements/container.rs`, `constrained_box.rs`, `align.rs`, `clipped.rs`, `rect.rs`, `empty.rs`, `hoverable.rs`, `event_handler.rs`.
   - `crates/octomusui_core/src/ui_components/components.rs` — `UiComponentStyles`.
   - `crates/octomusui_core/src/ui_components/button.rs`, `checkbox.rs`, `switch.rs`, `chip.rs`, `text_input.rs`, `list.rs`.
2. Reimplementează în `goble-ui` versiuni minimal-echivalente:
   - trait `Element` cu `layout`, `paint`, `dispatch_event`.
   - tipuri geometrice (Vector2F, RectF, Size2F, Point).
   - tipuri de stil (ColorU, Fill, Border, Margin, Padding).
   - `Flex`, `Stack`, `Container`, `Clipped`, `ConstrainedBox`, `Align`, `Text`, `Icon`, `Rect`, `Empty`.
   - stiluri `UiComponentStyles` și un `Theme` inițial.
3. Creează structura de bază în `crates/goble-ui`:
   - `Cargo.toml` cu dependințe decuplate (`winit`, `wgpu`, `euclid`, `palette` etc.)
   - `src/element/` — trait și primitive de layout
   - `src/style/` — `Theme`, `UiComponentStyles`
   - `src/component/` — componente de bază
   - `src/render/` și `src/platform/` — stub minimal pentru wgpu/winit
4. Asigură-te că `cargo check -p goble-ui` trece.

## Criterii de acceptare
- Crate `crates/goble-ui` creat și adăugat în workspace.
- `cargo check -p goble-ui` executabil fără erori.
- Fișier `notes/octomusui-port-map.md` cu:
  - lista tipurilor reimplementate vs. copiate
  - dependințele înlocuite/stubuite
  - licențe (MIT pentru octomusui_core/octomusui; evităm dependențe AGPL)

## Fișiere de referință
- `~/Projects/warp-new/crates/octomusui_core/src/elements/**/*.rs`
- `~/Projects/warp-new/crates/octomusui_core/src/ui_components/*.rs`
- `~/Projects/warp-new/crates/octomusui_core/src/rendering/*.rs`
- `~/Projects/warp-new/crates/octomusui/src/windowing/*.rs`

## Note
- Nu copia codul de business din `app/src`.
- Nu copia fișiere care depind de `octomus_util`/`markdown_parser`; reimplementează minimal.
- API-ul trebuie să semene cu Warp pentru a reduce efortul la componentele de domeniu.
