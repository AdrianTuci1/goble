# 014 — Chat right sidebar cu Info/History

## Status

[ ] Activ

## Context

Sidebar-ul din dreapta al chat-ului (`ChatSidebar`) afișează acum doar un mock cu "Computer Use" și "Routines". Aplicația Tauri are două tab-uri: **Info** (detalii conversație, model, agent/flow selectat) și **History** (execuții recente).

## Obiective

1. Refactorizează `ChatSidebar` să accepte un `tab` activ și callback `on_change_tab`.
2. Tab **Info**:
   - Titlul și modelul conversației active.
   - Dacă este selectat un agent/flow, afișează detalii (prompt, tools, schedule).
3. Tab **History**:
   - Listează execuțiile recente pentru conversația/agentul selectat.
   - Click pe o execuție deschide pagina de trace (`019`).
4. Propagă datele necesare din `ChatViewPanel` (agent/flow selectat, execuții).

## Criterii de acceptare

- Sidebar-ul are tab-uri Info/History funcționale.
- Info afișează detalii reale.
- History listează execuțiile.
- Build verde.

## Dependențe

- `011-chat-view.md`
- `019-executions-agenttrace-page.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/chat_sidebar.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
