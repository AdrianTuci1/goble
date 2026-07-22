# Agent 01 — goble-core

## Responsabilitate
`goble-core` este crate-ul partajat între desktop și worker. Nu depinde de UI sau de rețea în mod direct (doar de primitive). Conține:
- Tipuri de date (agent, worker, trigger, task, secret).
- Protocolul de comunicare între Goble și Goblin.
- Crypto (pairing, criptare secrets, hashing).
- LLM abstractions.
- Config serializabilă (TOML/JSON).

## Module planificate
- `agent` — `AgentId`, `AgentSpec`, `AgentState`, `Trigger`, `McpServer`, `McpManifest`, `McpRuntime`, `AuthField`, `Team`, `Chat`, `ChatMessage`, `ToolCall`.
- `worker` — `WorkerId`, `WorkerConfig`, `WorkerStatus`.
- `secret` — `Secret`, `SecretStore` (trait local), `InMemorySecretStore`.
- `secret_manager` — `SecretManager`: encrypt/decrypt + rotate key.
- `protocol` — `DesktopMessage`, `WorkerMessage`, `Envelope`.
- `crypto` — PBKDF2, ChaCha20Poly1305, pairing code, passphrase encryption.
- `llm` — `LlmProvider`, `Message`, `CompletionRequest`, `ToolCall`.
- `execution` — `ExecutionTrace`, `Step`, `LogLevel`, `Metric`.
- `config` — serializare/deserializare `GobleConfig`.
- `store` — SQLite intern pentru agenți, workeri, chat-uri, execuții, MCP-uri, setări.
- `isolate` — `IsolateConfig`, `McpInstance`, `AgentRuntime`, `AgentSource` pentru sandbox V8.

## Decizii de implementare
- SQLite cu `rusqlite` pentru toate datele locale.
- Secrets manager folosește o cheie master pentru a cripta/decripta valori înainte de a le pune în store.
- Protocolul include mesaje pentru MCP servers, update/remove agent, run team.
- Modelul `isolate` descrie runtime V8 multi-tenant: fiecare agent are propriul isolate, dar MCP-urile pot fi partajate ca instanțe separate.

## Reguli
1. Toate structurile publice implementează `serde::Serialize` + `Deserialize`.
2. Fără `unsafe`.
3. Toate funcțiile crypto sunt testate cu vectori de test standard.
4. `SecretValue` nu implementează `Display` sau `Debug` (leak prevention).
5. `AgentId` și `WorkerId` sunt `Uuid` v4 string-ificate, imuabile.

## Test coverage
- Fiecare modul are `mod tests` cu teste pentru parse, roundtrip, erori, edge cases.
- Proptest pentru serializare și crypto.
- Fără `#[ignore]`.
