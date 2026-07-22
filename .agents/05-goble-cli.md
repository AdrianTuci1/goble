# Agent 05 — goble-cli

## Responsabilitate
CLI utilitar pentru administrarea workerilor și debugging.

## Comenzi planificate
- `goble-cli worker init <host>` — configurează worker nou prin SSH.
- `goble-cli worker list` — workeri cunoscuți.
- `goble-cli worker logs <id>` — tail logs.
- `goble-cli worker run <id> <agent>` — rulează agent manual.
- `goble-cli agent list` — agenți locali.
- `goble-cli agent deploy <id>` — deploy agent la workeri.
- `goble-cli mcp search <term>` — caută în registry.
- `goble-cli mcp install <name>` — instalează MCP local.
- `goble-cli config` — afișează config local.

## Implementare
- `clap` pentru parse.
- Împarte cod cu `goble-core` și `goblin-worker` (ca lib).
- Autentificare prin cheie locală de pairing.
