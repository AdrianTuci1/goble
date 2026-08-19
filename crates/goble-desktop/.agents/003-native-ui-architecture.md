# 003 — Arhitectura noului crate `goble-ui`

## Context
Vom crea un crate Rust nou care va conține toată interfața nativă și care nu depinde de Tauri.

## Obiective
1. Decide locația crate-ului:
   - Opțiunea A: `goble/crates/goble-ui` și includerea lui în workspace (trebuie rezolvate eventualele conflicte cu `goble-desktop`).
   - Opțiunea B: subdirector în `goble-desktop/native-ui`.
2. Definește structura modulelor:
   - `window/` — creare fereastră cu `winit`, event loop, DPI scaling.
   - `render/` — renderer `wgpu`, pipeline 2D, clear, primitive shapes, clipping.
   - `element/` — trait `Element` minimal (layout/paint/event dispatch), inspirat din `octomusui_core/src/elements/mod.rs`.
   - `style/` — `Theme`, `UiComponentStyles`, tokens, spacing, fonturi.
   - `component/` — cele ~58 primitive.
   - `app/` — aplicația Goble: fereastră principală, rutare, state, conectare la `goble-core`.
3. Definește cum se comunică cu backend-ul:
   - apel direct funcții `goble-core` prin API sincron/async
   - sau un nou crate `goble-desktop-service` care expune un API peste `goble-core` și este folosit atât de Tauri cât și de nativ.
4. Definește build-ul dual:
   - binarul Tauri rămâne `src-tauri/src/main.rs`.
   - binarul nativ: `goble-ui/src/bin/goble-native.rs` sau `goble-desktop/src/bin/native.rs`.
   - mecanism de switch: feature `native-ui` sau două crate-uri separate.

## Criterii de acceptare
- Un document `notes/goble-ui-architecture.md` cu:
  - structura modulelor și responsabilitățile
  - diagramă a fluxului de date (user input → element tree → render → wgpu)
  - dependențele externe (cu versiuni)
  - planul de integrare cu `goble-core`
- Un `Cargo.toml` inițial pentru `goble-ui` care compilează (poate fi gol, fără funcționalitate).
- Build-ul `goble-desktop` actual rămâne verde.

## Fișiere afectate (creare/modificare)
- `crates/goble-ui/Cargo.toml` (nou)
- `crates/goble-ui/src/lib.rs` (nou)
- `Cargo.toml` (workspace, posibilă ajustare `members`/`exclude`)
- `src-tauri/Cargo.toml` (doar citit, nu modificat încă)
- `crates/goble-core/src/lib.rs` (doar citit)
