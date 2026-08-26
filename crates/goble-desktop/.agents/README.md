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
- [ ] `012-routines-sidebar-wiring.md` — Sidebar stânga cu rutine reale și resizable.
- [ ] `013-agent-chat-composer.md` — Composerul chat-ului principal: limbaj natural + slash extras.
- [ ] `014-routine-chips.md` — Chips în topbar-ul chat-ului pentru rutine deschise.
- [ ] `015-routine-trace-in-chat.md` — Trace/log-uri ale rutinei afișate în view-ul de chat.
- [ ] `016-connectors-page.md` — Pagina Connectors (MCP).
- [ ] `017-routines-list-item-design.md` — Designul vizual al item-ilor de rutină în sidebar.
- [ ] `018-teams-page.md` — Pagina Teams.
- [ ] `022-threads-enhancements.md` — Participanți, reacții, mentions, unread.
- [ ] `023-dual-build-qa.md` — Dual build și QA final.

## Graf de dependențe (simplificat)

```
009-topbar ─┬─> 010-conversation-sidebar ─┬─> 011-chat-view
            │                             │
            │                             ├──> 012-routines-sidebar-wiring
            │                             │     ├──> 017-routines-list-item-design
            │                             │     │
            │                             │     └──> 015-routine-trace-in-chat
            │                             │
            │                             ├──> 013-agent-chat-composer
            │                             │     │
            │                             │     └──> 014-routine-chips
            │                             │           │
            │                             │           └──> 015-routine-trace-in-chat
            │                             │
            ├─> 016-connectors-page
            ├─> 018-teams-page
            │
            └─> 022-threads-enhancements (parallel cu 012-021)

023-dual-build-qa (depinde de toate celelalte)
```

## Modelul de interfață curent

- **Sidebar stânga conține DOAR rutine.** Fără search, fără listă de agenți. Trebuie să fie resizable.
- **Chat-ul principal este shell-ul agentului.** Utilizatorul scrie în limbaj natural în rich input. Există și comenzi cu slash ca extra.
- **Nu există “conversație nouă”.** Chat-ul este infinit; putem începe o sesiune nouă doar opțional (de ex. `/new`), dar nu e cazul principal.
- **Rutina = subagent.** Abstractizarea internă este subagent, dar în UI ne referim la el ca **rutină**.
- **Click pe o rutină din sidebar deschide rutina în chat.** Chat-ul devine interfața rutinei; se poate închide chip-ul și ne întoarcem la agentul principal.
- **Trace-ul/log-urile unei rutine se văd în acel view de chat**, nu într-o pagină separată.
- **Agentul principal scaffoldează rutine** — creează/modifică rutine prin limbaj natural.
- **Rutinele în derulare** nu apar live în chat-ul principal decât dacă cerem explicit (verifică, modifică, șterge etc.).
