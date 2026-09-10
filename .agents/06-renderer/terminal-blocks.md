# 06 — Block terminal (command blocks)

**Status:** `[ ]` not started — functional map + build order. Read with [`terminal-emulator.md`](terminal-emulator.md).
**Owns:** the product layer that turns a terminal pane into a list of command blocks: shell integration, block lifecycle, per-block grids, prompt suppression, clear semantics, and the block list the renderer draws.
**Depends on:** [`terminal-emulator.md`](terminal-emulator.md), [`README.md`](README.md), [`remote-terminal-renderer.md`](remote-terminal-renderer.md)

## How to read this doc

Same convention as the emulator doc, verified against the reference terminal source at `~/Projects/warp-new`. Prefixes used here:

- `A/…` = `~/Projects/warp-new/app/src/terminal/model/…` — the model layer (terminal model, blocks, grids, ansi).
- `V/…` = `~/Projects/warp-new/app/src/terminal/…` *(outside `model/`)* — the view/PTY/input layer.
- `B/…` = `~/Projects/warp-new/app/assets/bundled/bootstrap/…` — the shell integration scripts shipped with the terminal.

## What this is

The requirement, in the user's words: *"I want the same terminal style in the form of blocks when I type a command. Not the default terminal where the user prompt appears."*

That is not a rendering preference — it is a different terminal model. A classic terminal is one continuous cell screen with a shell prompt at the bottom and a scrollback buffer behind it. A block terminal keeps a real shell and a real emulator, but hides the shell's prompt and presents each command as a discrete object: header (the command), body (its output), and metadata (exit code, cwd, git branch, duration). History is the list of blocks, not a grid scrollback.

This document maps how the reference does it, what the emulator has to expose for it to work, and the order we build it in. The single most important structural fact up front: **in a block terminal there is no global screen.** `TerminalModel` owns either a `BlockList` (a `Vec<Block>`, each block holding its own private grids) or an `AltScreen`, and switches between them per escape sequence (`A/terminal_model.rs:2379`).

## 1. What changes relative to a plain terminal

| Concern | Plain terminal | Block terminal |
|---|---|---|
| Screen | one grid + scrollback | one private grid set per block; the "screen" is a viewport over a height sum-tree (`A/blocks.rs:225-227`, `A/blocks.rs:389`) |
| History | grid row eviction into scrollback | the block list; each block keeps its own bounded storage |
| Shell prompt | displayed, is the UI | captured and **hidden**; the visible prompt/command line is rendered by us (§5) |
| Command boundary | not modelled | explicit, from shell hooks (§2) |
| `clear` | clears the screen | intercepted: resets the block layout, does not wipe history (§6) |
| Full-screen TUI | normal mode | alt screen takes over rendering; the block stays open and frozen (§9) |
| Selection/search | over one buffer | over per-block grids plus a viewport-level index |

## 2. Block lifecycle

**The authoritative signals are the shell hooks, not OSC 133.** `CommandFinished`, `Precmd`, `Preexec`, `Bootstrapped` and `InitShell` drive the state machine (`A/ansi/handler.rs:241-260`; dispatch `A/ansi/mod.rs:653-658`; payload types `A/ansi/dcs_hooks.rs:414`, `:421`, `:494`, `:506`). OSC 133 is used only to route prompt bytes (§5); OSC 9277 frames in-band generator output; OSC 9279 resyncs the grid for ConPTY.

```
startup        InitShell ─────────────► Session created (session_id, shell, host)
               Bootstrapped ──────────► bootstrap complete; blocks become user blocks
per command    [typing]  prompt bytes ─► hidden prompt grid (+ visible command grid)
               execute (Enter) ───────► Block::start(): start_ts, command grid begins
               Preexec ───────────────► state Executing, output grid begins
               [output bytes] ────────► output grid
               CommandFinished ───────► close block with exit_code, CREATE NEXT BLOCK
               Precmd ────────────────► fill metadata (pwd/git/env/rprompt), AfterBlockCompleted
```

Concrete chain for one command, verified end to end: input editor emits `ExecuteCommand` (`V/input.rs:14540`) → view re-emits (`V/view.rs:20949`) → `controller.write_command` (`V/writeable_pty/terminal_manager_util.rs:70`) → which marks the block started *before* writing bytes (`V/writeable_pty/pty_controller.rs:552`, `:572`) → `TerminalModel::start_command_execution` (`A/terminal_model.rs:1717`) → `BlockList::start_active_block` (`A/blocks.rs:2801`) → `Block::start` (`A/block.rs:1177`). Then the shell's `preexec` hook arrives → `Block::preexec` (`A/block.rs:3333`) sets state `Executing` and starts the output grid. Then two hooks arrive in order from the shell's `precmd`: `CommandFinished` closes the current block and creates the next (`A/blocks.rs:3716`, `:3024-3045`), and `Precmd` fills in metadata and emits `AfterBlockCompleted` for the block that just closed (`A/blocks.rs:3733`, `:3764-3776`). Shell-side ordering proof: zsh `warp_preexec` `B/zsh_body.sh:254`, `warp_precmd` emits `CommandFinished` at `:310` then `Precmd` at `:466`; bash at `:273`, `:443`, `:664`.

**Minimum hook set** for a working block terminal (anything less and blocks never close):

| Hook | Why it is load-bearing |
|---|---|
| `InitShell` | creates the `Session`; the handler returns an error without the pending session info (`A/terminal_model.rs:2888-2895`, `:2952-2998`) |
| `Preexec` | moves the block out of `BeforeExecution` and starts output (`A/blocks.rs:3781`, `A/block.rs:3363`) |
| `CommandFinished` | the only thing that closes a block and creates the next (`A/blocks.rs:3024-3045`) |
| `Precmd` | the only carrier of pwd/git/env/session, and the bootstrap-stage prerequisite for visible blocks (`A/block.rs:3300-3334`) |
| `Bootstrapped` | advances to `PostBootstrapPrecmd`, which is what makes blocks user blocks instead of bootstrap blocks (`A/bootstrap.rs:36-46`, `A/block.rs:511-590`) |

Degradable: `InputBuffer` (typeahead), `Clear` (clear interception), `InitSubshell`, the ssh hooks, `FinishUpdate`.

Notable detail: between blocks, output that arrives with no active command (typeahead, background jobs) is handled by an `EarlyOutput` layer that can create a **background block** without a command (`A/blocks.rs:3117`, `:3082`, `A/early_output.rs:229`, `:280`). Budget for this — it is why the output path cannot assume "there is always a live block".

## 3. Block data model

`pub struct Block` (`A/block.rs:274`), built by `Block::new` (`:914`), created from `BlockList::create_new_block` (`A/blocks.rs:2614`).

| Field | Notes |
|---|---|
| `id: BlockId` | client-side id; hex form for restored blocks (`A/blocks.rs:2911`) |
| `header_grid: HeaderGrid` | the prompt and the command line — several grids, see §4 |
| `rprompt_grid: BlockGrid` | right prompt (git branch etc.) from the `Precmd` payload (`A/block.rs:1274`) |
| `output_grid: BlockGrid` | the command's output |
| `state: BlockState` | `BeforeExecution`, `Executing`, `DoneWithExecution`, `DoneWithNoExecution`, `Background`, `Static` (`A/block.rs:610`) |
| `pwd`, `git_branch`, `virtual_env`, `conda_env`, `node_version`, `rprompt`, `session_id` | from the `Precmd` hook (`A/block.rs:3307-3312`); `pwd` can also update from OSC 7 mid-command (`:3270`) |
| `exit_code` | from the `CommandFinished` payload (`A/block.rs:1566`) |
| `creation_ts`, `start_ts`, `completed_ts` | client clock, not hook-derived (`A/block.rs:984`, `:1178`, `:1568-1570`) |
| `block_index` | assigned by the list (`A/block.rs:320`) |
| `is_for_in_band_command` | set in `preexec` by inspecting the command string (`A/block.rs:3352`) |
| `interaction_mode` | `User` / `Agent`, plus agent metadata — this is where our agent integration lands (`A/block/interaction_mode.rs:331`, `:465`) |
| `hidden`, `should_hide_output_grid`, `should_hide_command_grid` | UI policy (`A/block.rs:1376`) |

**Failure is derived, not stored:** `has_failed = state == DoneWithExecution && exit_code != 0` (`A/block.rs:77`, `:2751`). Do the same — a stored flag drifts.

**Statuses we will need that are not in the reference:** nothing structural; the reference's `InteractionMode` + `AgentInteractionMetadata` covers the agent-driven cases (monitoring, in control, blocked, long-running command control).

**Rendering needs no row ranges.** The reference stores grids plus cached demarcations and answers "which section is this row in" on demand (`BlockSection` `A/block.rs:660`, `Block::find` `:2571`). Store heights, not ranges — ranges go stale on every append.

## 4. Grid model: three grids per block, no global scrollback

Each block owns `HeaderGrid`, `rprompt_grid` and `output_grid` (`A/block.rs:933`, `:941`, `:950`). `HeaderGrid` internally splits into a *prompt* grid and a *prompt-and-command* grid (`V/block_list_element.rs:216-221`, `A/header_grid.rs:26-46`), which is the mechanism behind prompt hiding (§5). Coordinates are `WithinBlock<T> { block_index, grid, inner }` (`A/terminal_model.rs:234-238`) with a viewport-level `BlockListPoint` (`A/blocks.rs:413`, `:427`).

Layout is a **height sum-tree** over blocks plus `Gap` filler items (`A/blocks.rs:225-227`, `:389`); heights are recomputed after each PTY read batch (`A/blocks.rs:3800-3808` → `:1599`, `:1605`, `:1772`). This is what makes a long transcript cheap to scroll: nothing is a cell range, everything is a height lookup.

Freezing a finished block is explicit: `Block::finish` finishes all three grids (`A/block.rs:1560-1564`); `BlockGrid::finish` drops trim mode (`A/blockgrid.rs:287-292`); `GridHandler::finish` truncates everything after the cursor, resets trailing cells and pins the max cursor (`A/grid/ansi_handler.rs:1707-1718`). Later reads are served from memoized caches (`A/blockgrid.rs:306-310`).

**Emulator-side consequence:** the emulator's own scrollback is disabled for every block grid — `GridHandler::new` hardcodes `grid_max_scroll_limit = 0` and pushes rows into a per-block flat storage instead (`A/grid/grid_handler.rs:402-411`, `:424`), so `history_size == flat_storage.total_rows()` (`:2735-2737`). The alt screen is zero-scrollback too (`A/terminal_model.rs:1106-1111`). Per-block cap is `terminal.maximum_grid_size`, default 50 000 rows (`V/settings.rs:107-109`), with a truncation counter surfaced on completion (`A/block.rs:583-586`).

**As built here** (`crates/goble-terminal`, see the status notes in [`terminal-emulator.md`](terminal-emulator.md) §0): Alacritty already has the per-screen bounded row storage, so a block's screen keeps its own scrollback (`ScreenConfig::block`, `BLOCK_HISTORY` = 50 000 rows) instead of a second storage type next to a zero-history grid. The block body is then `Screen::content_lines` — history and screen read as one contiguous run of rows — and a block is never drawn from its viewport alone. The cap behaves as the reference's does: oldest rows first, and memory stays proportional to what a command actually printed.

Two mechanisms from the reference are deliberately not built yet: the three-grid split (header/prompt/output) and the height sum-tree layout. Blocks currently expose their rows directly (`Block::command_lines`, `Block::output_lines`, `BlockList::lines`), which is enough for a renderer to draw them and costs nothing to replace once prompt hiding and block virtualization arrive.

## 5. Prompt suppression — the "no user prompt" requirement

The shell prompt still exists; we capture it and **do not display it**.

- Prompt bytes are identified by OSC 133 markers: `A` (prompt start), `B` (prompt end), `P;k=i|r` (initial / right prompt) (`A/ansi/mod.rs:1070-1077`; grammar `crates/octomus_terminal/src/model/ansi/control_sequence_parameters.rs:620-681`).
- `HeaderGrid::prompt_marker` starts the prompt panes, records that prompt characters are arriving, and on prompt end caches the demarcation (`A/header_grid.rs:1072-1133`, `:571`).
- Routing rule: prompt characters always go to the hidden `prompt_grid` (a preview), and go to the **visible** `prompt_and_command_grid` only when `honor_ps1` is set (`A/header_grid.rs:26-46`). After prompt end the raw flag clears and typed bytes land in the visible command grid (`A/block.rs:3255`, `:2938-2959`).
- `honor_ps1` is negotiated with the shell, not guessed: the shell reports it in the `Precmd` payload; on mismatch the app sends an out-of-sync event and resyncs (`A/header_grid.rs:1144-1175`; shell side `B/zsh_body.sh:454-476`; user toggle via `ESC p` / `ESC w` `B/zsh_body.sh:806-820`, `B/bash_body.sh:872-886`).
- Shell-side suppression: bash saves `SAVED_PS1` and sets `PS1=""` when the prompt is not honoured (`B/bash_body.sh:804`, `:832`, restore `:872`); zsh wraps the original prompt in zero-width `%{ %}` and adds the 133 markers (`B/zsh_body.sh:764-782`); fish replaces `fish_prompt`/`fish_right_prompt` with empty functions (`B/fish.sh:200-206`); PowerShell rebinds the global `prompt` function (`B/pwsh.ps1:947`).

Two implications for us. First, this is exactly the feature the user asked for, and it is *shell-side plus routing*, not renderer-side: we must ship shell scripts. Second, it must degrade: with `honor_ps1` on (or an unbootstrapped shell) we show the shell's own prompt inside the block and lose the pretty header — never a broken pane.

## 6. Clear semantics

Two separate mechanisms; do not conflate them.

**(a) `CSI 2J` on the primary screen preserves rows by scrolling them into the block's own storage** — not into terminal scrollback (`A/grid/ansi_handler.rs:846-855` → `clear_viewport` `:1674-1702` → `scroll_region_up` `:1645-1661`). `ClearMode::{ResetAndClear, ActiveBlock}` instead clear **in place** (`:861-863` → `clear_visible_rows_in_place` `:1704-1727`).

**(b) The `clear` builtin never runs the real binary.** Every shell integration overrides it with a function that emits a `Clear` hook (`B/zsh_body.sh:631-633`, `B/bash_body.sh:888-893`, `B/fish.sh:537-538`, `B/pwsh.ps1:894-901`), and the app answers by keeping all blocks and inserting a single `Gap` at the cursor (`A/blocks.rs:862-919`). Gaps shrink as subsequent blocks execute (`:1784-1800`).

**(c) CLI-agent redraw churn.** When an agent CLI TUI is detected, the primary grid switches to `FullGridClearBehavior::Clear`, which makes both full clears and primary-screen resize reflow mutate the visible grid in place, so old frames do not accumulate in the block's storage (`A/grid/grid_handler.rs:333-340`, `:490`; enabled on agent-session start `V/view.rs:13129-13138`; applied at `A/grid/ansi_handler.rs:849-853` and `A/grid/resize.rs:64-81`). Rationale in their spec `specs/tui-output-redraw/TECH.md:19-45`.

**Reference wart to not inherit:** the shells emit a `ClearOnNextBlock` hook (`B/zsh_body.sh:480`, `B/bash_body.sh:681`) that has **no handler anywhere in the Rust tree** — it parses as an unknown hook variant and does nothing. Either implement it or stop emitting it; a silently dead protocol message is a debugging trap.

## 7. Shell integration (the bootstrap)

Scripts live in `B/`; the assembly step picks `bash.sh|zsh.sh|fish.sh|pwsh.ps1`, inlines `*_body.sh` one level, strips comments and blanks, substitutes a ConPTY flag (`V/bootstrap.rs:119`, `:152-201`).

| Shell | Mechanism | What is replaced |
|---|---|---|
| bash | forked bash-preexec: `DEBUG` trap + `PROMPT_COMMAND` precmd/preexec arrays (`B/bash.sh:220`, `:325`; `B/bash_body.sh:1259-1263`). The user's `PROMPT_COMMAND` is saved and unset (`B/bash_body.sh:1166-1189`) | `PROMPT_COMMAND`, `PS1` |
| zsh | `precmd_functions` / `preexec_functions` (`B/zsh_body.sh:1180-1181`) | `PROMPT`, `RPROMPT` |
| fish | `--on-event fish_preexec` / `fish_prompt` / `fish_posterror` (`B/fish.sh:160`, `:267`) | `fish_prompt`, `fish_right_prompt` |
| pwsh | global `prompt` wrapper + `CommandNotFoundAction`/`PostCommandLookupAction` + PSReadLine handlers (`B/pwsh.ps1:228-232`, `:947`, `:930-945`) | the `prompt` function |

**Wire formats** (all verified on both the shell and Rust sides):

| Format | Bytes | Rust parser |
|---|---|---|
| DCS hex JSON — **the authoritative hook channel** | `ESC P $ d <hex(json)> ST` | `A/ansi/mod.rs:838-846` |
| OSC hex JSON | `ESC ] 9278 ; d ; <hex> BEL` | `:1164-1189` |
| OSC plain JSON | `ESC ] 9278 ; f ; <json> BEL` | `:1191-1203`, `:698` |
| OSC ANSI-C key/value | `9278;k;A;<Hook>` / `;k;B;<key>;<value>` / `;k;C` | `:1207`, `:747`, `A/ansi/dcs_hooks.rs:177` |
| OSC 9277 in-band framing | `ESC ] 9277 ; A|B BEL` | `:1148-1160` |
| OSC 9279 grid resync | `ESC ] 9279 BEL` (ConPTY only) | `:1214-1217` |
| OSC 133 prompt markers | `ESC ] 133 ; A|B|P;k=r BEL` | `:1070` |

We should adopt **one** hook encoding, not four: DCS hex-JSON (robust against shells that mangle OSC, and it survives ConPTY better than OSC — which is why the reference also has the 9279 resync path). Keep OSC 133 for prompt demarcation, because that is what other tooling emits too.

**Unsupported shells.** Local: the shell type comes from the resolved executable name; unknown falls back to a supported shell plus a telemetry event (`V/local_tty/shell.rs:543-555`, `V/available_shells.rs:33-45`). Remote: the ssh probe rejects anything that is not bash/zsh/fish and reports a specific unavailability reason (`A/terminal_model.rs:3060-3078`, `A/ansi/dcs_hooks.rs:379-395`, `V/ssh/error.rs:59`). A never-bootstrapped session shows a banner after a timeout instead of silently degrading (`V/view.rs:15845-15935`). Do that — silent degradation is the worst outcome.

**Hazard to not copy:** `init_shell` panics on an unparseable shell name (`A/terminal_model.rs:2973-2975`). Return an error and degrade to a plain terminal.

## 8. Input line

There is **no native line editor**. Keystrokes are written to the PTY (`V/view.rs:8684-8690`) and the shell's own editor (bash readline, zsh zle, PSReadLine) echoes them back into the block's command grid. What the reference adds is instrumentation, not replacement: bash `bind -x` on `READLINE_LINE` (`B/bash_body.sh:532`, `:752`), zsh `zle -N` + `bindkey` (`B/zsh_body.sh:349-353`, `:629`), fish `commandline -f repaint` (`B/fish.sh:249`), PSReadLine handlers (`B/pwsh.ps1:333-377`). The reported input buffer is used only for typeahead (`A/early_output.rs:26-31`).

It also infers "is the line editor active" with a 50 ms timer after prompt end (`V/line_editor_status.rs:12-18`, `:71-103`) and gates writes on it (`V/pty_controller.rs:365`). Treat that as a heuristic to replace, not a design to copy — a timer is a race by construction.

The key-encoding rules from [`terminal-emulator.md` §3.10](terminal-emulator.md) apply unchanged: what we send to the PTY is identical in a block terminal.

## 9. Degradation and edge cases

| Case | Reference behaviour | What we should do |
|---|---|---|
| Full-screen TUI (`vim`, `htop`, `less`) | Block model does **not** become a plain terminal: rendering switches to the alt screen and the block's output grid freezes mid-command (`A/terminal_model.rs:2050-2093`, `:2379-2387`). Alt screen has zero scrollback (`:1106-1111`) and its `precmd` is a deliberate no-op (`A/alt_screen.rs:626`), so no block is created while a TUI runs. `CommandFinished` force-exits the alt screen and unsets bracketed paste so a crash cannot strand the UI (`:2809-2821`) | copy this; it is the correct behaviour and it is not obvious |
| OSC 7 from inside a TUI | `set_current_working_directory` deliberately bypasses the delegate so TUIs that launch tools still update the directory (`A/terminal_model.rs:2847-2860`) | copy |
| Enormous output | per-block cap (default 50 000 rows) with a truncation counter (`A/block.rs:583-586`); restore additionally caps serialization at 5 000 stylized / 50 plain lines (`A/block.rs:80-86`) | per-block cap plus a visible "output truncated" note |
| No shell integration | no `CommandFinished` ever closes the initial hidden block; `EarlyOutput` may create background blocks (`A/blocks.rs:3117-3123`); a timer shows a banner (`V/view.rs:15845`) | banner + explicit "plain terminal mode" fallback |
| Nested `ssh` | one block contains the whole remote session; no per-command blocks (`ssh` is hook-less). OSC 7 updates are **dropped** for ssh blocks (`A/terminal_model.rs:2847-2857`); if the remote runs the same integration, the session re-registers (`B/zsh_body.sh:99-131`) | same: degrade to one block; offer our own remote path (see [`remote-terminal-renderer.md`](remote-terminal-renderer.md)) instead of trying to bootstrap arbitrary hosts |
| Agent-driven commands | `InteractionMode::Agent` + metadata (`A/block/interaction_mode.rs:331`, `:465`) | this is our integration point for the agent runtime |
| Very old / restored blocks | `SerializedBlock` with stylized/plain blobs (`A/block/serialized_block.rs:145`) | restore by re-rendering from serialized text, not from a grid snapshot |

## 10. The emulator ↔ block seam

**Emulator → blocks.** The reference uses one `Handler` trait (~100 relevant methods) that carries both the VT surface and the product hooks, and re-implements it by delegation at each layer: `TerminalModel` (chooses block list vs alt screen, `A/terminal_model.rs:2379`) → `BlockList` (chooses early-output vs active block, `A/blocks.rs:505`, `:518`) → `Block` (header vs rprompt vs output, `A/block.rs:2938-2959`) → `HeaderGrid` (prompt vs command, `A/header_grid.rs:26-46`) → `BlockGrid` → `GridHandler` (the real VT code, `A/grid/ansi_handler.rs:157`). Secondary channel: `HandlerEvent` (`A/terminal_model.rs:2417-2449`) and the block-facing `Event::BlockCompleted` / `AfterBlockStarted` / `BlockMetadataReceived` / `TerminalClear` (`V/event.rs:27-60`).

This delegation chain is the reason their emulator is not reusable as a crate — the VT code cannot be reached without the block model. **Our design keeps the split at the composition root instead:**

- `vte::ansi::Handler` is implemented **once**, by the session type that owns grid + alt screen + block list. Hook callbacks (`DCS`/`OSC` payloads) are dispatched from that one impl. No forwarding chain.
- The grid, the cursor and the alt screen stay product-free: they expose imperative methods (`finish`, `start`, `len`, `cursor_point`, `clear_visible_rows_in_place`, `history_size`, `num_lines_truncated`, `dirty_range`) and know nothing about blocks (`A/blockgrid.rs:160`, `:287`, `:368`, `:190`, `A/grid/ansi_handler.rs:1704`, `:2735`, `:1699`, `:1587`).
- The *block list* is the only component that knows the block protocol.

**Blocks → emulator.** What the block layer must be able to command: start/finish a block's grids, freeze after the cursor, clear the visible rows in place, enable the in-place clear behaviour for agent TUIs, insert a `Gap`, and query heights. All of that is in the reference's `BlockGrid`/`GridHandler` query API listed above; ours should be a small explicit interface, not trait delegation.

## 11. Build order

| # | Phase | Exit criterion |
|---|---|---|
| B0 | **Shell integration for bash + zsh**: hooks on `precmd`/`preexec`, DCS hex-JSON encoding, `PS1` suppression, `InitShell`/`Bootstrapped` handshake | a real shell session emits a parseable `CommandFinished` with an exit code for every command |
| B1 | **Block model + block list**: `Block` with the field set of §3, state machine of §2, per-block grids, height sum-tree, freeze-on-finish | typed commands become blocks with exit codes and cwd, rendered in order, scrollable |
| B2 | **Prompt hiding**: OSC 133 detection, hidden prompt grid, `honor_ps1` negotiation and toggle | with integration on, no shell prompt is visible and the command line renders in our own header |
| B3 | **Clear + truncation + edge cases**: `clear` interception, in-place clears, per-block cap and truncation notice, alt-screen takeover, no-integration banner | `clear`, `vim`, huge output and a hook-less shell all behave per §6 and §9 |
| B4 | **Persistence and cross-block operations**: serialization/restore, cross-block selection, search over blocks | reopening the app restores the transcript; selection spans blocks |

Phase B0 is independent of the emulator work and can run in parallel with emulator phases 1–3: it only needs a PTY and a byte parser.

**Status after the vendor + crate landing** (`crates/goble-terminal`; status notes in [`terminal-emulator.md`](terminal-emulator.md) §0): the *model* side of B1 exists — `blocks.rs` has the lifecycle of §2, hook decoding, per-block screens, freeze-on-finish, background promotion, `clear` handling, metadata and a renderer-facing row view. Not built: the height sum-tree, the prompt/rprompt grids, prompt hiding, persistence, and the shell scripts that emit the hooks in the first place. So a pane wired to this today would show correct blocks only from a shell that already speaks the hook protocol.

## 12. Decisions

| Item | Reference | Decision |
|---|---|---|
| Block list as the history | yes | **take** — it is the requirement |
| Per-block private grids | yes | **take** |
| Height sum-tree + `Gap` layout | yes | **take** |
| Shell integration for bash/zsh/fish/pwsh | yes | **take** bash + zsh first, fish next, pwsh later |
| DCS hex-JSON hook channel | yes (plus three redundant OSC variants) | **take**, one channel only |
| OSC 133 prompt demarcation | yes | **take** |
| Prompt hiding + `honor_ps1` | yes | **take** |
| `clear` builtin interception | yes | **take** |
| In-place clear for agent TUIs | yes | **take** |
| `ClearOnNextBlock` | emitted by shells, **unimplemented** in Rust | **skip the emission** unless we implement it |
| Line-editor-active timer heuristic | yes (50 ms timer) | **replace** with an explicit shell signal |
| Remote host bootstrap (ssh/tmux control mode) | yes, large | **later** — our remote path is mTLS, §`remote-terminal-renderer.md` |
| In-band generator OSCs (9277) for agent output | yes | **later**, only if the agent runtime needs framed output |
| Nested `ssh` as one block | yes | **take** (degrade), plus our own remote transport |

## Tasks

- [ ] Ship bash + zsh integration scripts: `precmd`/`preexec` hooks, DCS hex-JSON encoding, `PS1` save/suppress/restore, `InitShell` + `Bootstrapped` handshake.
- [x] Define the hook payload schema (`CommandFinished{exit_code, next_block_id}`, `Precmd{pwd, git, env, rprompt, session_id, honor_ps1}`, `Preexec{command}`) and one decoder — `hooks.rs`: the envelope is `DCS $d <hex(JSON)> ST`, decoded into `HookEvent` by a byte tap that leaves every other byte, foreign `DCS` included, untouched for the parser. An envelope split across two PTY reads still decodes; a malformed, oversized or unknown one is dropped and counted rather than failing the session.
- [~] Build the `Block` type and `BlockList` with the state machine of §2 and the field set of §3 — the state machine, command/output data, exit code, metadata, timing and the block-level events are in; a height sum-tree is not, and `BlockList::lines` reports the rows in reading order instead.
- [~] Give each block its own grids (prompt, prompt+command, rprompt, output) with freeze-on-finish — a block owns a command screen and an output screen, each with its own bounded storage, and both freeze when the command finishes; the prompt and rprompt grids wait for prompt hiding.
- [ ] Implement the height sum-tree + `Gap` layout for viewport scrolling.
- [ ] Implement OSC 133 prompt detection and the hidden prompt grid; negotiate `honor_ps1` with a user toggle.
- [~] Intercept the `clear` builtin and implement in-place vs scroll-into-block semantics for `CSI 2J` — the `Clear` hook drops the earlier blocks, resets the running block's output and marks a gap; the `CSI 2J` half is the emulator's behaviour and is not wired to block semantics yet.
- [ ] Add the per-block output cap and the truncation notice.
- [ ] Implement alt-screen takeover so full-screen TUIs render without breaking the open block; force-exit on command finish.
- [ ] Add an `EarlyOutput` path for output with no active command (typeahead, background jobs) — partially: bytes before the shell bootstraps land in a static preamble block, output arriving while a command still runs promotes that command's block to `Background`, and the line editor's buffer arrives through the `InputBuffer` hook.
- [ ] Detect a never-bootstrapped shell and show a banner + plain-terminal fallback instead of a stuck pane.
- [ ] Add block serialization/restore and cross-block selection + search.
- [ ] Expose the block seam to the agent runtime (`InteractionMode`-equivalent: user vs agent, monitoring, blocked).
