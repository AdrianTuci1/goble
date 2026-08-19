# 002 — Catalog de ~58 primitive adaptat la nevoile Goble

## Context
Warp are mult mai multe componente decât avem nevoie. Trebuie să reducem la ~58 primitive care acoperă toate ecranele actuale din `goble-desktop`.

## Obiective
1. Inventariază componentele React existente:
   - `src/components/*.tsx`
   - `src/views/**/*.tsx`
   - `src/pages/*.tsx`
   - elementele comune din `src/index.css`.
2. Compară-le cu primitivele disponibile în `octomusui_core/src/ui_components` și `crates/ui_components`.
3. Construiește un catalog final de ~58 primitive, împărțit în:
   - base / layout
   - typography
   - media
   - status / feedback
   - inputs
   - navigation
   - data display
   - domain Goble
4. Pentru fiecare primitivă definește:
   - numele Rust
   - props (obligatorii/optionale)
   - stări (hover, active, disabled, selected, focus)
   - stiluri implicate (culoare, padding, radius, font)
   - componenta React echivalentă
   - dacă este must-have pentru MVP sau poate veni mai târziu

## Criterii de acceptare
- Fișier `notes/primitive-catalog.md` cu tabelul/structura celor ~58 primitive.
- Fiecare primitivă are o motivație scurtă (de ce este necesară, ce înlocuiește din React).
- Lista este validată împotriva ecranelor: Chat, Threads, Agents, Settings, Connectors, Vault, Workers, History.
- Catalogul nu depășește 60 primitive; dacă depășește, se grupează/eliminează duplicate.

## Fișiere de referință (doar citite)
- `src/components/**/*.tsx`
- `src/views/**/*.tsx`
- `src/pages/*.tsx`
- `src/index.css`, `src/utils/designSystem.ts`
- `~/Projects/warp-new/crates/octomusui_core/src/ui_components/*.rs`
- `~/Projects/warp-new/crates/ui_components/src/*.rs`
