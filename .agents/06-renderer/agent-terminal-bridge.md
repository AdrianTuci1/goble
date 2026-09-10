# 06 — Agent terminal bridge (the agent + terminal mode)

**Status:** `[ ]` not started — design + build order. Read with [`agent-tui.md`](agent-tui.md) and [`terminal-blocks.md`](terminal-blocks.md).
**Owns:** the seam that turns one pane's **agent + terminal** traffic into blocks: which command ran, who asked for it, what it printed, and which tool call it belongs to.
**Depends on:** [`agent-tui.md`](agent-tui.md), [`terminal-blocks.md`](terminal-blocks.md), [`terminal-emulator.md`](terminal-emulator.md), [`../04-agent-runtime/README.md`](../04-agent-runtime/README.md)

## 1. What "agent + terminal mode" means here

One pane, one shell, two authors. The user types commands and the agent runs commands, and both produce blocks in the same list, in the order they happened. This is the surface `app/src/terminal.rs` already half-models:

- `TerminalMode::{Shell, Agent(TuiAgent)}` (`app/src/terminal.rs:146-150`) — the pane's surface mode.
- `TuiAgent::detect(command)` (`:91-101`) — recognises codex / claude / gemini / opencode / cursor / aider from the first command token, so a pane that hosts somebody else's agent goes native-first.
- `InputClass::{TerminalCommand, AgentPrompt}` with `classify_input` (`:278-320`) — an input is a shell command by default and an agent prompt in agent mode, and `!`-prefixed forces a command from agent mode. That is warp-new's input-line model, already implemented.

What is missing is the **output** half. Today the two authors are not correlated at all: the user's commands are attributed to nobody (`associate_with_conversation` exists in [`blocks.rs`](../../crates/goble-terminal/src/blocks.rs) but nothing calls it), and the agent's commands do not reach the pane at all — they go through the harness's own runner:

```rust
// crates/goble-core/src/harness.rs:167-188
async fn run(&self, command: &str, args: &[String]) -> Result<String> {
    if !self.allowed_commands.contains(command) {
        anyhow::bail!("command `{command}` is not in the allowed list");
    }
```

`SandboxedCommandRunner` spawns its own process, allow-lists nine commands, and times out at 60 s. So an agent-issued command is invisible in the terminal, and its output exists only as a chat row.

The parser is what closes that: **a command run by the agent becomes a real block in the pane's list, and its output is handed back to the harness as the tool result.**

## 2. What is parsed, and what is not

The important scoping fact: **we do not parse the agent's own stream.** The harness is in-process (`crates/goble-core/src/harness.rs`, driven from `DesktopState::run_chat_turn`), so its messages, tool-call status and approval requests are already structured values — no wire format is involved and none needs inventing.

Warp reached the same conclusion from the other direction. Its agent is a library (`warp-new/app/src/pane_group/child_agent.rs:5` — `use octomus_cli::agent::Harness;`), and the reference harness we are reusing has **no in-band framing protocol on the agent's terminal stream**: grok-build's host↔agent contract is out-of-band JSON-RPC/NDJSON over stdio (`grok-build/crates/codegen/xai-acp-lib/src/line_reader.rs`), and its terminal traffic is standard escapes. The nearest precedent for a marker is the shell tool's state channel, and it is deliberately *not* on stdout (`grok-build/crates/codegen/xai-grok-tools/src/computer/local/shell_state.rs:8` — *"State is transported via extra file descriptors (fd 3 for input, fd 4 for output) so that dump traffic never pollutes stdout/stderr"*, sentinels at `:26-33`).

So the parser has exactly one input: **the pane's PTY byte stream**, which we already decode twice for other reasons —

| Tap | Module | What it lifts out |
|---|---|---|
| `HookTap` | `crates/goble-terminal/src/hooks.rs` | the shell-integration channel: a `DCS $d <hex(JSON)> ST` envelope giving command start/finish, exit code, cwd, git branch |
| `OscTap` | `crates/goble-terminal/src/osc.rs` | the OSC numbers the parser discards: `7` (cwd), `133` (prompt markers); pure observer, passes every byte through |

and the parser is the third consumer of the same stream, correlating those two with the app's own knowledge of what it asked for.

## 3. The correlation model

The app knows something the terminal does not: it knows *it* just asked for a command. That is the whole mechanism — **a claim**, not a wire format.

```
app (agent turn)                     pane session                     shell
  run shell tool
    └─ claim(command, call_id)  ──►  write "cmd\r" to the pty
                                     pending_claim = Some(call_id)
                                                          ──►  Preexec hook
                                     attach pending_claim to the active block
                                     clear pending_claim
                                                          ──►  CommandFinished(exit)
                                     block finishes; owner already set
    ◄── tool result: output text + exit code  ────────────
```

Because the shell already reports command boundaries over the hook channel, the parser does not need to detect boundaries — it needs to **attribute** them. Three rules:

1. **A `Preexec` with a pending claim** claims that block: `BlockOwner::Agent { conversation_id, call_id }`.
2. **A `Preexec` with no pending claim** is the user's: `BlockOwner::User`.
3. **A claim that is still pending when a `Preexec` for a different command arrives** — or that outlives a bounded wait — fails the tool call rather than silently attaching to the wrong block.

The alternative, marking the claim in-band (a marker in the command line or an environment variable carrying the call id), is deliberately **not** the first step. It needs shell integration we do not have yet (B0 in [`terminal-blocks.md`](terminal-blocks.md)) and it changes the bytes the user sees in their own history. It is the hardening step (P5), taken only if rule 3 proves insufficient in practice.

## 4. What a block must carry

Today a shell block knows its command, its output, its exit code and its metadata. The parser adds an owner, which is the piece [`agent-tui.md`](agent-tui.md) A1 anticipated:

```
BlockOwner::User                                  — typed by the user
BlockOwner::Agent { conversation_id, call_id }    — run on behalf of a turn
```

That one field is what makes the rest of the surface fall out:

- The agent view shows the blocks whose owner names its conversation (`associate_with_conversation`, A1).
- The tool-call block in the transcript (A4) is the *same* block, keyed by `call_id` — status is the block lifecycle (`BeforeExecution → Executing → Done`), and the output is the block's output screen. There is no second copy of the text.
- The approval composer (A5/A6) is a claim that is only written to the PTY *after* the decision, so a rejected command never becomes a block at all.

## 5. The hard parts

Honest list, because these decide whether the feature feels solid:

- **The sandbox changes meaning.** A command the agent runs in the pane is a command in the **user's own shell**, with the user's environment and credentials — not in `SandboxedCommandRunner` with nine allowed commands and a 60 s timeout. This is the point of the feature (that is how warp behaves, and grok-build has the same shape in `xai-grok-shell-terminal/src/pty_session.rs:1`, *"Agent-scoped interactive PTY manager. PTYs are keyed by `terminalId`, outlive sessions…"*), but it must be a stated trade, not a silent regression. The gates are the per-command approval (A6) and the choice of pane.
- **Shell state is shared.** `cd`, `export`, a virtualenv: a command the agent runs changes the shell the user is in. That is a feature and a hazard. The block's `Precmd` metadata (cwd) is the only feedback the harness gets, so `sandbox-and-cwd.md`'s per-agent cwd story needs re-reading in that light.
- **Interruption.** Ctrl-C gives exit 130; the claim must resolve to a failed tool call, never hang. Same for a command that the shell never runs (a syntax error, or a shell that has not bootstrapped yet) — a claim needs a bounded wait and a definite failure.
- **Background promotion.** A `Precmd` arriving while the claim's command still runs promotes that block to `Background` (`blocks.rs`, `on_precmd`). The tool result becomes "the command is still running" plus whatever output arrived, not a hang.
- **Full-screen programs.** If the agent runs `vim` or `htop`, the alt screen takes over and the block freezes; the tool result has to say an interactive program took the terminal rather than pretend to capture output.
- **`clear`.** It drops earlier blocks from the list (`blocks.rs`, `on_clear`). A claimed block's output must be captured into the tool result before that can matter.
- **Two authors, one line editor.** The user can type while a claimed command is pending; the shell's own editor queues it, and the hook stream still orders the commands correctly. Rule 3 is what keeps a queued user command from stealing the agent's claim.

## 6. Build order

Same convention as the other docs in this folder: each row is one turn of work, small and verifiable on its own.

| # | Item | Where | Verified by |
|---|---|---|---|
| P1 | **Block ownership.** `BlockOwner::{User, Agent{conversation_id, call_id}}`, `Block::owner()`, `BlockList::claim_next_pre_exec(id)` | `crates/goble-terminal/src/blocks.rs` | crate tests: a claimed `Preexec` takes the owner, an unclaimed one is the user's, a claim attached to the wrong block is refused |
| P2 | **The claim protocol.** `TerminalSession::{claim_command, pending_claim}` — write the command to the pty, hold the claim, resolve it on the next `Preexec`, and fail it on a bounded timeout | `app/src/terminal.rs` | app tests with a scripted hook sequence: claim → resolve, claim → timeout, claim → a user command arrives first |
| P3 | **Tool-result extraction.** On `CommandFinished`, produce the output text and exit code for a claimed block and hand them back to the caller | `app/src/terminal.rs`, `crates/goble-terminal/src/blocks.rs` | crate tests: exit 0 with output, non-zero exit, empty output, interrupted (130), background promotion |
| P4 | **Route the agent's shell tool through the pane.** Agent+terminal panes run the shell tool in the pane's session; panes without a terminal keep `SandboxedCommandRunner` | `crates/goble-core/src/harness.rs`, `app/src/actions.rs` | harness tests: the tool returns the pane's output; the sandbox path is still used when there is no session |
| P5 | **Correlation hardening (in-band claim id).** Carry the call id in the shell hook payload, which needs B0's shell scripts | `crates/goble-terminal/src/hooks.rs`, shell scripts | only if P2's rule 3 proves insufficient; tested against the same scripted sequences |
| P6 | **Owner into the views.** Feed the owner into A1's `associate_with_conversation` so an agent-run command appears in its conversation and a user-run one does not | `app/src/state.rs`, `app/src/ui/terminal.rs` | app tests: the two views show the right blocks after a mixed sequence |

P1–P3 are self-contained and useful without P4; P4 is the switch that makes the mode real. P6 is the handoff to [`agent-tui.md`](agent-tui.md), where A4 renders the block and A5/A6 gate the claim.

## 7. What this does not cover

- **Rendering.** The tool-call block, the status icons and the approval composer are [`agent-tui.md`](agent-tui.md) A4–A6. This doc stops at producing the block and the tool result.
- **Third-party agents.** `TuiAgent` panes host somebody else's TUI and keep their own output; parsing *their* screens for state is a separate feature and is not in this plan.
- **Remote panes.** A pane whose shell runs on a worker reaches us as a stream (`remote-terminal-renderer.md`); the claim protocol applies there only once that stream exists.
- **Shell integration (B0).** The hook channel decoder exists; the bash/zsh scripts that emit it do not. Until they do, a pane has no `Preexec`/`CommandFinished` at all and the parser has nothing to attribute — which means **B0 is a prerequisite for any of this shipping**, and P1–P3 are written against scripted hook sequences in the meantime.

## 8. Verification

- Crate items (P1, P3, P5): `cargo test -p goble-terminal`, with the test named in the row.
- App items (P2, P6): `cargo test -p goble-app`.
- Harness item (P4): `cargo test -p goble-core`.
- End-to-end: a scripted shell is not enough to prove the mode works against a real zsh; that check needs B0 and a running window. It is recorded as unverified until then, rather than claimed.
