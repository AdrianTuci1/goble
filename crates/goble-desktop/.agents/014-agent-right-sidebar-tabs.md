# 014 — Sidebar dreapta Info/Trace pentru agent/rutină selectată

## Status

[ ] Activ

## Context

Sidebar-ul din dreapta (`ChatSidebar`) afișează acum doar un mock cu "Computer Use" și "Routines". În noua direcție, când utilizatorul selectează un agent sau o rutină în sidebar-ul stânga, panelul din dreapta arată informații despre entitatea selectată. Logs și traces nu sunt pagini standalone, ci intră în tab-ul **Trace** al agentului/rutinei.

## Obiective

1. Refactorizează `ChatSidebar` să accepte un `tab` activ și callback `on_change_tab`.
2. Tab **Info**:
   - Nume, prompt, model/provider, runtime target pentru agent/rutină selectată.
   - Dacă e un agent: tool-urile/MCP-urile activate.
   - Dacă e o rutină: pașii, dependențele, trigger-ul.
3. Tab **Trace**:
   - Listează execuțiile recente ale agentului/rutinei selectate.
   - Click pe o execuție deschide detaliile trace-ului în același panel (pași, log-uri, tool calls, metrici).
4. Propagă din `ChatViewPanel` / `ShellView` datele agentului/rutinei selectate și execuțiile asociate.

## Criterii de acceptare

- Sidebar-ul din dreapta are tab-uri Info/Trace funcționale.
- Info afișează detalii reale despre agent/rutină.
- Trace listează execuțiile și permite drill-down într-un trace.
- Build verde.

## Dependințe

- `011-chat-view.md`
- `015-agents-in-sidebar.md`
- `017-routines-in-sidebar.md`
- `019-routine-trace-panel.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/chat_sidebar.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
- `crates/goble-desktop-native/src/views/agent_trace.rs` (opțional, doar dacă se folosește componentă reutilizabilă)
