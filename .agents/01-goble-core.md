# Agent 01 — goble-core

## Responsabilitate
`goble-core` este crate-ul partajat între desktop și worker. Nu depinde de UI sau de rețea în mod direct (doar de primitive). Conține:
- Tipuri de date (agent, worker, trigger, task, secret).
- Protocolul de comunicare între Goble și Goblin.
- Crypto (pairing, criptare secrets, hashing).
- LLM abstractions.
- Config serializabilă (TOML/JSON).

## Module planificate
- `agent` — `AgentId`, `AgentSpec`, `AgentState`, `Trigger`.
- `worker` — `WorkerId`, `WorkerConfig`, `WorkerStatus`.
- `secret` — `SecretId`, `SecretValue`, `SecretStore` (trait local).
- `protocol` — `DesktopMessage`, `WorkerMessage`, `Envelope`.
- `crypto` — `PairingKey`, `Cipher`, `KeyDerivation`, `Nonce`.
- `llm` — `LlmProvider`, `Message`, `CompletionRequest`, `ToolCall`.
- `execution` — `ExecutionTrace`, `Step`, `LogLevel`, `Metric`.
- `config` — serializare/deserializare `GobleConfig`.

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
