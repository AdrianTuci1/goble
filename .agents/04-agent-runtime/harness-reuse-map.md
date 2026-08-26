# 04 — grok-build harness reuse map

**Status:** `[x]` mapping surveyed
**Owns:** which grok-build modules we reuse for which Goble subsystem
**Depends on:** [`README.md`](README.md)

**Source:** `~/Projects/grok-build`, a SpaceXAI Rust monorepo (`crates/*`, `crates/codegen`, `crates/common`). Its agent runtime is modular and high-standard; we reuse it behind our own renderer. Crates below are under `crates/codegen/` unless noted.

## The map

| grok-build crate | Reuse for |
| --- | --- |
| `xai-grok-agent` (builder, config, system prompt, compaction, discovery, plugins/prompt) | agent definition/builder, system-prompt assembly |
| `xai-agent-lifecycle` | agent lifecycle / state machine |
| `xai-grok-tools`, `xai-grok-tools-api`, `xai-tool-runtime`, `xai-tool-types`, `xai-tool-protocol` | tool registry + runtime + protocol |
| `xai-grok-mcp` (rmcp quarantine, credential store, OAuth) | MCP integration + credential store + OAuth flow |
| `xai-grok-sandbox` (Landlock/Seatbelt via `nono`) | OS-level sandboxing (per-agent isolation) |
| `xai-grok-shell`, `xai-grok-shell-base`, `xai-grok-shell-session-support`, `xai-grok-shell-terminal`, `ptyctl`, `xai-grok-pager-pty-harness` | PTY terminal + session; **remote terminal capture** for the renderer |
| `xai-grok-sampler`, `xai-grok-sampling-types`, `xai-grok-models` | LLM sampling loop + model config |
| `xai-grok-memory` (embed, mmr, dream, search) | memory / `remember` |
| `xai-grok-secrets` | outbound secret sanitizer on logs/events |
| `xai-grok-config`, `xai-grok-config-types` (layering: requirements > user > managed, TOML merge) | the **agent-editable TOML** (`GobleConfig`) layering |
| `xai-grok-workspace`, `-client`, `-daemon`, `-types` | workspace daemon (local/remote) — a `Router` target |
| `xai-grok-subagent-resolution` | sub-agent definition/runtime/prompt/resume |
| `xai-grok-compaction`, `xai-compaction-transcript` | transcript compaction ("to infinity") |
| `xai-grok-plugin-marketplace`, `xai-hooks-plugins-types` | plugins (skills + MCP servers) |
| `xai-grok-markdown`, `xai-grok-markdown-core`, `xai-grok-pager-render`, `xai-grok-pager-diff` | content rendering (markdown/diff) we can lean on inside our renderer |
| `xai-grok-hooks`, `xai-workflow`, `xai-grok-session-events`, `xai-grok-codebase-graph`, `xai-grok-voice` | hooks, workflows, session events, codebase graph, voice |

## Reuse strategies per area

- **Direct dependency (port into `goble-core`):** config/model/tool-protocol/data-shape crates with no OS/UI coupling — e.g. `xai-grok-config-types`, `xai-tool-types`, `xai-grok-sampling-types`, `xai-grok-mcp` wire types.
- **Invoke as a crate if allowed / vendor the module:** `xai-grok-sandbox`, `xai-grok-shell-*`, `xai-grok-memory` — these pull heavier deps (landlock/seatbelt, LMDB, etc.); decide per-module.
- **Reference implementation only:** the TUI pager/ratatui crates — we do **not** reuse the UI layer, only the content-rendering logic.
- **Inspect for porting:** `xai-grok-agent` prompt assembly + compaction are prime port candidates.

## Gaps to fill ourselves

- The **chat renderer** (ours: `goble-ui`/`goble-ui-hot` on `wgpu`) — see [`../06-renderer/README.md`](../06-renderer/README.md).
- **Workspace-as-a-package** that ships to a remote host (grok-build has a workspace daemon, but we need a remote *bootstrap* that self-configures from the TOML) — see [`../02-first-run-and-routing/remote-bootstrap.md`](../02-first-run-and-routing/remote-bootstrap.md).
- The **routing / local↔remote promotion** and the **worker-as-workspace** story.

## Licensing — resolved

Both `~/Projects/grok-build` and `~/Projects/warp-new` are **open source; we treat them as MIT**. Modules, crates, or code chunks can be **copied wholesale** and re-licensed under our own name — we do **not** need to rewrite them. So the porting strategy is "copy the module cleanly and adapt its interfaces", not "reimplement".

## Open question

- How much of `xai-grok-sandbox` is portable to `goblin-worker`'s current Linux-first isolation (module paths differ; the sandbox is OS-specific — Landlock/Seatbelt/nono).
