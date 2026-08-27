# Resolver — live work registry

**Status:** `[x]` mechanism adopted
**Owns:** the single item being resolved **right now**, and its proof.
**Depends on:** [`TRACKER.md`](TRACKER.md) (the full backlog), [`GUIDE.md`](GUIDE.md) (how to execute).

`TRACKER.md` is the complete backlog. **This file is the living side of the plan**: it records exactly one item in flight, its plan, and the verification that proves it is done. Its whole purpose is that neither we nor the model ever lose sight of what is being resolved — so nothing gets silently dropped and nothing is marked done without evidence.

## How the model uses this (each turn)

1. Pick **one** item from `TRACKER.md` — the smallest fully-defined, unblocked item.
2. Copy it into **Active work** below; fill in plan, files, and definition of done.
3. Implement it. Then **verify** with a real command (cargo / tsc / npm) or, for UI, the browser. Record the exact verification.
4. Flip the item to `[x]` in `TRACKER.md`, move it to **Recently resolved** here, clear **Active work**, and repeat.
5. If an item is too big to implement + verify in one turn, split it into smaller items **in its owning doc first** — do not attempt "the whole system".

## Active work

- **Item:** — *(id + link to the owning doc)*
- **Status:** `[~]` in progress
- **Definition of done:** — *(what observable behavior proves this is complete)*
- **Files touched:** — *(concrete paths)*
- **Verification run:** — *(exact command / browser steps + result)*
- **Blockers:** none

## Recently resolved

| Date | Item | Verification (what passed) |
| --- | --- | --- |
| 2026-08-27 | Align local + remote on a single runner core (`02-first-run-and-routing/router-local-vs-remote.md`, `04-agent-runtime/sandbox-and-cwd.md`): local agent runs now route through `goblin_worker::Runner` in-process (shared store via `AppState::set_store`, event bridge `event_tx` → `DesktopState::handle_worker_message`, knobs via `HarnessOptions`), so local and remote share memory/MCP/workspace/compaction semantics. `AppState::set_store` + `HarnessOptions` added; `drain_agent_stream` removed. | `cargo check --workspace` passed; `cargo test -p goble-desktop-service` passed (24, incl. `test_local_runtime_target_runs_agent_in_process`); `cargo test -p goblin-worker --lib` passed (32, incl. `runner::test_run_agent_success`/`test_run_team_success`); `cargo test -p goblin-worker --test agent_runtime_integration` passed (2); `cargo test -p goble-cli --test e2e_worker` passed |
| 2026-08-26 | Fix hardcoded `/var/goblin/workspaces` in `goblin-worker` (`04-agent-runtime/sandbox-and-cwd.md`) | `cargo test -p goblin-worker` lib + integration passed; `cargo test -p goble-cli --test e2e_worker` passed |
| 2026-08-27 | Make `local` a valid runtime target: `resolve_worker_for_target("local")` returns the `local` sentinel and `run_agent`/`run_agent_for_thread_reply` run in-process via the harness (`02-first-run-and-routing/router-local-vs-remote.md`) | `cargo test -p goble-desktop-service` passed (24 tests, incl. new `test_local_runtime_target_runs_agent_in_process`) |

## Recently resolved

| Date | Item | Verification (what passed) |
| --- | --- | --- |
| *(yyyy-mm-dd)* | *(id + doc)* | *(command / test result)* |

## Rule: no `[x]` without proof

An item is only `[x]` in `TRACKER.md` when this file records the command / test / browser check that actually passed. If something could not be verified, it stays `[~]` and the reason is written here — never silently dropped.

## Anti-laziness contract (also in [`GUIDE.md`](GUIDE.md))

- One small item per turn; stop when it is verified.
- Read the owning doc + the real code before writing.
- No placeholder comments; implement fully or don't touch.
- State plainly what was **not** verified and why.
