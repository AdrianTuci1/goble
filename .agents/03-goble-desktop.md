# Agent 03 — goble-desktop

## Responsabilitate
`goble-desktop` este aplicația desktop nativă. Folosește wgpu + winit pentru UI custom, tokio pentru async, și `goble-core` pentru logică.

## Module
- `main` — entrypoint, logging, panic hook, config paths, winit event loop.
- `app` — `App` global, `Window`, `Renderer` din goble-ui, `GlobalStore`.
- `state` — `GobleState`: agenți, chat history, workeri, teams, config.
- `chat` — modelul de chat, mesaje, tool calls, streaming LLM.
- `views` — ferestre/view-uri:
  - `chat_view` — thread-uri de chat.
  - `agents_view` — listă, creare, editare agenți.
  - `workers_view` — workeri, status, adăugare SSH.
  - `teams_view` — echopi (gestionat manual).
  - `execution_view` — execution trace secvențial.
  - `settings_view` — LLM providers, theme, shortcuts.
- `worker` — manager de conexiuni worker: SSH, pairing, WebSocket, SCP.
- `mcp` — registry browser, composer custom, local store.
- `llm` — client multi-provider, streaming, caching.
- `ui` — pune laolaltă componente din `goble-ui`.

## Fluxuri UI
- La pornire: splash → onboarding (LLM + prim worker) → chat principal.
- Sidebar stânga: istoric chat, agenți, workeri, echipe, settings.
- Panou principal: chat activ sau view selectat.
- Creare agent: chat → spec generat → wizard → salvare + deploy.
- MCP composer: registry → select → UI dinamic pentru credențiale → salvează local → trimite workerului.
- Observability: execuție secvențială, logs colorate, health, metrics.

## Design tokens
- Fără shadows, flat, monocrom + accent teal.
- Font: system native, fallback Inter.
- Borduri subtile, 1px, radius 6px.
- Sidebar 260px, chat input 56px.

## Test coverage
- Unit tests pentru state, chat model, worker manager.
- Test headless cu renderer mock / offscreen.
- Mock LLM provider.
- Fără `#[ignore]`.
