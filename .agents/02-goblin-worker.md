# Agent 02 — goblin-worker

## Responsabilitate
`goblin-worker` este binarul care rulează pe VPS. Primește config de la Goble, execută agenți, rulează MCP-uri, raportează observability.

## Module
- `server` — axum, rute, WebSocket upgrade.
- `api` — request/response types, handlers.
- `pairing` — verificare pairing code, derive key.
- `crypto` — decriptare secrets, verificare envelope.
- `state` — `AppState` (Arc), worker config, task queue, metrics.
- `runner` — execută task: V8 Isolate, subprocess utilitar, MCP.
- `isolate` — manager V8 Isolate: compile, start, comunicare, limită resurse.
- `scheduler` — cron + HTTP triggers.
- `mcp` — descărcare MCP din registry, configurare, pornire în isolate.
- `registry` — catalog local, fetch de la registry-uri remote (GitHub, npm, custom).
- `observability` — logs, execution trace, metrics, health.
- `cli` — argumente clap, entrypoint.

## Sandbox V8 Isolate
- Agenții și MCP-urile sunt scripturi JavaScript / TypeScript compilate la un bundle.
- Goblin pornește un V8 Isolate per agent/MCP cu limite: CPU time, memorie, heap, network outbound prin proxy.
- Isolate-urile nu au access direct la sistemul de fișiere, doar la un director temporar montat.
- Comunicarea între worker și isolate se face prin JSON-RPC over stdio sau shared memory.
- Pentru MCP-uri care necesită binare native, workerul poate lansa un subprocess sandboxat (fără container).

## Protocol desktop ↔ worker
- Pairing: code numeric + PBKDF2/AES-GCM.
- După pairing: WebSocket binar cu mesaje JSON, semnate și opțional criptate.
- Heartbeat: worker trimite `StatusReport` la 5s.
- Trigger manual: `RunAgent`.
- Trigger programat: `ScheduleAgent`.
- Răspuns: `AgentFinished`, `AgentLog`, `MetricReport`.

## Securitate
- Pairing code generat de desktop, introdus pe worker la prima pornire.
- Credentialele vin criptate; workerul le decriptează în memorie și le șterge după folosire.
- Isolate-urile rulează fără env global, cu access control la resurse.
- Network outbound din isolate este proxiat și logat.
- Subprocese native doar pentru MCP-uri în whitelist, cu timeout și limită de resurse.

## Test coverage
- Unit tests pentru fiecare handler.
- Teste de integrare cu `axum::TestServer`.
- Mock SSH / mock MCP.
- Fără `#[ignore]`.
