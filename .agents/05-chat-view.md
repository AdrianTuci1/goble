# 05 — Chat View (center)

## Structure

```
+---------------------------------------+
| [Avatar] Chief of Staff        [≡]  |
+---------------------------------------+
|                                       |
|  +----------------------------+       |
|  | message bubble             |       |
|  +----------------------------+       |
|                                       |
|           +----------------+          |
|           | agent pill     |          |
|           +----------------+          |
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
- Bubble background: `SurfaceRaised` with `Default` radius.
- Text color `Text`.
- Small agent pills (e.g., "ask Agent for ...") use `Rounded` pill with `Hover` background.

## Composer

- Left attach button: `+` or `paperclip` icon inside a circle / subtle button.
- Center: rounded input field with placeholder "Message {name}...".
- Right send button: `send` / arrow icon, `Accent` background when text is non-empty.
- Height ~48px, padding `md`, background `Surface`.

## Files

- `crates/goble-ui/src/views/chat_view.rs`
- `crates/goble-ui/src/elements/chat_message_bubble.rs`
- `crates/goble-ui/src/elements/chat_composer.rs`
