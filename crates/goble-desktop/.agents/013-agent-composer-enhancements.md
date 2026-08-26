# 013 — Composer-ul principal devine creator/editor de agenți

## Status

[ ] Activ

## Context

În noua direcție, view-ul principal nu mai creează o conversație, ci un **agent**. Composer-ul devine un editor de agent: utilizatorul definește prompt-ul, modelul/provider-ul, target-ul de execuție, tool-urile/MCP-urile disponibile și alte setări. Salvarea creează sau actualizează agentul în `DesktopState`.

## Obiective

1. Transformă `ChatComposer` într-un editor de agent:
   - Câmp principal pentru **prompt / instrucțiuni** (`TextArea`).
   - Selector **model/provider** (apelând `set_chat_model` / `get_llm_setting`).
   - Selector **runtime target** (`auto`, `local`, `tag`, `worker`).
   - Selecție **tools / MCP connectors** (lista tool-urilor disponibile, toggle enable).
   - Câmp opțional pentru variant/temperature.
   - Câmp pentru numele agentului.
2. Implementează `Enter` pentru acțiunea principală (salvare/rulare) și `Shift+Enter` pentru newline în `TextArea`.
3. Detectează când modelul/provider-ul nu este configurat și afișează un card inline pentru introducerea cheii API (similar `ApiKeyCard` din React).
4. Buton de **Save** care creează sau actualizează agentul via `state_api`.
5. Buton de **Run** care, după salvare, pornește o execuție a agentului/rutinei selectate.

## Criterii de acceptare

- Utilizatorul poate defini un agent: nume, prompt, model, target, tools.
- Enter confirmă/ salvează; Shift+Enter adaugă o linie nouă în prompt.
- Cardul de API key apare când nu există cheie/model.
- Agentul nou apare în sidebar după salvare.
- Build-ul rămâne verde.

## Dependențe

- `011-chat-view.md`
- `012-conversation-sidebar-wiring.md`
- `015-agents-in-sidebar.md`

## Fișiere afectate

- `crates/goble-ui/src/elements/chat_composer.rs`
- `crates/goble-ui/src/elements/text_area.rs`
- `crates/goble-ui/src/elements/agent_card.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
- `crates/goble-desktop-native/src/state_api.rs`
