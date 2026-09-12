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

- Left attach button: `+` or `paperclip` icon inside a circle / subtle button.
- Center: rounded input field with placeholder "Message {name}...".
- Right send button: `send` / arrow icon, `Accent` background when text is non-empty.
- Height ~48px, padding `md`, background `Surface`.

## Files

- `crates/goble-ui/src/views/chat_view.rs`
- `crates/goble-ui/src/elements/chat_message_bubble.rs`
- `crates/goble-ui/src/elements/chat_composer.rs`
