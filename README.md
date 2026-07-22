# Goble

Aplicație desktop + worker VPS pentru agenți autonomi. Codul este Rust, UI construit cu gpui (custom/native, la fel ca OctomusUI), și worker livrat ca binar static.

## Cuvânt cheie

- **Goble** — aplicația desktop (clientul).
- **Goblin** — worker-ul care rulează pe VPS (sau mai mulți).
- **Agent** — unitate autonomă creată prin limbaj natural, activată la heartbeat sau HTTP.

## Arhitectură (pe scurt)

```
Goble Desktop  gpui + tokio
       |
   SSH / mTLS / WebSocket
       |
Goblin Worker  axum + runner + scheduler + MCP registry
```

## Repo layout

- `.agents/` — documentație de planificare și context pentru fiecare subsistem.
- `crates/goble-core/` — tipuri partajate, protocol desktop↔worker, crypto, LLM abstractions.
- `crates/goble-ui/` — componente UI comune (widget-uri, teme, tokeni de design).
- `crates/goble-desktop/` — aplicația desktop (gpui window, state, chat, views).
- `crates/goblin-worker/` — binarul worker (server axum, runner, observability).
- `crates/goble-cli/` — CLI utilitar pentru worker.
- `tests/` — teste end-to-end între desktop și worker.
- `scripts/` — scripturi de build, deploy, audit.

## Dezvoltare

1. Citeste `.agents/` în ordine numerică.
2. Fiecare crate are `#[cfg(test)] mod tests;` și target de coverage 100%.
3. Testele rulează cu `cargo test` din root.

## Licență

MIT — vezi `LICENSE`.
