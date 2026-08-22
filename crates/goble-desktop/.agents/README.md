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
- [x] `003-native-ui-architecture.md` — Arhitectura noului crate `goble-ui`.
- [x] `004-design-tokens.md` — Portarea design tokens în Rust.
- [x] `005-layout-primitives.md` — Primitive de layout (Flex, Stack, Scroll etc.).
- [x] `006-text-and-icons.md` — Text, fonturi, iconițe SVG.
- [x] `007-button-input-primitives.md` — Butoane, input-uri, switch, checkbox, chip.
- [x] `008-list-and-sidebar.md` — Liste și componente sidebar.
- [x] `009-topbar.md` — Topbar nativ premium.
- [x] `010-conversation-sidebar.md` — Sidebar de conversație custom.
- [x] `011-chat-view.md` — Chat header, content gol pentru conversație nouă și composer.
- [x] `012-conversation-sidebar-wiring.md` — Legarea sidebar-ului de conversație cu date reale.
- [x] `013-chat-composer-enhancements.md` — Selector model, runtime, API key card.
- [ ] `014-chat-right-sidebar-tabs.md` — Tab-uri Info/History în chat sidebar.
- [ ] `015-agents-page.md` — Parity pagină agenți cu Tauri.
- [ ] `016-connectors-page.md` — Pagina Connectors (MCP).
- [ ] `017-workflows-page.md` — Pagina Workflows.
- [ ] `018-teams-page.md` — Pagina Teams.
- [x] `019-executions-agenttrace-page.md` — Pagini Executions și AgentTrace.
- [ ] `020-logs-page.md` — Pagina Logs.
- [ ] `021-search-page.md` — Pagina Search.
- [ ] `022-threads-enhancements.md` — Participanți, reacții, mentions, unread.
- [ ] `023-dual-build-qa.md` — Dual build și QA final.

## Graf de dependențe (simplificat)

```
009-topbar ─┬─> 010-conversation-sidebar ─┬─> 011-chat-view ─┬─> 012-conversation-sidebar-wiring
            │                             │                  └─> 013-chat-composer-enhancements
            │                             │                  └─> 014-chat-right-sidebar-tabs
            │                             │
            ├─> 015-agents-page ──────────┴────────────────> 014-chat-right-sidebar-tabs
            ├─> 016-connectors-page
            ├─> 017-workflows-page
            ├─> 018-teams-page
            ├─> 019-executions-agenttrace-page ────────────> 014-chat-right-sidebar-tabs
            ├─> 020-logs-page
            └─> 021-search-page

022-threads-enhancements (parallel cu 012-021, depinde de 011)
023-dual-build-qa (depinde de toate celelalte)
```
