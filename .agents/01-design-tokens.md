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
