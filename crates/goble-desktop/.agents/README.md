# Goble native Rust UI — `.agents` task folder

Acest director conține task-urile incrementale pentru migrarea aplicației Goble Desktop de la Tauri+React la o interfață nativă Rust, inspirată din stilul și interacțiunile `~/Projects/warp-new`.

## Reguli de lucru

1. Fiecare fișier este un task cu număr, titlu, context, obiective, criterii de acceptare și fișiere afectate.
2. Task-urile sunt gândite să fie executate în ordine; unele pot rula în paralel, dar se va menționa explicit.
3. După finalizarea unui task se adaugă `[x]` în titlu și se actualizează acest README cu statusul.
4. Se preferă modificări mici, verificabile, care nu strică build-ul Tauri existent.
5. La final trebuie să putem alterna între aplicația Tauri (`npx tauri dev`) și aplicația nativă (`cargo run --bin goble-native`).

## Task-uri active și viitoare

- [x] `001-understand-warp-ui.md` — Copiază/adaptă nucleul generic de UI din `warp-new`.
- [x] `002-primitive-catalog.md` — Catalog de ~58 primitive adaptat la nevoile Goble.
- [ ] `003-native-ui-architecture.md` — Arhitectura noului crate `goble-ui`.
- [ ] `004-design-tokens.md` — Portarea design tokens în Rust.
- [ ] `005-layout-primitives.md` — Primitive de layout (Flex, Stack, Scroll etc.).
- [ ] `006-text-and-icons.md` — Text, fonturi, iconițe SVG.
- [ ] `007-button-input-primitives.md` — Butoane, input-uri, switch, checkbox, chip.
- [ ] `008-list-and-sidebar.md` — Liste și componente sidebar.

Task-urile vor fi adăugate pe măsură ce avansăm (chat/composer, cards, agents, settings, integrarea cu `goble-core`, dual build).
