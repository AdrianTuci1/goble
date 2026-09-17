# 01 — Remote direction: what the user gets

**Status:** `[~]` planned — nothing below is built yet
**Owns:** the user-visible feature list of the remote direction. No code.
**Depends on:** [`../02-first-run-and-routing/remote-bootstrap.md`](../02-first-run-and-routing/remote-bootstrap.md), [`../04-agent-runtime/computer-use.md`](../04-agent-runtime/computer-use.md), [`../05-execution-router-and-targets/runtime-targets.md`](../05-execution-router-and-targets/runtime-targets.md), [`../06-renderer/remote-terminal-renderer.md`](../06-renderer/remote-terminal-renderer.md)
**Delivered by:** [`.grok/workflows/remote-rollout.rhai`](../../.grok/workflows/remote-rollout.rhai), one item at a time

Each feature names the item that delivers it. The item's row in [`../TRACKER.md`](../TRACKER.md) carries the acceptance criterion and the command that proves it; the owning doc above carries the reasoning.

## 1. A remote turn actually runs

**The keys reach the worker** (`K1`). A conversation routed to a paired worker resolves a model and answers, instead of failing with no key. The keys already configured in the local `~/.goble/config.toml` are sent to that worker after it confirms the pairing, are re-sent when the client reconnects, and a change to the local config reaches a worker that is already connected. Only the keys the resolved models need are sent — not the vault. No key value is ever logged, drawn, or written into a transcript.

## 2. Computer use with one driver at a time

- **The agent cannot fight you for the mouse** (`C1`). A remote desktop opened by an agent starts view-only; the agent can look but not click or type. Input starts working only when control is taken, stops when it is released, and two holders cannot both hold it. An agent whose input is refused gets a clear error, and the agent can ask for the desktop back.
- **A desktop can be closed** (`C2`). Dismissing the card in the conversation ends the desktop: the local client thread stops, the frames stop, and the remote `xrdp` session does not live on after the work that asked for it. Closing one card never kills a stream another conversation is still watching.
- **The desktop password is stored once, not asked of the model** (`C3`). The agent names a stored credential; it is resolved on the host at the moment the connection is built. A password never appears in a tool call, an event payload, a log line, or a transcript, and a missing credential fails loudly instead of connecting without one.
- **The desktop appears as a card, not a window thrown over the app** (`C4`). An agent's handoff draws the desktop inside the conversation, named with its host and showing who is currently driving, with a close control. The app window is not thrown open by an agent's tool call. Clicking a screen link still opens the full screen sheet.
- **One desktop per host, found again** (`C5`). A second handoff for the same host reuses the desktop already open instead of connecting a second time, and after a reconnect the client finds the desktop it had rather than starting a new one. A desktop that is gone reports that, instead of showing a stale frame.

## 3. The SSH panel

- **Typing an `ssh` command binds the session to its host** (`S1`). An interactive `ssh host`, `ssh user@host`, `ssh -p 2222 host`, or an alias from `~/.ssh/config` binds the session with the right host, user and port. `scp`, `sftp`, `ssh -V`, and a command that merely mentions ssh are not sessions.
- **The session says where it is** (`S2`). A bound session draws a chip naming `user@host` above the composer. A local session draws none, and the chip disappears when the binding is cleared.
- **A plain pane stays a plain terminal** (`S4`). A shell pane is exactly a shell until you ask otherwise: no agent transcript, no agent chrome, no agent round trip. An SSH-bound pane in terminal mode behaves the same way.
- **`Cmd+Enter` enters terminal + agent mode on the same session** (`S3`). The pane becomes an agent session without restarting the shell and without dropping the SSH connection: same process, same identity, and everything the shell already printed is still there. Leaving agent mode does not kill the SSH session either. `Cmd+Enter` in agent mode does not push a second mode, and a long-running command may refuse the switch but must say so rather than doing nothing.

## 4. The cloud agent

- **A viewer pane with nothing local behind it** (`A1`). A conversation routed to a worker gets a pane fed entirely by the remote stream — no local shell. It distinguishes and shows connecting, attached, detached with a way back, and failed, and a dropped connection never silently becomes a local shell.
- **Re-attach instead of starting over** (`A2`). This is the reason the direction exists: the run lives on the worker, so closing the laptop or letting it sleep does not lose it. Reopening the pane — or recovering a dropped connection — rejoins the session that is still running and replays what was missed, from the session's own transcript. A conversation with no live session says so rather than quietly starting a fresh one.
- **Environment and model chosen where you send** (`A3`). A remote conversation picks which medium the work runs on and which model it uses, at submit time, and remembers the last choice per conversation. A local conversation shows neither control. The model control ships only if a per-turn model can honestly reach the worker; otherwise it stays `[~]` with the missing path recorded rather than faked.

## 5. Testability (wave 0)

These come first because every item above is otherwise provable only by compilation.

- **The worker's end-to-end tests run** (`T1`). The existing `#[ignore]`d tests stop depending on a hardcoded Linux path (they build and locate the worker binary through cargo) and run in the normal suite.
- **Continuous integration covers the worker** (`T2`). A CI job runs the worker's tests on every relevant change; today the only job is the terminal conformance suite.
- **The install script is verified on a real systemd host** (`T3`). A container with systemd as PID 1 runs the generated install script and confirms the service starts and answers on `/health`.
- **A real RDP connection is verified** (`T4`). A container running xrdp exercises `RdpRemoteSource::connect` end to end and confirms at least one frame is decoded. This is the largest piece of the direction that nothing currently proves.

## What this explicitly does not give

- **No GUI of ours on the host.** The worker binary is the whole remote payload; the local client stays the GUI.
- **No container runtime, language runtime, or third-party agent framework on the host.** The install carries none of them; the agent installs what a specific project needs.
- **The SSH panel is a terminal, not an agent browser.** It stays terminal-only until `Cmd+Enter`.

## What no run in this environment can settle

- There is no provisioned host, no paired worker and no network peer: an RDP session actually opening and streaming, a key arriving at a running worker, or a reconnect after a real drop are proven by tests over the protocol, the registry and the app state.
- The pixels: building or running the GUI binary is forbidden here, so the session chip, the desktop card and the mode switch are proven by drawn commands, dispatched events and the state they leave — never seen.
- The mode switch against a real interactive shell and a real SSH connection: the PTY identity before and after is asserted, not observed against a live host.

## Related

- [`README.md`](README.md) — the product goals these features serve
- [`../TRACKER.md`](../TRACKER.md) — the acceptance criterion and proof command per item
