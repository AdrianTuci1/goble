# 01 — Design Tokens

There are two palettes, each owned by the client it styles:

- `crates/goble-ui/src/theme.rs` is the source of truth **for the Rust desktop app**. Its `ColorToken` enum is the only colour API components should use.
- `apps/common/src/lib/theme.ts` is the source of truth **for the TS clients** (`apps/web` and `apps/mobile`). Components consume the exported `palette` by name.

The two files are edited independently: a token change belongs in the file for the client it targets. The Rust theme also defines light, midnight and preset schemes; **only the dark palette exists in the TS clients (there is no light variant)**.

## Color tokens — Rust desktop (dark)

Use `ColorToken` everywhere; do not hardcode hex values in components.

| Token | Dark | Usage |
|-------|------|-------|
| `Bg` | `#0e0e0e` | Window background |
| `Surface` | `#161616` | Panels, sidebars, cards |
| `SurfaceRaised` | `#1e1e1e` | Inputs, composer, raised cards |
| `Border` | `#262626` | Dividers, borders |
| `Text` | `#e8e8e8` | Primary text |
| `Muted` | `#8e8e8e` | Timestamps, descriptions, placeholders |
| `Hover` | `#1e1e1e` | Hover background |
| `Selected` | `#2a2a2a` | Selected card / menu item |
| `Focus` | `#19aad8` | Focused input ring and caret |
| `Accent` | `#9a9a9a` | Active buttons, links, unread dots (neutral grey) |
| `Success` | `#10b981` | Running / ok status |
| `Warning` | `#f59e0b` | Warnings |
| `Path` | `#ff9e64` | Tool row's file path |
| `Command` | `#e0af68` | Tool row's shell command |
| `Error` | `#ef4444` | Errors, delete |
| `Badge` | `#e01e5a` | Notification badges |

## Color tokens — TS clients (dark)

Use `palette.*`; do not hardcode hex values in components.

| Token | Dark | Usage |
|-------|------|-------|
| `bg` | `#0e0e0e` | Window background |
| `surface` | `#161616` | Panels, sidebars, cards |
| `raised` | `#1e1e1e` | Inputs, composer, raised cards |
| `overlay` | `#242424` | Popovers, menus, segmented controls |
| `border` | `#262626` | Decorative dividers and borders |
| `borderInteractive` | `#6a6a6a` | Border that is a control's only affordance (WCAG 3:1) |
| `borderStrong` | `#343434` | Emphasised decorative border |
| `text` | `#e8e8e8` | Primary text |
| `muted` | `#a0a0a0` | Timestamps, descriptions, placeholders |
| `subtle` | `#8b8b8b` | 10–12 px footnotes, helper labels, disabled glyphs |
| `primary` | `#f1f1ef` | Primary foreground on accent surfaces |
| `accent` | `#19aad8` | Active buttons, links, unread dots |
| `success` | `#10b981` | Running / ok status |
| `warning` | `#f59e0b` | Warnings |
| `error` | `#f87171` | Errors, delete |
| `path` | `#ff9e64` | Tool row's file path |
| `command` | `#e0af68` | Tool row's shell command |

## Contrast

Normal text in the TS palette must clear WCAG 2.1 AA, 4.5:1. `scripts/check-contrast.mjs` (`npm run a11y:contrast`) parses `apps/common/src/lib/theme.ts` and enforces:

- `text`, `muted`, `subtle`, `accent`, `success`, `warning`, `error` ≥ **4.5:1** against each of `bg`, `surface`, `raised`, `overlay`;
- the tinted pairs `error`/`errorSoft`, `success`/`successSoft`, `warning`/`warningSoft`, `accent`/`accentSoft` and `text`/`accentSoft` ≥ **4.5:1**;
- the destructive button's dark label on its red fill (`bg` on `error`) ≥ **4.5:1**;
- `borderInteractive` ≥ **3.0:1** against each of `bg`, `surface`, `raised`. `border` is decorative and exempt.

The script fails on a missing expected token instead of skipping it, prints every `token × background` ratio, and names each failing pair. CI runs it in the `js / typecheck + web build` job, and `scripts/check-contrast.mjs` is in that job's `paths` triggers. This gate is for the TS dark palette only; the Rust desktop palette is not read by the script.

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
