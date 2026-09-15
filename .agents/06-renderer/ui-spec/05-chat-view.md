# 05 — Chat View (center)

## Structure

```
+---------------------------------------+
| [Avatar] Chief of Staff        [≡]  |
+---------------------------------------+
|                                       |
| message from the user                 |
| (a square SurfaceRaised band,         |
|  pane-wide)                           |
|                                       |
| the agent's own reply                 |
| (no box: no fill, no border, no       |
|  radius, pane-wide)                   |
|                                       |
+---------------------------------------+
| [+]  Message Chief of Staff    [➤]  |
+---------------------------------------+
```

## Header

- Left: selected conversation `Avatar` + name in medium weight text.
- Right: icon button to toggle the right chat-sidebar (`≡` or `sidebar` icon).
- Height ~52px, bottom border `Border`, background `Surface`.

## Message content

- Reuse `ChatMessageBubble` and `GroupChatMessageGroup`.
- Sender avatar on the left of each group.
- A transcript row is full-width and square, not a pill: the agent's own reply paints no box at all (no background, no border, no corner radius) and the user's own message is a `SurfaceRaised` band with square corners. Both span the pane and carry `md` (12 px at the default density, 10–15 px across the density range) of padding at both edges, so no line of text touches the pane's border. This holds for every row the agent's turn draws — prose, reasoning, a tool call in any fold and any status, a sub-agent, the interruption bands, the footer — and it is locked by tests rather than asserted by hand ([`agent-parser.md`](../agent-parser.md) §12).
- Text color `Text`.
- Small agent pills (e.g., "ask Agent for ...") use `Rounded` pill with `Hover` background. This is the composer's own quick-action control, not the agent's row: nothing the agent's transcript draws is ever wrapped in one.

## Composer

- The rich input is the shared `ChatComposer`, one column: the context row (the harness, working-directory and branch pills, plus attach) above the editor, then the editor, then one action row under it holding the model, stop while a turn streams and the modal-editing badge, left to right, then — only for the entries a host keeps inside its input — the instruction strip (`R15`, `R24`, current). The composer's own hints are its last row; everything else about the input is unchanged from warp-new's order.
- The instruction strip is the pane's, drawn over the input: the order in the pane's column is transcript → instructions → separator → [the slash menu, while the draft is a command] → composer (context row → editor → action row). The separator line is the input's own top edge, drawn by `ChatView` under the strip, and the instructions describe the editor below them (`R24`). The one exception is the shell bar's `⌘↵ new conversation`, which stays inside the input, under the editor, because it belongs to the command line being typed there (see *The instruction strip*); a pane whose host passes no instructions draws the separator only.
- The input block **hugs its content and pins to the bottom**: the transcript column claims the height it was given (`MainAxisSize::Max`) and the transcript is the one `Expanded` child, so a conversation longer than the pane scrolls under the input instead of carrying it out of the view (`R15`).
- Centre: the draft editor with placeholder "Ask anything...". No box behind it: the card keeps its gutters only, on the square workspace surface.
- The editor's caret is a `TextArea` beam that never hides what it covers: the block caret (modal editing's normal and visual modes) draws the character it sits on in `ColorToken::Bg` inside its accent cell, and the underline caret keeps the character in `Text` with the accent bar under it, so a letter under the cursor stays readable in every mode. A press inside the editor puts the beam where the pointer landed (`TextArea::index_at_offset` measures the character boundaries of what is drawn and takes the nearest), and the editor reports the full width it was given (`with_full_width(true)`) so the hit box is the line the user sees, not just the ink (`R19`).
- Height hugs its content (48 px editor, capped at 160 px), padding `md`.
- The working-directory pill's menu is **capped and scrollable**, so a directory deeper than the window does not open a panel taller than the pane it opens into: the composer is handed an app-owned `PanelScroll` (`with_dir_menu_scroll`) and the menu is built with `PopupMenu::with_max_visible_rows(MENU_MAX_VISIBLE_ROWS)` — 8 rows, 270 px at the default density (`crates/goble-ui/src/elements/popup_menu.rs`). The offset lives in the app (beside the menu's open flag) because the tree is rebuilt every frame, and the row the offset was last moved to reveal is remembered with it so a wheel scroll away from the highlighted row is not undone by the next frame's reveal. A `PopupMenu` that asks for no cap keeps its natural height, which is every other menu in the app.

## The instruction strip

- What the keys do, drawn over the input's separator, as the last row of the pane's content. Element: `crates/goble-ui/src/elements/shortcut_hints.rs` (`ShortcutHint` / `ShortcutHints`); `ChatView` draws the strip it was handed with `with_composer_hints` between the transcript and the separator, and `ChatComposer::with_hints` draws the entries a host keeps *inside* the input, as its own last row under the action row (`R24`).
- The shape is warp-new's shortcuts view: a cap per key of the chord (`⌘` and `K` are two caps, 2 px apart), then the name 4 px after them, in `Muted` at 11 px. Caps are square (`SurfaceRaised` fill, 1 px `Border` stroke), like every other control.
- A key the bundled faces have no glyph for is drawn from the icon set, never as its own character: `key_icon(key)` maps `⌘`→`key-command`, `⇧`→`key-shift`, `⌥`→`key-option`, `⌃`→`key-control`, `↵`/`⏎`→`key-return`, `⌫`→`key-delete`, `⇥`→`key-tab` and the four arrows, and `key_has_icon(key)` is the same question for a caller that asserts a cap's ink. Every remaining cap is ASCII or a symbol the faces really cover (`font_covers`), so no cap can paint the font's `.notdef` box — which is what a dotted line above or below the text was (`R16`). The same rule covers the in-flight cues: the topbar's live indicator and the pane's turn-status footer mark their count with `Icon::new("diamond")`, not a `◆` character.
- The strip **wraps** rather than overflowing: it is a `Wrap::row`, and whole entries move to a second line when the pane is too narrow for them, instead of being drawn past the pane's edge. The same element holds the footer row, which wraps the same way.
- The entries are the app's, in `app/src/ui/shortcut_hints.rs`: grok-build's prompt-focused hints, worded as grok-build words them, and kept only where this GUI binds the chord. The agent pane draws `↵ send` (or `↵ queue` while this pane's turn is in flight, grok-build's own relabel), `⌘↵ new conversation`, `! shell`, `⌘K commands` and `⌘⇧W tasks` over its separator. The shell bar draws only what it answers — `⌘↵ new conversation`, because Cmd/Ctrl+Enter is the one gesture that leaves the shell for the agent (Enter at that bar runs the draft as a command, so there is no prefix and no second meaning to explain) — and it draws it inside the input, under the editor: it is the exception named above, and the only instruction a shell pane shows.
- Not shown, because nothing here answers them: grok-build's `newline`, `mode`, `cancel`, `yolo`, `todos`, `sessions`, `extensions`, `send to bg`, `multiline` and `stash`. (`queue` is not a separate entry in either program: it is what grok-build relabels its send hint to while a turn runs, and this strip relabels the same way.)
- An empty list draws nothing at all, so a surface with no instructions pays neither the strip's height nor the column's spacing.

## Files

- `crates/goble-ui/src/views/chat_view/`
- `crates/goble-ui/src/elements/chat_message_bubble.rs`
- `crates/goble-ui/src/elements/chat_composer/`
- `crates/goble-ui/src/elements/shortcut_hints.rs`
