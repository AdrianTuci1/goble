# 01 — Design Tokens

Source of truth: `crates/goble-ui/src/theme.rs`.

## Color tokens

Use `ColorToken` everywhere; do not hardcode hex values in components.

| Token | Dark | Light | Usage |
|-------|------|-------|-------|
| `Bg` | `#0f1115` | `#f6f7f9` | Window background |
| `Surface` | `#181b21` | `#ffffff` | Panels, sidebars, cards |
| `SurfaceRaised` | `#21252e` | `#f3f4f6` | Inputs, composer, raised cards |
| `Border` | `#2a2e36` | `#e2e4e9` | Dividers, borders |
| `Text` | `#e4e6eb` | `#1f2937` | Primary text |
| `Muted` | `#8b949e` | `#6b7280` | Timestamps, descriptions, placeholders |
| `Hover` | `#21252e` | `#f3f4f6` | Hover background |
| `Selected` | `#2a2e36` | `#e5e7eb` | Selected card / menu item |
| `Accent` | `#2563eb` | `#2563eb` | Active buttons, links, unread dots |
| `Success` | `#10b981` | `#10b981` | Running / ok status |
| `Warning` | `#f59e0b` | `#f59e0b` | Warnings |
| `Error` | `#ef4444` | `#ef4444` | Errors, delete |
| `Badge` | `#e01e5a` | `#e01e5a` | Notification badges |

## Fonts

- Primary UI: **Roboto** (Regular / Medium / Bold) bundled in `crates/goble-ui/assets/fonts/roboto/`.
- Monospace: **Hack** bundled in `crates/goble-ui/assets/fonts/hack/`.
- `Theme.font` gets a new `Roboto` variant; default is `Roboto`.

## Spacing

Base grid from `Spacing::default()`:

- `xs` = 4
- `sm` = 8
- `md` = 12
- `lg` = 16
- `xl` = 24

Apply `theme.density_factor()` at runtime.

## Radius

- `Sharp` = 0
- `Default` = 8
- `Rounded` = 14

Cards, inputs, and buttons use `Default`. Small pills use `Rounded`.

## Component rules

- Every card has a rounded `Surface` or `Hover`/`Selected` background.
- Text color defaults to `Text`; timestamps and descriptions use `Muted`.
- Borders and dividers use `Border` at 1px.
- Active / toggled controls use `Accent`.

## warp-new parity

`ColorToken` is the reduced catalog of warp-new's theme. The mapping is
deliberate — use the table below rather than hardcoding hex.

| `ColorToken` | warp-new | Usage |
|--------------|----------|-------|
| `Bg` | `background()` | Window / transcript background |
| `Surface` | `surface_1` | Panels, sidebars, message blocks, composer bar |
| `SurfaceRaised` | `surface_2` | Inputs, raised cards, tool/command sub-surface |
| `Border` | `outline()` | Dividers, borders |
| `Text` | `main_text_color` | Primary text |
| `Muted` | `sub_text_color` | Secondary text, timestamps, placeholders |
| `Hover` | button hover overlay | Hover background |
| `Selected` | `block_selection_color` | Selected card / menu item |
| `Accent` | `accent()` | Active buttons, links, unread dots |
| `Success` | `ui_green_color` | Running / ok status |
| `Warning` | `ui_warning_color` | Warnings |
| `Error` | `ui_error_color` | Errors |

Chat + composer rules (see `ChatMessageBubble` / `ChatComposer`):

- A message block sits on `Surface` (`surface_1`); user rows use `SurfaceRaised` (`surface_2`).
- A tool/command block is a **raised card** (`surface_2`): full-width, 1px
  `surface_2` border, radius 8, leading status icon + mono body. Tool
  invocations render as these cards above the assistant prose; a tool result
  renders as the same card style via `TerminalBlock`.
- The composer is a **floating card** (`SurfaceRaised`/`surface_2` + 1px
  `Border` outline + radius 8, side/bottom gutters) with an 80px editor; its
  footer buttons (model, profile, attach, stop) are `Surface`/`surface_1` pills
  that brighten to `SurfaceRaised`/`surface_2` on hover, with a 1px `Border`
  and `Muted` icons (mirrors warp-new's `AgentInputButton`).

## Streaming to the renderer

Tool-call status is **persisted and re-read** (`refresh_messages` after
`chat:updated`), so chips + result blocks appear when the harness writes them.
Live streaming of tool-call start/finish events mid-turn is not yet forwarded to
the renderer (warp-new streams this via `UpdatedStreamingExchange`).
