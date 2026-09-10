# 06 — Terminal emulator

**Status:** `[ ]` not started — this is the functional map and the build order. Read it before writing any emulator code.
**Owns:** the VT emulator behind the terminal pane: parser, cell grid, cursor model, modes, scrollback, alt screen, resize, host→PTY input encoding, and the seam to the renderer.
**Depends on:** [`README.md`](README.md), [`remote-terminal-renderer.md`](remote-terminal-renderer.md), [`../04-agent-runtime/sandbox-and-cwd.md`](../04-agent-runtime/sandbox-and-cwd.md)

## How to read this doc

Everything here was read out of a reference terminal's real source — `~/Projects/warp-new` (the Warp source tree, internally text-substituted to "octomus"). Two citation prefixes are used throughout:

- `A/…` = `~/Projects/warp-new/app/src/terminal/model/…` — where the emulator state machine actually lives.
- `C/…` = `~/Projects/warp-new/crates/octomus_terminal/src/model/…` — the small reusable primitives crate.

The citations are there to **check our design head-on against a shipping implementation**: to know which behaviours are load-bearing, which edge cases are subtle, and where the reference is wrong or incomplete so we do not reproduce the mistake. They are not a port plan, and no code is to be copied (see [Appendix A](#appendix-a--licensing)).

## What this doc is

Our terminal pane is not a terminal. `app/src/terminal.rs:163-168` says it plainly: *"Real terminal emulation (cursor addressing, alternate screen, etc.) is out of scope; this is 'ANSI-aware enough'."* Every escape sequence is **discarded** — CSI (`:244-248`), OSC (`:249-258`), two-byte `ESC x` (`:235-239`). Only `\r` and backspace are acted on. The pane can echo a shell's output, but it cannot render a full-screen TUI — which is exactly what `codex`, `claude` and `gemini` are. And it cannot *drive* one either: today no key beyond Enter/Backspace/Tab/Ctrl-letter reaches the PTY (`classify_key`, `app/src/terminal.rs:328-370`).

So the job is a real VT emulator. This document is the **functional map**: every capability such an emulator must have, the semantics of each, the order to build them in, and how each one is verified. It is deliberately written at implementation depth — precise enough that the person writing the code does not have to re-derive terminal semantics from a spec.

## 0. What is already built

The emulator is not written from scratch, and it does not live in a tree under `app/src/terminal/`. The decisions taken while implementing it:

- **The emulator is the vendored Alacritty crate.** `vendor/alacritty_terminal` holds `alacritty_terminal` 0.26.0 (Apache-2.0) verbatim: grid, cursor, modes, alt screen, reflow, and the `vte` parser. It is a path dependency of `crates/goble-terminal` and is excluded from the workspace. `goble-terminal` always reaches the parser through the crate's own `pub use vte` re-export, so parser and handler cannot drift apart. Provenance, the SHA-256 of the published archive, and the two deliberate deletions are recorded in `THIRD-PARTY-NOTICES.md`; `scripts/fetch-alacritty-terminal.sh` re-downloads the archive and diffs it against `vendor/`.
- **There is no second parser.** The agent harness speaks typed protocol events rather than escape sequences, and a pane runs a real shell over a PTY, so one VT parser covers every byte that reaches a grid. What the shell-integration channel adds is an *envelope* decoder (`hooks.rs`) and what the keyboard adds is an *encoder* (`keys.rs`); neither parses escape sequences.
- **`crates/goble-terminal` is the seam** that keeps Alacritty's types out of the rest of the codebase. `keys.rs` — host→PTY key encoding with the xterm modifier form. `hooks.rs` — the shell-hook channel: a byte tap that lifts a `DCS $d <hex(JSON)> ST` envelope out of the PTY stream, decodes it, and passes every other byte through untouched, including foreign `DCS`. `screen.rs` — one terminal surface (grid, cursor, modes, alt screen, query formatters) plus a plain row/cell snapshot for the renderer. `blocks.rs` — the command-block model of [`terminal-blocks.md`](terminal-blocks.md). 64 unit tests, `cargo check`/`clippy` clean, no app binary built.
- **The renderer seam is a snapshot, not a trait with borrows.** §2.3 rule 3 proposed `trait GridView`. What shipped is `Screen::lines` (visible rows), `Screen::content_lines` (history + screen), `Screen::cursor` and `ScreenColor`/`CellAttrs`/`ScreenCell` — owned values the renderer consumes without knowing what a screen is. A trait can still be introduced if a second implementation ever appears; a snapshot cannot leak a screen's lifetime into the renderer.
- **Block screens keep 50 000 rows of scrollback each** (`ScreenConfig::block`, `BLOCK_HISTORY`). Warp disables per-grid history and copies scrolled rows into a separate flat storage (see §3.7); Alacritty already has that storage, so the same model is one `Config` field. A block draws its whole grid (`Screen::content_lines`), never just its viewport, so output taller than the window is kept and a resize that reflows content into more rows cannot lose it. The cap bounds one runaway command; past it the oldest rows are dropped, which is what the reference does too.
- **Freezing is not a row cap.** `Screen::freeze` makes the screen refuse further bytes, and `BlockList`/`Block` route later output to the block that owns it, so a finished block cannot grow. Nothing is truncated, because the block was already showing all of its content.
- **Status is tracked in [`.agents/TRACKER.md`](../TRACKER.md)**, not here: the phases below describe the emulator's capability set, and the crate delivers their mechanism, while the pane itself is still an ANSI-stripping line buffer.

## 1. Build order

Ordered by value per unit of work, not by subsystem tidiness. Each phase has an exit criterion; do not start the next phase before it holds.

| # | Phase | What ships | Exit criterion |
|---|---|---|---|
| 0 | **Key encoding** | Complete host→PTY encoder: arrows, Home/End, PageUp/Down, Insert/Delete, F1–F20, Alt/Meta, Shift+Tab, xterm modifier form | `codex`/`claude` receive arrow keys and respond; today they cannot (see §3.10) |
| 1 | **Parser + grid core** | `vte` driver, VT-only `Handler` trait, cell grid with fg/bg/flags, renderer seam via `GridView` | A shell's PS1 renders with correct colours through the new grid; existing pane behaviour preserved |
| 2 | **Addressing + editing + SGR** | Cursor addressing, deferred wrap, ED/EL/ICH/DCH/ECH/IL/DL/SU/SD/DECSTBM, tab stops, IRM, SGR (16/256/truecolor) | `vim` opens in the primary screen with correct colours and line edits; `printf` cursor-addressing test loop is stable |
| 3 | **Alt screen + modes + replies** | 47/1049 alt screen, DEC private mode table, queries (DA1/DA2/DSR/CPR/XTVERSION), DECSCUSR, title | `vim`, `htop`, `less` fill the pane and restore on exit; no TUI-driven misparse |
| 4 | **Scrollback + resize/reflow** | Flat storage scrollback with a row cap, `set_size` wired, reflow path + no-reflow path for alt screen | Resizing the window reflows wrapped text without corruption; scrollback survives resize; the PTY is no longer stuck at 24×96 |
| 5 | **Terminal surface** | Mouse (1000/1002/1003 + SGR 1006), bracketed paste, focus events, OSC 52 clipboard, OSC 8 hyperlinks, OSC 7 cwd, OSC 133 markers | Mouse-driven TUIs work; paste is not eaten by TUIs; the directory pill updates from shell integration |
| 6 | **Conformance harness** | Fixture format + runner (see §5), fed by recorded real sessions; widen the corpus | New parser/grid work is verified by fixtures, not by eyeballing the pane |

The product is a **block terminal**, so a second, parallel track exists above this emulator: shell integration and the block list ([`terminal-blocks.md`](terminal-blocks.md) §11, phases B0–B4). Its phase B0 needs only a PTY and this parser and can start immediately — it does not wait for phases 1–6.

Phase 0 is one file of table lookups and it unblocks the entire agent story; it is the single highest-return item in this document. Phase 1 is the one that must be designed carefully, because everything else hangs off the cell and cursor representation.

## 2. Architecture

### 2.1 Pipeline

```
PTY bytes ──► vte::Parser ──► Performer (impl vte::Perform) ──► Handler trait ──► grid state
                                                                      │
renderer ◄──── GridView trait ◄───────────────────────────────────────┘
```

The reference uses exactly this shape and it is the right one (`A/ansi/mod.rs:34`, `:395`, `:630`, `:790`). Two properties make it worth adopting:

- The parser is **not hand-written**. It is the `vte` crate, driven by a thin `Processor`/`Performer` layer. Escape-sequence state machines are where hand-rolled emulators die.
- The parser faces a **trait**, so the grid can be swapped (primary vs alt screen), wrapped (blocks, later), or observed (tests) without touching the parser.

### 2.2 Where our version lives

The plan was a tree under `app/src/terminal/`. What shipped is a crate, so that the emulator can be compiled and tested without the app and without the GPU:

```
crates/goble-terminal/
  src/keys.rs     host→PTY encoder (phase 0)
  src/hooks.rs    the shell-hook channel: envelope tap + HookEvent
  src/screen.rs   one terminal surface: grid, cursor, modes, alt screen, query
                  formatters, and the row/cell snapshot the renderer reads
  src/blocks.rs   the command-block model (terminal-blocks.md)
vendor/alacritty_terminal/   the emulator itself (grid, cursor, reflow, vte)
```

### 2.3 Rules we adopt

1. **Upstream `vte`, not the pinned fork.** The reference pins a warpdotdev fork by git rev (`Cargo.toml:312`, `vte` 0.13.0 at `4b399c87`). Upstream `vte` 0.15 (Apache-2.0) is more than a byte state machine: behind its `ansi` feature it ships the whole performer layer — `Processor` (with synchronized-output buffering and a pluggable `Timeout`), the `Handler` trait, and the VT type vocabulary we would otherwise write ourselves: `Mode`/`NamedMode`/`PrivateMode`/`NamedPrivateMode`, `KeyboardModesApplyBehavior`, `ModifyOtherKeys`, `CursorStyle`/`CursorShape`, `LineClearMode`/`ClearMode`/`TabulationClearMode`, `NamedColor`/`Color`, `Attr`, `CharsetIndex`/`StandardCharset`, `Rgb`, `Hyperlink` (verified in `vte/src/ansi.rs`). So "start from Alacritty" at minimum means: depend on `vte` 0.15 with the `ansi` feature and implement `Handler`. See §2.4.
2. **One `Handler` impl as the composition root; the grid stays product-free.** The reference's trait is ~120 methods (`A/ansi/handler.rs:26-410`) of which roughly 25 are product callbacks — `command_finished`, `precmd`, `preexec`, `bootstrapped`, `ssh`, `init_shell`, `input_buffer`, `clear`, `exit_shell` (`:241-331`), in-band output (`:316-324`), completions (`:333-360`) — and it re-implements that trait by delegation at six layers (`TerminalModel` → `BlockList` → `Block` → `HeaderGrid` → `BlockGrid` → `GridHandler`). That chain is why their emulator cannot be lifted as a crate, and it is the thing not to copy. We implement `vte::ansi::Handler` **once**, on the type that owns grid + alt screen + block list, and dispatch the hook callbacks from there. The grid, cursor and alt screen expose imperative methods and know nothing about blocks or agents ([`terminal-blocks.md` §10](terminal-blocks.md)).
3. **A `GridView` trait at the renderer seam.** The reference has none: `render_grid(&GridHandler, …)` takes a concrete app type and ~30 parameters (`app/src/terminal/grid_renderer.rs:282-313`). Ours should be `trait GridView { fn rows(&self) -> usize; fn cols(&self) -> usize; fn row(&self, i: usize) -> Cow<'_, Row>; fn cursor(&self) -> Option<CursorView>; }` so the renderer never learns what a screen is.
4. **Reflow is in scope from day one.** Retrofitting it later means rewriting scrollback, selection and cursor restoration at the same time. It is phase 4 above, designed in phase 1.
5. **Ship phase 0 before anything else.**

### 2.4 Build vs. reuse: what we take from Alacritty

Question that prompted this: *should we start from Alacritty, and does Apache-2.0 let us modify or reuse parts of it while keeping the license?* Verified answers.

**Yes — Apache-2.0 is permissive, not copyleft.** It allows use, modification, partial reuse and redistribution, including inside a differently-licensed (here MIT) or closed-source product. We do **not** have to license our own code under Apache-2.0. The conditions, if we copy code rather than depend on it:

1. keep all copyright, patent, trademark and attribution notices for the parts we take;
2. ship the Apache-2.0 license text;
3. carry the attribution notices from any upstream `NOTICE` file — **Alacritty has no `NOTICE` file** (verified: the repo root holds only `LICENSE-APACHE` and `LICENSE-MIT`), so this is moot for Alacritty;
4. state that we changed the files;
5. accept the patent-termination clause (suing over patents ends the grant);
6. no implied endorsement and no use of their marks.

Alacritty the repository is **dual-licensed Apache-2.0 OR MIT** (both license files at the root), so code taken from the repo may be taken under MIT — the simplest option for an MIT workspace. The published `alacritty_terminal` crate, however, declares only `Apache-2.0` (`alacritty_terminal/Cargo.toml:7`), so as a *dependency* the Apache terms apply; still trivial.

**The distinction that actually matters: Alacritty ≠ warp-new's copy of Alacritty.** Warp's `crates/octomus_terminal` is distributed under that workspace's **AGPL-3.0-only** license, and its "adapted from alacritty_terminal under the Apache license" headers do not grant us a clean Apache license for *Warp's modifications*. If we want Alacritty code, we take it from Alacritty at a pinned version or commit — never from the reference tree. Same rule as everywhere else in this doc: read Warp for behaviour, take code from the permissive upstream only.

Three levels of reuse, all license-clean:

| Option | What we get | What we still write | Cost |
|---|---|---|---|
| **A. `vte` 0.15 (parser + `Processor` + `Handler` + VT types)** | the whole parser layer, and the trait we implement | the grid, cursor, modes, scrollback, alt screen, reflow, input encoder, renderer seam — i.e. this document | 3–6 months of the phases in §1 |
| **B. `alacritty_terminal` 0.26 as a dependency** | a battle-tested emulator: grid, cells, wide chars, combining, reflow, alt screen, modes, selection, vi-mode, search, damage tracking, a renderer-facing `RenderableContent` + `RenderableCursor` API, and even its own PTY module | the renderer, the input encoder, shell integration, and any product events it does not emit | weeks, not months |
| **C. Vendor specific Alacritty files into our crate** | the parts of B we want, without the whole crate | the rest | middle; requires the attribution duties above |

**Recommendation: A, with B as the oracle.** Option B is genuinely attractive and nothing blocks it legally, but two things count against it here:

- It emits no command-boundary events. `alacritty_terminal` 0.26's `Event` enum is `MouseCursorDirty`, `Title`, `ResetTitle`, `ClipboardStore`, `ClipboardLoad`, `ColorRequest`, `PtyWrite`, `TextAreaSizeRequest`, `CursorBlinkingChange`, `Wakeup`, `Bell`, `Exit`, `ChildExit` (verified from docs.rs) — there is no cwd event and no prompt/command event. We need both for the block terminal (§9). Intercepting OSC before it reaches their parser means writing a wrapper that forwards all ~200 `Handler` methods — worse than owning the trait.
- Its scroll model assumes terminal scrollback. Our history is the block list (§9).

Because the parser is already a dependency either way, option A is not "write an emulator from scratch" — it is "implement `Handler` and own the grid". And B stays useful *inside our test suite*: feeding the same recordings to `alacritty_terminal` and to our emulator and diffing the grids is a differential oracle the reference project does not have (§5).

Option C is legitimate and worth remembering if a specific algorithm (the reflow index, say) proves expensive; take it from Alacritty at a pinned rev, with the attribution duties discharged in `THIRD-PARTY-NOTICES.md`.

**Outcome (see §0): we took B, and vendored it.** The crate is used as a unit, but from `vendor/` rather than from crates.io, so the parser, the grid and our `Handler` side are pinned together and a fork is a one-line path change if we ever need one. The two objections above were both answered without touching Alacritty's source:

- *No command-boundary events.* We do not need Alacritty to emit them: the shell emits them, as hex-JSON in a DCS envelope, and `hooks.rs` lifts that out of the byte stream before the parser sees it (the `Handler` wrapper that option A would have required is exactly what this avoids).
- *Its scroll model assumes terminal scrollback.* Per block, terminal scrollback *is* the right model: the rows a block scrolls through are that block's own output. The block list is the layout, not the storage.

Option B also keeps its value as a differential oracle: `alacritty_terminal` is now inside the product, so the recordings in its upstream test suite can be run against it with `scripts/fetch-alacritty-terminal.sh <dir>`.

## 3. The functional map

Each subsection is a capability area. "Must" items are required for the phase in §1; the reference citations document the exact behaviour to match or to beat.

### 3.1 Parser and control input

| Input class | Must handle | Notes from the reference |
|---|---|---|
| C0 controls | HT BS CR LF VT FF BEL SUB SI SO | `A/ansi/mod.rs:803-817` |
| CSI | Full dispatch skeleton, parameters, intermediates, private (`?`) prefix, colon sub-parameters | `:1309-1561` |
| OSC | Curated set (§3.11); BEL and ST terminators both accepted | `:860-1305`, `bell_terminated` at `:862` |
| ESC two-byte | `ESC 7`/`ESC 8` (DECSC/DECRC), `ESC D/E/H/M` (IND/NEL/RI/RI), `ESC =`/`ESC >` (keypad), `ESC ( X` charset designation | `:1595-1613`, `:1610`/`:1612` |
| DCS | Pass through / ignore for now; the parser must consume it without corrupting the grid | Reference implements tmux control mode + its own JSON hooks as final `d`/`f` (`:820-828`, `:838-856`) — product surface, skip |
| APC | Ignore (Kitty graphics later) | — |
| Charsets | ASCII + DEC Special Graphics (line drawing), G0–G3 designation, SI/SO | `C/ansi/control_sequence_parameters.rs:465-513` |
| Invalid input | Never panic. Log-and-ignore is correct; silently dropping the sequence is correct | `unhandled!()` pattern throughout |
| UTF-8 | Incremental decode across chunk boundaries | `vte` handles this if fed raw bytes; do not pre-decode to `String` |

**Synchronized output (DEC 2026)** must be handled in the parser layer, not the grid: while active, the performer buffers rendered updates and flushes on unset. In the reference, the mode-set path is an empty handler in the grid (`A/grid/ansi_handler.rs:1036`) and the actual buffering lives in the performer (`A/ansi/mod.rs:1388-1390`), with the `'l'` arm deliberately skipping it (`:1431-1435`) and the flush in `finish_sync_output` (`:1609`). It is what stops agent TUIs from tearing; treat it as required for phase 3.

### 3.2 Cell and row model

The cell is the most load-bearing data type in the whole system; get it right first.

| Concern | Requirement | Reference |
|---|---|---|
| Cell layout | char + fg + bg + attribute flags + optional extra (combining marks). Reference cell is 24 bytes with `CellExtra` boxed out of line | `C/cell.rs:144-150`, `cell_type.rs:6-16` |
| Wide chars | A wide glyph occupies **two** cells: the base carries `WIDE_CHAR`, its right neighbour is `DEFAULT_CHAR` (`'\0'`) with `WIDE_CHAR_SPACER`. A wide char that does not fit at the row end leaves `LEADING_WIDE_CHAR_SPACER` | `C/cell.rs:16,48,49,54` |
| Width rule | Width is always `1 + has(WIDE_CHAR)` — never recomputed per write | `C/flat_storage/grapheme.rs:41-59` |
| Overwriting | Writing over either half of a wide char must repair **both** cells, including removing the previous row's `LEADING_WIDE_CHAR_SPACER` | `A/grid/ansi_handler.rs:1538-1600` (spacer removal `:1564-1571`) |
| Boundary repair | Per edit operation, before/after shifting: `reset_wide_char_at_start_boundary` / `_end_boundary`. Called by ICH, DCH, ECH, EL, ED | `:1504-1519`, `:1525-1533`; call sites `:347,:357` (ICH), `:683,:685` (DCH), `:651,:653` (ECH), `:749,:752` (EL), `:822,:835` (ED) |
| Combining marks | Stored in `CellExtra.cell_with_zero_width`; cap 256 bytes with a warning at 128, overflow silently dropped | `C/cell.rs:33,37,113-121,225-247` |
| Combining attach point | Cursor column, or `col-1` if the cursor is *not* wrap-pending, minus one more if that lands on a spacer | `A/grid/ansi_handler.rs:198-207` |
| VS16 emoji promotion | Variation selector promotes the previous glyph to wide: writes a spacer at the cursor and advances. Gate per-grid behind a capability flag | `:227-239`, `grid_handler.rs:494-500` |
| Grapheme segmentation | The reference does **not** segment graphemes at write time (`unicode-segmentation` is a dev-dependency only). We should: it is what makes emoji ZWJ sequences and Indic clusters render correctly | — (our improvement) |
| Row | Row = `Vec<Cell>` or flat storage slice, plus a `WRAPLINE` flag for soft-wrapped continuation | `C/flat_storage`, flags `C/cell.rs:16` |

Erased cells are **not** default cells: every erase site writes the **current cursor background** with default fg and cleared flags (`C/cell.rs:340-348`; sites `:649` ECH, `:671` DCH, `:741` EL, `:798` ED, wide-resets `:1504-1533`). Getting this wrong makes `clear` and TUI backgrounds visibly wrong. Exception: DECALN writes `'E'` with `Cell::default()` to the visible grid only (`:1187-1193`).

### 3.3 Cursor model

**Per-grid cursor state** (reference: `A/grid/grid_storage.rs:30-49`): position (`point`), the **cell template** (current SGR state: char + fg + bg + flags), the charset selection, and the **deferred-wrap flag**.

The deferred-wrap flag (`input_needs_wrap`) is the single most commonly botched piece of terminal semantics. The rule:

- Writing a character in the **last column** does not wrap immediately; it sets the flag (`A/grid/ansi_handler.rs:1459`, `:1466`).
- The next printable character triggers the wrap (`:244-246` → `wrapline()` `:1473-1496`), and the wrapped row gets `WRAPLINE` **only if** line wrap is enabled.
- The flag is cleared by anything that moves the cursor explicitly: the shared `goto` choke point (CUP/HVP/VPA/CHA/CUU/CUD/CNL/CPL) `:309`, `:324`; `move_forward`/`move_backward` `:450-465`; BS but **only when `col > 0`** `:515-521`; CR `:526-530`; DECRC `:729`; RIS `:894`.
- It is **not** cleared by LF/VT/FF/IND `:534-547`, nor by RI, ICH/DCH/ECH/IL/DL/ED/EL/SGR, nor by mode set/unset — DECCOLM/DECSTBM only clear it indirectly by homing the cursor.
- TAB while wrap-pending is a line break `:481-484`.

Cursor mutation map (what each sequence touches):

| Sequence | Effect |
|---|---|
| CUP/HVP/CUU/CUD/CNL/CPL/VPA/CHA/CUF/CUB | position; clears the wrap flag |
| LF/VT/FF/IND | row only; wrap flag untouched |
| RI | row |
| CR | column |
| BS | column; clears wrap only if `col > 0` |
| TAB/HTS/CHT | column, may bounce to the next line |
| CBT | column |
| TBC | tab stops |
| SGR | cell template (fg / bg / flags) |
| DECSC/DECRC | the whole cursor struct (§3.4 save set) |
| RIS | cursor, saved cursor, max-cursor reset (`grid_storage.rs:442-453`) |
| DECSTBM | position (homes to origin) **and** scroll region |
| DECCOLM | scroll region + all visible cells |
| ED/EL/ICH/DCH/ECH/IL/DL/SU/SD | **no cursor movement** |
| DSR 6 / CPR | read-only |

Also tracked: `max_cursor_point` (high-water mark of cursor and content) updated on writes (`:258-268`, `grid_handler.rs:1681-1691`). This is what bounds how much scrollback can be pulled back after a large scroll; it is easy to forget and cheap to keep.

### 3.4 Modes

Reference: definition `C/ansi/control_sequence_parameters.rs:114-161`, set `A/grid/ansi_handler.rs:969-1037`, unset `:1040-1095`. `TermMode::default()` = `SHOW_CURSOR | LINE_WRAP | ALTERNATE_SCROLL | URGENCY_HINTS` (`C/mode.rs:70-77`).

Private (`?`) modes to implement:

| Mode | Name | Behaviour |
|---|---|---|
| 1 | DECCKM | application cursor keys → SS3 instead of CSI |
| 3 | DECCOLM | reset scroll region + blank visible cells. Reference deliberately does **not** switch 80/132 (`:1028`) |
| 6 | DECOM | origin mode, relative to the scroll region |
| 7 | DECAWM | line wrap |
| 12 | Blinking cursor | cursor style + a change event (`:1030-1035`) |
| 25 | DECTCEM | cursor visibility |
| 47 / 1049 | Alt screen | single `Mode::SwapScreen { save_cursor_and_clear_screen }`: `?47` → false (`C/ansi/control_sequence_parameters.rs:142`), `?1049` → true (`:145`) |
| 1000 / 1002 / 1003 | Mouse reporting | click / drag / any-motion, mutually exclusive |
| 1004 | Focus in/out | `ESC[I` / `ESC[O` |
| 1005 / 1006 | Mouse encoding | only 1006 (SGR) matters (§3.10) |
| 1007 | Alternate scroll | wheel in alt screen sends arrows |
| 1042 | Urgency hints | ignore-able |
| 2004 | Bracketed paste | wrap pasted text in `ESC[200~` … `ESC[201~` |
| 2026 | Synchronized output | parser-level buffering (§3.1) |

Non-private modes: `4` (IRM insert), `20` (LNM: LF implies CR).

Non-private modes other than 4 and 20 are out of scope, and `?1047`/`?1048` are legitimately uncommon — the reference **does not implement them** and ignores them with a trace (`control_sequence_parameters.rs:142-153`). If we implement them, do it deliberately (1047 = alt screen without cursor save, 1048 = save/restore cursor only).

**DECRQM** (`CSI ? Ps $ p`) must report real mode state for every mode we implement. The reference implements it **only for `?2026`** and replies `unhandled!()` for everything else (`A/ansi/mod.rs:1459-1483`); worse, it reports the performer's sync state rather than the grid's `TermMode`, so no mode bit is ever visible to an application. Applications do probe this to decide whether to use a feature. Implementing it properly is cheap and puts us ahead.

### 3.5 Edit operations

All in `A/grid/ansi_handler.rs`; dispatch at `A/ansi/mod.rs:1343` (ICH), `:1397` (ED), `:1411` (EL), `:1424` (IL), `:1442` (DL), `:1458` (DCH), `:1558` (ECH), `:1515` (SU), `:1519` (SD), `:1506` (DECSTBM).

| Op | Parameter semantics | Cursor |
|---|---|---|
| ICH `CSI @` | default 1, clamped to `cols - col`; shift `[dest..cols)` right, blank the inserted span | unmoved |
| DCH `CSI P` | default 1, 0 is a no-op, clamp to `cols`; left-shift and blank the tail | unmoved |
| ECH `CSI X` | default 1, 0 → 1 at dispatch, clamp to `cols`; blank `[col, end)` | unmoved |
| IL `CSI L` | default 1; only inside the scroll region, else no-op | unmoved |
| DL `CSI M` | default 1, clamped to rows below the cursor; requires the cursor inside the region | unmoved |
| SU `CSI S` | default 1; scrolls the region up from `region.start` | unmoved |
| SD `CSI T` | default 1; scrolls the region down from `region.start` | unmoved |
| ED `CSI J` | `0` below, `1` above, `2` all, `3` saved lines; ignore other values | unmoved |
| EL `CSI K` | `0` right, `1` left, `2` all | unmoved |
| DECSTBM `CSI r` | top default 1, bottom default rows; **`CSI 1;0r` means full screen** (0 is filtered, `:1509-1511`); `top >= bottom` is ignored, not reset (`:1104-1107`); homes the cursor relative to origin | homes |

Scroll-region mechanics worth getting exactly right:

- `scroll_down_relative` clamps the line count **twice** — by region length and by `region.end - origin_row` — and scrolls only `origin_row..region.end` (`:1607-1621`).
- `scroll_up_relative` clamps by region length only (`:1632-1638`); over-scroll is prevented by the reset branch in `GridStorage::scroll_up` (`A/grid/grid_storage.rs:378-383`).
- SU/SD originate at `region.start`; IL/DL originate at the **cursor row**, which is why IL/DL are no-ops when the cursor is outside the region (xterm-conformant).
- The region is reset to full screen on resize (`A/grid/resize.rs:44-45`) and by DECCOLM (`:1664`).

Edge cases in the reference we should decide on deliberately rather than inherit: DCH with `count >= cols` blanks the whole row (`:674`); ED `1` (above) calls `flat_storage.clear()` first (`:805`), i.e. **`CSI 1J` silently destroys all scrollback**; EL also evicts iTerm image placements (`:785-791`). Also, `clear_viewport` has a dead code path — its walk-back loop seeds `(0, columns)`, which is out of bounds, so it always scrolls a full viewport into scrollback (`:1674`).

### 3.6 SGR

Parsing (`C/ansi/control_sequence_parameters.rs:517-602`) matches **one slice per `;` parameter**, and colon sub-parameters arrive as a **multi-element slice** — that is the mechanism behind `38:2:r:g:b`.

Supported set to implement: `0` reset · `1` bold · `2` dim · `3` italic · `4` underline (+ `4:0` cancel, `4:2` double, `4:3/4:4/4:5` curly/dotted/dashed) · `5`/`6` blink · `7` reverse · `8` hidden · `9` strike · `21` cancel bold · `22` cancel bold+dim · `23`/`24`/`25`/`27`/`28`/`29` cancels · `30–37`/`90–97` fg · `40–47`/`100–107` bg · `38`/`48` extended colour · `39`/`49` defaults · `58` underline colour · `53` overline.

Colour parsing rules that must be exact:

- Semicolon form: `38;5;n` (256-indexed) and `38;2;r;g;b` (truecolor), disambiguated by the first sub-parameter, with the rest flattened; `_` → unsupported (`:550`, `:607-616`; same for `48`).
- Colon form: `rgb_start = if params.len() > 4 { 2 } else { 1 }` (`:555`). So `38:2:r:g:b` and `38:2:cs:r:g:b` both work — but `38:5:0:n` (length 3) takes `rgb_start = 1` and **misreads `0` as the palette index**. Fix the length-3 case (`38:5:n`).
- A missing parameter drops the whole colour parameter and leaves the colour unchanged (`next()?`).
- Out-of-range components are **rejected, not clamped** (`u8::try_from(..).ok()?`), e.g. `38;2;300;0;0` is silently ignored. Clamping is friendlier; pick one and document it.
- Application: setting underline clears double-underline and vice versa; cancel-bold-dim clears both bits; reset clears fg/bg/flags.
- **Reference bug not to copy:** SGR 5/6/25 are parsed and then fall through to `log::debug!("Term got unhandled attr")` (`A/grid/ansi_handler.rs:962-966`). Blink is a visible, cheap feature; implement it (and keep DECSCUSR's blinking cursor distinct from cell blink).

### 3.7 Scrolling, scrollback, alt screen, resize

**Scrollback.** Two-tier in the reference: the per-grid history is disabled (`max_scroll_limit = 0`, `grid_handler.rs:406-411`) and rows are pushed into a flat storage (`:347`, `:425`), so `history_size == flat_storage.total_rows()` (`:2734`). Rows are pushed in `scroll_region_up` (`A/grid/ansi_handler.rs:1645-1662`), skipping the alt screen. Storage layout: `Content` is 1024-byte append-only chunks in a `BTreeMap` (`C/content.rs:186`); `Index` is a `VecDeque` of 24-byte row entries (`C/index.rs:74-82`). Eviction is strictly FIFO from the front (`C/flat_storage/mod.rs:94-99,114-123,164-179`) with a `num_truncated_rows` counter. Cap: reference default **50 000 rows** (`app/src/terminal/settings.rs:107-115`), 0 on the alt screen.

Anything that stores absolute row positions (selection, search results, bookmarks) must be anchored, and re-anchored on eviction — the reference uses `AbsolutePoint` (`grid_handler.rs:226-248`) and prunes find matches (`async_find.rs:992-1019`). Their selection only slides through one eviction path (`blocks.rs:3480-3489` → `selection.rs:1149`), so other eviction routes leave it stale; and `flat_storage.clear()` does not bump the truncation counter. We should centralise "rows were evicted" into one signal that all anchored state listens to.

**Alt screen.** Entry (`A/terminal_model.rs:2050-2096`): no-op if already active; copy the block cursor into the alt grid; reset fractional scroll and the kitty keyboard stack; for `1049` also save the cursor to the primary grid, wipe the alt grid to the background colour, and clear secrets; always clear the selection and emit a mode-swap event. Exit (`:2103-2118`): evict all alt-screen images; restore the cursor only for `1049`. The alt grid persists across swaps; it has no scrollback (`max_scroll_limit = 0`, `:1106-1108`) and a debug assert enforces it at end-of-byte-processing (`A/grid/ansi_handler.rs:1229-1231`). Resize while active takes the no-reflow branch and clears the selection on dimension change (`alt_screen.rs:123-129`).

**Resize and reflow.** Entry `A/grid/resize.rs:15-52`: reset the scroll region, rebuild tab stops, re-apply the display filter. Two paths:

- **No reflow** — alt screen, or an in-flight CLI-agent TUI: `GridStorage::resize(reflow=false, …)`: truncate/pad rows in place.
- **Full reflow** — snapshot the three cursor states (stamping `HAS_CURSOR`), push visible rows to flat storage, convert positions to byte offsets, `FlatStorage::set_columns` → `Index::rebuild` (**this is the whole rewrap algorithm**, `C/index.rs:99-145`), convert back, pop rows, store them, clamp the cursors, apply the row cap (`:178`). Soft-wrapped rows are joined and rewrapped; hard rows keep their `'\n'` (`C/flat_storage/mod.rs:255-267`, `index.rs:126-131`).

The reference also still carries the legacy alacritty reflow (`C/grid_storage/resize.rs:107-478`) behind a feature flag (`grid_storage.rs:118-120`). We implement one path only.

**Fix while we are here:** our PTY is hard-coded to 24×96 (`app/src/terminal.rs:509-514`) and `TerminalSession::set_size` has **no caller** (`:583`). Until that is wired, every program we spawn sees the wrong dimensions — no amount of emulator work fixes that.

### 3.8 Terminal surface features

| Feature | Behaviour | Reference |
|---|---|---|
| Title (OSC 0 / OSC 2) | set window/tab title; keep a title stack (`CSI 22/23 t`) | `A/ansi/mod.rs:891`, `:1522-1524` |
| Cursor style | DECSCUSR `CSI Ps q` sets block/beam/underline + blink; OSC 50 also accepted | `:1485-1503`, `:1011-1026` |
| Bell | audible bell; also expose an event so the app can flash | `A/grid/ansi_handler.rs:549-555` |
| Bracketed paste | wrap in `ESC[200~`/`ESC[201~`; rewrite `\n` → `\r` | `app/src/terminal/view.rs:16222-16298` |
| Focus events | `ESC[I` / `ESC[O` only while focus reporting is on and in the alt screen | `view.rs:7737-7795` |
| OSC 52 clipboard | store and load; base64; `?` queries | `A/ansi/mod.rs:1029-1039` |
| Mouse | §3.10 | — |
| Selection + copy | basic word/line/block selection; copy on the platform clipboard | `model/selection.rs`, `grid_handler.rs:1812-2055` |
| Search / find | search scrollback with match navigation | `model/find.rs`, `async_find.rs` |
| OSC 8 hyperlinks | **the reference does not implement them** — no `b"8"` arm in `osc_dispatch` | — (our advantage; cheap) |
| URL detection | detect bare URLs in grid text | `grid_handler.rs:648-1354` |
| Inline images | iTerm2 OSC 1337, Kitty graphics | feature-flagged there; later for us |
| IME | composition must not corrupt the grid; marked-text path is platform-specific | `block_list_element.rs:4590`, `view.rs` (marked text behind a flag), `crates/octomusui/src/platform/mac/objc/host_view.m:441-476` |

### 3.9 Queries and replies

TUIs probe the terminal at startup and adapt. Without replies, several of them degrade or hang.

| Query | Reply | Reference |
|---|---|---|
| DA1 `CSI c` | `ESC[?62c` (VT220-class) | `A/grid/ansi_handler.rs:389` |
| DA2 `CSI > c` | `ESC[>{version}c` | `:389` |
| DSR 5 `CSI 5n` | `ESC[0n` (ready) | `:435` |
| DSR 6 `CSI 6n` | CPR: `ESC[<row>;<col>R`, viewport-relative | `:435` |
| XTVERSION `CSI > q` | `DCS > \| <name>(<version>) ST` | `:429` |
| CSI 14 t / 18 t | text-area size in pixels / in cells | `:1204`, `:1210` |
| DECRQM | real per-mode state (§3.4) | reference: only `?2026` |

### 3.10 Host → PTY input encoding (phase 0)

This is the half of the emulator we are missing entirely today, and it is independent of the parser.

Encoder dispatch order in the reference (`C/escape_sequences.rs:231-250`): Kitty CSI-u → function keys → Ctrl C0 → cursor keys → Meta prefix. Note their **key bindings in the app take precedence** over the encoder (`app/src/terminal/view/init.rs`), which is the right layering: the encoder is a pure function of (key, modifiers, mode), and the app may intercept before it.

| Key | Encoding |
|---|---|
| Arrows, Home, End | `CSI A/B/C/D/H/F`; `SS3 A/B/C/D/H/F` when `APP_CURSOR` is set. **Any modifier forces** `CSI 1;<mod><code>` |
| F1–F4 | `SS3 P/Q/R/S` |
| F5–F12 | `CSI 15~/17~/18~/19~/20~/21~/23~/24~` |
| F13–F20 | `CSI 25~/26~/28~/29~/31~/32~/33~/34~` |
| Insert / Delete | `ESC[2~` / `ESC[3~` |
| PageUp / PageDown | `CSI 5~` / `6~`; the reference only forwards these while a command is running |
| Backspace / Tab / Shift+Tab / Esc | `0x7f` / `0x09` / `CSI Z` / `0x1b` |
| Ctrl+letter | explicit table of C0 bytes (`KEYSTROKE_TO_C0_CODE`, `:470-483`), applied only when Ctrl is held with no other modifier; otherwise the byte comes from the OS text |
| Alt / Meta | `ESC` followed by the UTF-8 bytes of the key (`:566-593`) |
| Modifier numbers | xterm: 2 Shift, 3 Alt, 4 Shift+Alt, 5 Ctrl, 6 Ctrl+Shift, 7 Ctrl+Alt, 8 all (`:301-370`) |

Kitty keyboard protocol (`CSI = … u`, `> … u`, `< … u`, `? u`) with stack depth 4096 is a later phase; it must be gated per-application and the reference explicitly never uses CSI-u for F1–F12, arrows, Home/End, Insert/Delete or PageUp/PageDown (`:377-459`). One reference wart to avoid: `APP_KEYPAD` is defined in `C/mode.rs:12` but **never set**, and no parsed mode maps to it.

**Mouse** is phase 5. The reference always emits **SGR 1006** regardless of negotiated mode (`C/escape_sequences.rs:253-289`): left 0, right 2, left-drag 32, move 35, wheel 64/65. It has **no middle/back/forward buttons**, **never encodes modifiers**, tracks `UTF8_MOUSE` without implementing it, and does not parse `1015` (urxvt). Forwarding is gated by `should_intercept_mouse` (`app/src/terminal/alt_screen/mod.rs:11-35`) plus user settings. We should implement middle button and modifier encoding from the start.

### 3.11 OSC table

Reference: `osc_dispatch` `A/ansi/mod.rs:860-1305`, parameters split on `;`, terminator distinguished by `bell_terminated` (`:862`), unknown → `unhandled`.

| OSC | Meaning | Reply? |
|---|---|---|
| 0, 2 | set title | no |
| 4 | set palette entry — **reference bug:** it `return`s after the first successful pair (`:935`), so only the first pair of a multi-pair OSC 4 is applied | no |
| 7 | current working directory from a `file://` URI (feeds our directory pill) | no |
| 8 | hyperlink (start/end) — **absent in the reference** | no |
| 9 / 777 | desktop notifications (ConEmu / urxvt forms) | no |
| 10, 11, 12 | get/set fg / bg / cursor colour | **yes** — `OSC <code>;rgb:RR/GG/BB` when queried with `?` (`:976-1007`, `terminal_model.rs:2735-2748`) |
| 50 | cursor shape (`0/1/2` = block/beam/underline) | no |
| 52 | clipboard store/load, base64; `?` loads | **yes, async** (`:1029-1039`, reply `ansi_handler.rs:1167-1185`) |
| 104 | reset palette indices (all 256 by default) | no |
| 110 / 111 / 112 | reset fg / bg / cursor colour | no |
| 133 | semantic prompt markers (`A` prompt, `B` command start, `C` output start, `D` command end) — lets us know when a command runs | no |
| 1337 | iTerm2 inline images | no |

Everything else is ignored without breaking the parser. Our own in-band markers, if we need any, should use a private OSC number and stay out of the standard range.

## 4. The six hard parts

Ranked by how much rework a wrong first design costs. These are the parts to prototype before committing the data model.

1. **Reflow on resize.** It rewrites every stored row, so it forces the scrollback representation (byte-offset index over text chunks), the cursor representation (three cursor states to remap), and the wrap-flag semantics all at once. `C/index.rs:99-145` is the algorithm; the rest of `resize.rs` is bookkeeping to hand it the right input.
2. **Wide characters and their spacers.** Two cells per glyph, plus a third state at the row boundary, means every shift/erase operation has a repair step. Enumerate the operations and write the repair helper once; do not inline it per op.
3. **Deferred wrap.** Half the cursor sequences clear the flag and half do not, and the distinction is invisible until a TUI draws its right-hand border one column off.
4. **Combining marks and grapheme clusters.** The zero-width attachment rule (previous cell, minus spacer) is subtle, and the reference does not segment graphemes at all. Decide the storage cap and the overflow policy up front.
5. **Scrollback eviction vs anchored state.** Selection, search hits and any absolute row pointer must be re-anchored when rows are evicted. Funnel eviction through one signal.
6. **Alt screen lifecycle.** Save/restore, wipe-on-1049, cursor handoff, no-scrollback assertion, no-reflow resize, image eviction on exit. Small surface, many ordered steps, easy to get 90% right.

## 5. Verification

The reference's approach is good and we should adopt it in phase 6 — with one structural fix.

**Fixture format.** Four files per fixture, under a `ref_tests/` corpus (`~/Projects/warp-new/app/src/terminal/ref_tests/mod.rs`), adapted from `alacritty_terminal` (Apache-2.0 headers):

| File | Contents |
|---|---|
| `alacritty.recording` | the raw PTY byte stream |
| `size.json` | `rows`, `columns`, `cell_width_px`, `cell_height_px`, `padding_x_px`, `padding_y_px`, `pane_width_px`, `pane_height_px` |
| `grid.json` | the serialized grid (their `GridStorage`; `Cursor` is `#[serde(skip)]`) |
| `config.json` | `{"history_size": N}` |

The assertion is row counts plus cell-by-cell equality (char, fg, bg, flags, extra). **The cursor is not compared** — that is the structural fix: our fixture checker should assert cursor position, visibility, and the deferred-wrap flag too, or the entire §3.3 class of bugs is invisible to the suite.

Registration is a macro list (`ref_tests! { … }`, `mod.rs:47-88`). The reference registers 39 fixtures but has 40 directories — `row_reset` is an orphan that never runs. Our runner should enumerate the corpus directory instead of a hand-maintained list.

**Test infrastructure.** Their CI entry point is `script/presubmit`: format, an inline-test-module check (`script/check_no_inline_test_modules` — inline `mod tests {}` is banned), clippy with `-D warnings`, clang-format, wgslfmt, powershell lint, `cargo nextest run --no-fail-fast --workspace`, doc tests. Notably, their `.github/workflows/` contains no workflow that runs it — the suite is a local contract, not a gate. Our `release.yml` is manual `workflow_dispatch` only, and we have no nextest config. Adopting nextest plus a fixtures crate is the cheap part of their setup worth copying.

**Coverage gaps in the reference**, i.e. where we should do better: no fuzzing or property tests, no differential testing against a live emulator, no terminal benchmarks, and `vttest` only as recorded fixtures (`esctest` absent).

**Our ladder for phases 1–5, before the fixture corpus exists** (goble has no fixture infra today; terminal tests live in `app/src/integration_testing/*.rs` with `common.rs`, driving `render_element` headlessly):

- Phase 1–3: golden-grid unit tests that feed a byte string to the parser and assert cells, plus a recorded real session (`vim`, `htop`, `less`, `codex`) replayed through the emulator with its final grid compared.
- Phase 4: resize matrices — resize a wrapped grid wider and narrower repeatedly and assert the content round-trips.
- Phase 5: encode-direction tables (key + modifiers + mode → bytes) asserted directly; these need no PTY at all.

## 6. Feature decisions

`W` = reference behaviour, `G` = goble today. **take** = in scope for the phases above · **later** = deferred, keep the seam open · **skip** = deliberately not.

### Parser and sequences

| Feature | W | G | Decision |
|---|---|---|---|
| VT parser | `vte` fork | none | **take** — upstream crate |
| C0, CSI, OSC dispatch | full | CSI/OSC discarded | **take** |
| Charsets: ASCII + DEC Special Graphics | `control_sequence_parameters.rs:465-513` | none | **take** |
| UTF-8 designation (`ESC % G`), SS2/SS3, NRC sets | absent | none | **skip** |
| Synchronized output 2026 | `ansi/mod.rs:270-354` | none | **take** |
| C1 7-bit equivalents | partial | none | **take** |
| DCS / APC | product hooks, tmux | none | **later** |

### SGR and colour

| Feature | W | G | Decision |
|---|---|---|---|
| Core attributes + 16/256/truecolor | full | none | **take** |
| Dynamic colours OSC 10/11/12 (set **and** query) | `:976-1007` | none | **take** |
| OSC 4 palette, 104, 110–112 | `:926-940`, `:1042-1065` | none | **take** (fix the first-pair bug) |
| Double underline | `:527-529` | none | **take** |
| Curly/dotted/dashed underline, underline colour, overline | collapses / absent | none | **later** |
| Blink 5/6/25 | parsed then **dropped** | none | **take** — we implement it |

### Grid, scroll, resize

| Feature | W | G | Decision |
|---|---|---|---|
| Cell + flags + wide spacers | `cell.rs:144-150` | none | **take** |
| Combining marks per cell | `cell.rs:211-256` | none | **take** |
| Grapheme segmentation at write time | absent | none | **take** — we need it more than they did |
| Cursor addressing, DECSC/DECRC, tab stops, DECSTBM + origin, DECAWM, IRM, ICH/DCH/ECH/IL/DL/ED/EL, DECALN, RIS | `ansi/…` | none | **take** |
| Scrollback | flat storage, 50 000 rows | 2000 plain lines | **take** |
| Reflow on resize | reflow + legacy path behind a flag | none | **take**, one path only |
| Alt screen resize | in-place, no reflow | none | **take** |
| DECCOLM 80/132 | partial | none | **later** |
| "Clear instead of scroll" for CLI-agent TUIs | `grid_handler.rs:331-340` | none | **take** — see §9 / [`terminal-blocks.md`](terminal-blocks.md) |
| Per-block grids instead of one screen + scrollback | `blocks.rs`, `blockgrid.rs` | none | **take** — the block terminal is the product requirement, not a variant |
| Displayed-output row filtering | `displayed_output.rs`, `filtering.rs` | none | **skip** |

### Terminal surface

| Feature | W | G | Decision |
|---|---|---|---|
| Alt screen 47/1049 | full | none | **take** — required for any TUI |
| Cursor style DECSCUSR + OSC 50, title OSC 0/2 + stack | full | none | **take** |
| OSC 7 cwd | `:911-923` | none | **take** — feeds the directory pill |
| OSC 133 prompt markers | `:1070-1077` | none | **take** — command boundaries |
| Bell | yes | none | **later** |
| Bracketed paste, focus events, OSC 52 | full | none | **take** |
| Mouse 1000/1002/1003 + SGR 1006 | partial | none | **take** (add middle button + modifiers) |
| Mouse 1005/1007/1015/X10 | unused/absent | none | **skip** |
| Selection + copy | full incl. smart selection | none | **take** basic, **later** smart |
| Search / find | yes | none | **later** |
| OSC 8 hyperlinks | **absent** | none | **take** — we would be ahead |
| URL detection | yes | none | **later** |
| DA1/DA2, DSR, XTVERSION, CSI 14/18 t | full | none | **take** — TUIs probe these |
| DECRQM | only `?2026` | none | **take** properly |
| Kitty keyboard protocol | partial | none | **later** |
| Legacy key encoding (arrows/Home/End/F1–F20/meta) | full | **none at all** | **take, urgent** — phase 0 |
| iTerm2 images, Kitty graphics | flagged | none | **later** |
| Kitty unicode placeholder, Sixel | rejected / absent | none | **skip** |
| Ligatures | off by default | none | **skip** until text shaping exists |
| tmux control mode | flagged | none | **skip** |
| In-band shell/completion OSCs | pervasive | none | **skip** |

### Explicitly out of scope

The reference's entire **block layer** (`blocks.rs`, `block.rs`, `blockgrid.rs`, `header_grid.rs` — 9 600+ LOC), its secret obfuscation inside the grid (`grid/secrets.rs`, threaded through `GridHandler::new` at `grid_handler.rs:393-403`), its shell-hook session state machine, and its tmux wrapper. These are product surface, not emulation, and they are the main reason its emulator cannot be lifted as a crate.

## 7. Reference defects and deviations — do not inherit

| # | Defect | Where | What we do instead |
|---|---|---|---|
| 1 | DECRQM answers only `?2026`, and reports the performer's sync state rather than grid mode state | `A/ansi/mod.rs:1459-1483` | Report every implemented mode from the grid |
| 2 | OSC 4 applies only its first colour pair | `A/ansi/mod.rs:928-935` | Apply all pairs |
| 3 | SGR 5/6/25 parsed then dropped on the floor | `A/grid/ansi_handler.rs:962-966` | Implement blink |
| 4 | Colon-form `38:5:0:n` misreads the palette index | `C/ansi/control_sequence_parameters.rs:555` | Handle length-3 colon form |
| 5 | `CSI 1J` wipes all scrollback | `A/grid/ansi_handler.rs:805` | Decide explicitly; warn in docs if kept |
| 6 | Selection is only re-anchored on one eviction path; `flat_storage.clear()` does not bump `num_truncated_rows` | `blocks.rs:3480-3489`, `flat_storage/mod.rs` | One eviction signal for all anchored state |
| 7 | `?1047` / `?1048` unimplemented; `APP_KEYPAD` defined but never set | `control_sequence_parameters.rs:142-153`, `C/mode.rs:12` | Implement 1047/1048; wire keypad mode or drop the bit |
| 8 | DECSC/DECRC save only the cursor struct — not DECOM, DECAWM, GL/GR, the scroll region, or cursor shape | `A/grid/grid_storage.rs:30-49` vs xterm | Save the xterm set |
| 9 | `clear_viewport` walk-back loop is dead code (out-of-bounds seed) | `A/grid/ansi_handler.rs:1674` | Write it correctly or not at all |
| 10 | Mouse: no middle/back/forward buttons, modifiers never encoded, `1005` tracked but unimplemented, `1015` unparsed | `C/escape_sequences.rs:253-289` | Implement middle button + modifiers |
| 11 | Fixture corpus registers 39 of 40 directories — `row_reset` never runs; the cursor is not compared at all | `ref_tests/mod.rs:47-88` | Enumerate the directory; compare cursor + wrap flag |
| 12 | Big test suite, but no CI workflow runs it | `.github/workflows/` | Wire the suite into CI |

## 8. Tasks

- [ ] **Phase 0:** complete host→PTY key encoding — arrows, Home/End, PageUp/PageDown, Insert/Delete, F1–F20, Alt/Meta, Shift+Tab, xterm modifier form; assert with encode-direction tables.
Status of this list after the vendor + crate landing (§0): an item marked `[x]` exists in `crates/goble-terminal` and is covered by its tests. An item whose exit criterion is behaviour *in the app pane* stays `[ ]` even when the mechanism is in place, because the pane has not been switched over yet.

- [ ] Wire `TerminalSession::set_size` and stop hard-coding the PTY at 24×96 (`app/src/terminal.rs:509-514`, `:583`) — `Screen::resize` and reflow are done; the pane does not call them yet.
- [x] Add the upstream `vte` parser driver + a VT-only `Handler` trait — the parser driver is Alacritty's, reached through the crate's own `vte` re-export; our side implements `EventListener`, not `Handler`, so no wrapper layer is needed.
- [x] Replace `TerminalBuffer` with a real grid: 24-byte cell, fg/bg/flags, wide-char spacers, combining extras, row storage — `Screen` over the vendored grid; the pane still uses `TerminalBuffer`.
- [x] Define the renderer seam and paint the grid through it — `Screen::lines`/`content_lines`/`cursor` plus `ScreenCell`/`CellAttrs`/`ScreenColor`; a snapshot rather than the `GridView` trait this document first proposed (§0).
- [x] Implement cursor addressing, deferred wrap, erase/insert/delete, scroll regions, tab stops, DECAWM, IRM — from the emulator; only CUP and reset are asserted here, the rest is upstream's.
- [x] Implement SGR including 256/truecolor, colon form, and blink — asserted for 16-colour, 256-colour, truecolor, underline styles and the reset form.
- [x] Implement alt screen (47/1049) with the full lifecycle — asserted, including that the alt screen has no scrollback.
- [~] Implement the DEC private mode table and a correct DECRQM — the emulator answers mode queries; not asserted here.
- [x] Implement the query/reply set — clipboard read, colour report and text-area size arrive with their formatters (`ScreenQuery`), with clipboard read off by default.
- [x] Implement scrollback with a row cap — per block, `BLOCK_HISTORY` = 50 000 rows, oldest dropped at the cap; `content_lines` is the renderer's view of it.
- [x] Implement resize + reflow (one path) and the no-reflow alt-screen path — asserted, including that a shrink that reflows into more rows loses nothing.
- [~] Add bracketed paste, focus events, mouse reporting (1000/1002/1003 + SGR 1006 with modifiers), OSC 8, 7, 133, 52 — bracketed-paste mode, app-cursor mode, OSC 8, OSC 52 and window-title events are exposed; mouse, focus events, OSC 7 and OSC 133 are not.
- [ ] Add the fixture-based conformance harness (4-file format, directory-enumerated, cursor asserted) and adopt `cargo nextest` — upstream's own suite is one command away as an oracle: `scripts/fetch-alacritty-terminal.sh <dir>`.
- [ ] Fix the `row_reset`-style orphaning by construction: the runner enumerates the corpus.
- [ ] Replace or re-license the 42 icon assets copied from the reference tree — see [Appendix A](#appendix-a--licensing).
- [~] Build the block-terminal layer above this emulator — the model is in `blocks.rs` (lifecycle, feed routing, freeze, clear, per-block metadata) and the hook channel is in `hooks.rs`; the shell-side integration scripts that emit those hooks, prompt hiding, and the height sum-tree layout are not written.

## 9. Block terminal — what it changes here

The product is a **block terminal**: each command and its output is a discrete object, and the shell's own prompt is hidden ([`terminal-blocks.md`](terminal-blocks.md) has the map). That layer sits above this emulator, but it constrains it in five concrete ways:

1. **History is the block list, not grid scrollback.** Each block owns private grids with its own bounded row storage (`A/grid/grid_handler.rs:402-411`, `A/block.rs:583-586`); the alt screen keeps zero scrollback (`A/terminal_model.rs:1106-1111`). §3.7's design still applies, but per block rather than per session. **As built:** the bounded storage is the screen's own scrollback (`ScreenConfig::block`, `BLOCK_HISTORY` = 50 000 rows), so the two-tier model falls out of one `Config` field instead of a second storage type; `Screen::content_lines` reads history and screen as one contiguous body, which is what a block renders. The session-level scrollback people expect from a normal terminal is not what a block shows — the block list is.
2. **A hook channel is a first-class requirement.** The composition root must parse the shell-hook payloads (`InitShell`, `Bootstrapped`, `Precmd`, `Preexec`, `CommandFinished`, `Clear`, `InputBuffer`) out of DCS or OSC messages and emit lifecycle events. We use the DCS hex-JSON envelope (`A/ansi/mod.rs:838-846`) as the single channel. These are the "product callbacks" rule 2 of §2.3 keeps out of the *grid* — they belong to the one `Handler` impl.
3. **Byte batches need an end-of-batch signal.** The block layer recomputes block heights once per PTY read batch (`A/blocks.rs:3800-3808`), so the read loop must expose `on_finish_byte_processing`-style boundaries rather than letting the grid notify per row.
4. **Two clear modes beyond the standard set.** The reference adds `ClearMode::ResetAndClear` and `ClearMode::ActiveBlock` to the standard `Below/Above/All/Saved` (`crates/octomus_terminal/src/model/ansi/control_sequence_parameters.rs:190-198`). `vte::ansi::ClearMode` does not have them, so we need our own extension point for clear modes — an enum of ours that wraps the upstream one, not a fork of `vte`.
5. **OSC 133 is required, not optional.** `A`/`B`/`P;k=i|r` delimit the prompt so its bytes can be routed to a hidden grid (`A/ansi/mod.rs:1070-1077`). Without prompt demarcation there is no way to hide the prompt without also hiding the command line.

Nothing else in this document changes: cells, cursor, modes, edit operations, SGR, alt screen and input encoding are identical in a block terminal, and the phase order of §1 stands. Phase 0 (key encoding) and the block layer's phase B0 (shell integration) are independent and can proceed in parallel.

## Appendix A — Licensing

This is an implementation constraint, not the point of the doc; recorded so nobody trips over it later.

- The reference workspace declares `license = "AGPL-3.0-only"` (`~/Projects/warp-new/Cargo.toml:25-28`), inherited by `crates/octomus_terminal/Cargo.toml:6`. **The crate named after the terminal is AGPL-3.0-only.**
- `README.md:56-60` carves out only the UI framework: `octomusui_core` and `octomusui` are MIT (`crates/octomusui/Cargo.toml:7`, `crates/octomusui_core/Cargo.toml:7`).
- Several files carry an "adapted from the alacritty_terminal crate under the Apache license" header (e.g. `C/model/grid/cell.rs:1-2`, `C/model/mode.rs:1-2`, `C/model/ansi/control_sequence_parameters.rs:1-2`) with text at `C/model/LICENSE-ALACRITTY` (Apache-2.0, "Copyright 2020 The Alacritty Project"). The adaptations themselves sit under the AGPL workspace license, so do not assume they are safe to lift.
- Other files carry no header at all (`C/lib.rs`, `C/model/escape_sequences.rs`, `A/terminal_model.rs`); `grep -i spdx` finds nothing, so provenance is not machine-readable.
- Some dependencies are private git crates that the reference's own `about.toml:36-42` documents as having **no license yet**.

**Alacritty, by contrast, is safe to reuse** (see §2.4): the repository is dual-licensed Apache-2.0 OR MIT, there is no `NOTICE` file to carry, and its dependency tree is entirely permissive (`arrayvec`, `base64`, `bitflags`, `home`, `libc`, `log`, `parking_lot`, `polling`, `regex-automata`, `unicode-width`, `vte`, plus `rustix`/`rustix-openpty`/`signal-hook` on Unix and `piper`/`miow`/`windows-sys` on Windows) — no copyleft is dragged in. Take it from `github.com/alacritty/alacritty` at a pinned rev, not from the reference tree.

**Verdict:** study warp-new, map it, reimplement. Take code only from permissive upstreams (Alacritty, `vte`) and discharge the attribution duties in a `THIRD-PARTY-NOTICES.md`. The mapping in this doc is the deliverable — it is what makes our implementation professional without taking on the AGPL.

**Discharged.** `THIRD-PARTY-NOTICES.md` records the vendored `alacritty_terminal` 0.26.0 (Apache-2.0), the SHA-256 of the published archive, and the two deliberate deletions (`tests/`, `Cargo.lock`); `vendor/alacritty_terminal/LICENSE-APACHE` ships the license text next to the code, and `scripts/fetch-alacritty-terminal.sh` both checks that checksum and diffs the vendored tree against the published archive. Nothing was taken from `~/Projects/warp-new`, which stays a read-only reference.

The one genuinely reusable piece is the MIT UI framework: `octomusui` / `octomusui_core` (fonts, platform, rendering, windowing). Those are relevant to `goble-ui`, not to the emulator, and are a separate decision (see [`README.md`](README.md) design direction).

**Outstanding asset exposure:** `crates/goble-ui/assets/icons/` holds **42 of 60 SVGs byte-identical** to files under `~/Projects/warp-new/app/assets/` (`check.svg`, `close.svg`, `gear.svg`, `message-chat-square.svg`, and 38 more). That tree is on the AGPL side, and no third-party icon attribution exists there (`app/assets/` carries license files only for `windows/`). Either prove the icons come from a permissively licensed upstream set or replace them. Cheap now, expensive later.
