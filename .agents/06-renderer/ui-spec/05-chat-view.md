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

- The rich input is the shared `ChatComposer`: the context pills (harness, working directory, git branch) above the editor, the action row (model, stop) below it, and — above all of them — the instruction strip (`R8`, current).
- Centre: the draft editor with placeholder "Ask anything...". No box behind it: the card keeps its gutters only, on the square workspace surface.
- Right of the action row: `Stop` while this pane's turn is in flight.
- Height hugs its content (48 px editor, capped at 160 px), padding `md`.

## The instruction strip

- What the keys do, drawn as the first row of the input, above the context pills and the editor. Element: `crates/goble-ui/src/elements/shortcut_hints.rs` (`ShortcutHint` / `ShortcutHints`); the composer takes the entries with `with_hints` and `ChatView` forwards them with `with_composer_hints`, so both surfaces that mount the rich input (the chat pane and the shell pane's bar) draw the same widget.
- The shape is warp-new's shortcuts view: a cap per key of the chord (`⌘` and `K` are two caps, 2 px apart), then the name 4 px after them, in `Muted` at 11 px. Caps are square (`SurfaceRaised` fill, 1 px `Border` stroke), like every other control.
- The entries are the app's, in `app/src/ui/shortcut_hints.rs`: grok-build's prompt-focused hints, worded as grok-build words them, and kept only where this GUI binds the chord. The agent input draws `↵ send` (or `↵ queue` while this pane's turn is in flight, grok-build's own relabel), `⌘↵ new conversation`, `! shell`, `⌘K commands` and `⌘⇧W tasks`; the shell input draws `↵ run`, `⌘↵ agent`, `⌘K commands`, `⌘⇧W tasks` and no `!`, because a shell pane's input is a command already.
- Not shown, because nothing here answers them: grok-build's `newline`, `mode`, `cancel`, `yolo`, `todos`, `sessions`, `extensions`, `send to bg`, `multiline` and `stash`. (`queue` is not a separate entry in either program: it is what grok-build relabels its send hint to while a turn runs, and this strip relabels the same way.)
- An empty list draws nothing at all, so a surface with no instructions pays neither the row's height nor the column's spacing.

## Files

- `crates/goble-ui/src/views/chat_view/`
- `crates/goble-ui/src/elements/chat_message_bubble.rs`
- `crates/goble-ui/src/elements/chat_composer/`
- `crates/goble-ui/src/elements/shortcut_hints.rs`
