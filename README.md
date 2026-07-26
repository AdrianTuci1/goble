# Goble

Aplicație desktop + worker VPS pentru agenți autonomi. Desktop-ul este construit cu Tauri (Rust backend + React frontend), iar worker-ul este un binar Rust care rulează pe VPS.

## Cuvinte cheie

- **Goble** — aplicația desktop (Tauri + React).
- **Goblin** — worker-ul care rulează pe VPS (sau mai mulți).
- **Agent** — unitate autonomă creată prin limbaj natural, activată la heartbeat, cron sau HTTP.
- **MCP** — Model Context Protocol: conectează servere externe de tool-uri (search, install, discover, execute).

## Arhitectură (pe scurt)

```
Goble Desktop  Tauri (Rust + React)
       |
   WebSocket / mTLS / SSH
       |
Goblin Worker  axum + runner + scheduler + MCP registry
```

## Repo layout

- `crates/goble-core/` — tipuri partajate, protocol desktop↔worker, crypto, LLM abstractions, MCP manager, registry.
- `crates/goble-desktop/` — aplicația desktop Tauri (Rust commands + React UI).
- `crates/goblin-worker/` — binarul worker (server axum, runner, scheduler, task store).
- `crates/goble-cli/` — CLI utilitar pentru worker.
- `tests/` — teste end-to-end între desktop și worker.
- `scripts/` — scripturi de build, deploy, audit.

## Dezvoltare

```bash
cd /root/goble
cargo fmt --all
cargo check --workspace --all-targets
cargo test --workspace
```

Pentru frontend:

```bash
cd /root/goble/crates/goble-desktop
npm test
npm run build
```

Pentru release desktop (Tauri bundle):

```bash
cd /root/goble/crates/goble-desktop
npm run tauri build
```

## Licență

MIT — vezi `LICENSE`.
