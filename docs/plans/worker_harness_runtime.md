# Plan: Worker runtime complet cu Harness

## Obiectiv
Înlocuim `AgentRuntime` simplu din `goblin-worker` cu `Harness` din `goble-core` pentru a rula agenți autonomi cu reasoning, MCP, tools, shell, persistent state. Odată ce worker-ul/cluster-ul există, desktopul nu mai folosește LLM local ci pe cel din worker.

## Context
- `goblin-worker` rulează un `AgentRuntime` minimal (`src/agent_runtime/`) cu doar 8 tool-uri de bază (read_file, edit_file, finish etc.).
- `goble-core` are deja un `Harness` avansat (`src/harness.rs`, `src/reasoning.rs`) cu reasoning, mission, MCP, store, tool-uri complete.
- Desktop Tauri folosește `Harness` local și doar trimite task-uri worker-ului pentru execuție simplă.

## Modificări necesare

### 1. Protocol worker-desktop
- `DesktopMessage::RunAgent` are deja `spec` și `mcp_servers`.
- `ScheduleAgent` are deja `trigger` și `mcp_servers`.
- Trebuie adăugate mesaje pentru a cere/cancela reasoning pause (AskUser) din worker.
- Nu este necesar să adăugăm un nou mesaj: `HarnessEvent::AskUser` poate fi serializat ca `WorkerMessage` nou.

Adăugăm în `WorkerMessage`:
```rust
AskUser {
    trace_id: String,
    question: String,
    quick_replies: Vec<String>,
},
AssistantDelta {
    trace_id: String,
    delta: String,
},
ToolCallStarted { ... },
ToolCallFinished { ... },
ToolCallError { ... },
MissionUpdated { ... },
Done { trace_id: String },
```

### 2. Worker: conectarea la LLM real
- `Runner` din worker citește `LLM_PROVIDER` și secretul `llm_api_key`.
- Trebuie să permită și setarea LLM prin `PushSecrets` / `SetVaultSecret` + `LLM_PROVIDER`.
- După plan, adăugăm un mesaj `SetLlmSetting` similar cu cel din desktop, sau folosim `PushSecrets` cu `llm_api_key` și variabilă env `LLM_PROVIDER`.

### 3. Worker: integrare Harness în loc de AgentRuntime
- Creăm `src/harness_runner.rs` care primește `AgentSpec`, MCP servers, secrets, chat_id/trace_id și pornește un `Harness::run_turn`.
- `Harness` are nevoie de `Store`, `McpManager`, `LlmProvider`, `CommandRunner`.
- Store worker va fi un fișier SQLite în workspace root (e.g. `/var/goblin/worker.db`).
- MCP servers se populează în `McpManager` din `mcp_servers` trimise de desktop.
- Secrets se pun în store ca vault secrets.
- `CommandRunner` va fi `SandboxedCommandRunner` cu allowed commands configurate din worker config.
- `deploy_sender` va fi un sender care, la tool call `deploy_agent`, trimite `DesktopMessage` către desktop prin WebSocket invers? Sau, pentru MVP, nu permitem deploy din worker înapoi (returnăm eroare explicativă).

### 4. ScheduleAgent implementat
- În `websocket.rs`, handlerul `ScheduleAgent` va salva task în `TaskStore` via `Scheduler::schedule`.
- `Scheduler` trebuie să pornească `AgentRuntime` / `HarnessRunner` pentru task-uri programate.
- Refactorizăm `Scheduler` să primească un `Runner` generic sau direct `fn` care rulează agent.

### 5. Vault secret implementat
- `SetVaultSecret` salvează în `CredentialVault` și persistă pe disk.
- `GetVaultSecret` citește și emite `VaultSecret`.

### 6. List/Cancel scheduled tasks
- `ListScheduledTasks` citește din `TaskStore` și emite `ScheduledTasks` cu summary-uri.
- `CancelScheduledTask` apelează `Scheduler::cancel_task`.

### 7. Teste
- `test_worker_run_agent_flow` trebuie să treacă. Eșuează acum pentru că worker-ul nu emite `AgentFinished`? Verificăm: `AgentRuntime` emite `AgentFinished` la final. Deci problema e altundeva, probabil pentru că `default_provider_factory` cere `llm_api_key` real sau testul folosește worker fără factory mock.
  - Refactorizăm `Runner` să accepte `ProviderFactory` și în teste să folosească `MockProvider`.
  - Verificăm de ce nu ajunge `AgentFinished` la client în test.
- Adăugăm test pentru `ScheduleAgent` persistence.
- Adăugăm test pentru `SetVaultSecret` / `GetVaultSecret` roundtrip.
- Adăugăm test pentru harness runner cu mock provider.

### 8. Migrare pas cu pas (ordine implementare)
1. Extindem `WorkerMessage` cu event-uri de harness streaming.
2. Refactorizăm `Runner` să accepte `ProviderFactory` în mod implicit (default_factory real, test factory mock).
3. Implementăm `SetVaultSecret`/`GetVaultSecret` în worker.
4. Implementăm `ListScheduledTasks`/`CancelScheduledTask`.
5. Implementăm `ScheduleAgent` real cu `TaskStore`.
6. Creăm `HarnessRunner` și-l înlocuim pe `AgentRuntime` în `Runner`.
7. Adăugăm `Store` în `AppState` pentru worker.
8. Populăm `McpManager` și vault secrets înainte de rulare.
9. Actualizăm testele și ne asigurăm că trec.
10. Build final și test `cargo test --workspace` + `npm run tauri build`.

## Criterii de acceptare
- `cargo test --workspace` trece complet.
- `npm run tauri build` merge.
- Worker-ul poate rula un agent cu mock provider end-to-end și emite `AgentFinished`.
- `ScheduleAgent` salvează task în SQLite; `ListScheduledTasks` îl returnează; `CancelScheduledTask` îl șterge.
- `SetVaultSecret`/`GetVaultSecret` persistă și returnează secret.
- Harness runner este folosit pentru execuție în loc de `AgentRuntime`.

## Riscuri
- `Harness` depinde de `Store` care are schema mare; worker-ul are `TaskStore` separat. Vom unifica prin folosirea `Store` ca worker state DB.
- `Harness` are tool-uri care se așteaptă la desktop (e.g. `deploy_agent`). Pe worker, `deploy_sender` va fi `None` și tool-ul va returna eroare clară.
- `complete_stream` trebuie implementat pentru `MockProvider` în teste; altfel harness nu va funcționa.
