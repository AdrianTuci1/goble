# Agent 07 — Roadmap

## Faza 0 — Scaffolding ✅
- Repo nou, licență MIT, git, workspace Cargo.
- Documentație `.agents/`.
- Crate-uri goale: `goble-core`, `goble-ui`, `goble-desktop`, `goblin-worker`, `goble-cli`.

## Faza 1 — goble-core (fără UI, doar teste)
- Tipuri: agent, worker, secret, trigger, execution trace.
- Protocol: envelope, mesaje desktop/worker.
- Crypto: pairing, criptare, hash.
- LLM abstraction + mock provider.
- Config TOML.
- **Target:** 100% coverage, `cargo test` verde.

## Faza 2 — goblin-worker (server + runner)
- axum server + handlers.
- pairing + crypto.
- task runner subprocess.
- scheduler cron + HTTP triggers.
- registry MCP (mock la început).
- observability: logs, health, metrics.
- **Target:** coverage, test server funcțional.

## Faza 3 — goble-ui (componente, fără logică de business)
- Design tokens.
- Renderer wgpu: setup, pipeline, clear, resize.
- Primitive 2D.
- Componente de bază: button, input, card, list, chat bubble, sidebar.
- Mascote Goble/Goblin.
- **Target:** componentele compilează și au unit tests.

## Faza 4 — goble-desktop (fără client UI final)
- State management.
- Worker manager (SSH, pairing, WebSocket).
- Chat model + LLM streaming.
- MCP composer.
- **Target:** toate fluxurile testate, fără UI headless dacă gpui nu permite.

## Faza 5 — goble-cli
- Comenzi de worker/agent/MCP.
- **Target:** CLI funcțional în teste.

## Faza 6 — E2E și polisare
- Desktop + worker pe același host.
- Creare agent din chat, deploy, run, observability.
- README, docs, release workflow.

## Faza 7 — Client UI final
- Abia acum construim ferestrele finale în wgpu și pornim aplicația.
