# 04 — Computer use

**Status:** `[x]` removed — the direction is not part of Goble
**Owns:** the decision record for the interaction surface the agent uses. No code.
**Depends on:** [`README.md`](README.md), [`../01-vision/README.md`](../01-vision/README.md), [`../06-renderer/agent-terminal-bridge.md`](../06-renderer/agent-terminal-bridge.md)

## The decision

Goble is a **terminal window manager with the harness integrated**. The agent's interaction surface
is the user's own shell: a pane draws real command blocks from OSC 133/DCS hooks, an agent command is
claimed through the harness claim protocol and written to that pane's PTY, and its transcript, tool
calls and sub-agents sit beside the prompt that produced them. Panes can be local, bound to an SSH
session, or fed by a remote worker; execution stays reversible through `goble-replay` (fork, replay,
revert).

The earlier computer-use direction — a streamed remote graphical desktop that an agent could drive —
was the wrong product. Its capture/source crates, the harness tool that requested a desktop, the
event and worker handoff path, the desktop service's open/close surface, the install step and the
in-app card have all been removed. The terminal is the one interaction surface, and the SSH panel,
worker/viewer panes, remote daemon, voice, projects and crons remain.

## What this means for the agent

- **Its commands run where the user's do.** The harness claims a block on the pane's own shell through
  the claim protocol; the command and its output are the same `TerminalBlock` the user sees, and the
  result returns as a tool result.
- **A pane is one session.** An SSH-bound pane keeps its shell process across a switch into agent
  mode; a worker conversation is a viewer pane fed by the remote stream. The agent does not get a
  second desktop or a separate execution context.
- **Reversibility is unchanged.** Fork, replay and revert continue to operate over the per-turn
  transcript ledger in `goble-replay`.

## Related

- [`../06-renderer/agent-terminal-bridge.md`](../06-renderer/agent-terminal-bridge.md) — the claim protocol and command blocks
- [`../01-vision/remote-features.md`](../01-vision/remote-features.md) — the SSH panel, worker panes and the remote daemon
- [`README.md`](README.md) — the harness surface index
