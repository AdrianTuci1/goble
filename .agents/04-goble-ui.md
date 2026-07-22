# Agent 04 — goble-ui

## Responsabilitate
`goble-ui` este design system-ul: componente vizuale, teme, layout helpers, icoane, mascotă, renderer wgpu.

## Module
- `theme` — culori, fonturi, spacing, radius, shadows (evitate).
- `renderer` — setup wgpu, pipeline, clear, resize, comandă de desenare.
- `scene` — primitive 2D (rect, text, icon) abstracte peste wgpu.
- `components` — butoane, input, card, list, badge, tooltip, modal, sidebar, chat bubble.
- `icons` — SVG icon set, mascotă Goble, mascotă Goblin.
- `illustrations` — personaje, stări empty, loading.
- `layout` — panou responsive, split view, scroll, virtual list.

## Principii
- Stateless cât mai mult posibil; state primit prin parametri.
- Tema este globală, injectată în renderer.
- Suport dark/light, dar default dark.
- Componentele sunt testate vizual și cu unit tests pentru props.

## Test coverage
- Unit tests pentru constructori, stări, evenimente.
- Screenshot tests opționale (după MVP).
