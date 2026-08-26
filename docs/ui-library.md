# Goble UI Library

This document explains how the Goble desktop UI is implemented with the
`goble-ui` crate and how to build new screens with it. It is written for
contributors who need to add or change UI.

## Crate layout

The UI is split into a library and an in-app builder:

- `crates/goble-ui` — the **library**. Platform-agnostic elements (buttons,
  inputs, lists, chat, sheets), the theme, text measurement/rasterization, and
  the winit/wgpu window backend. This crate is compiled into the app binary.
- `app/src/ui` — the **in-app shell**. `build_ui` and the snapshot/action
  types (`UiSnapshot` / `UiActions` / `AiSnapshot` / `AiActions`) plus the
  per-screen modules (`chat`, `sidebar`, `shell`, `crons`, `connectors`,
  `vault`, `model_form`) live here, in the `goble-app` crate, mirroring how
  warp-new builds its windows in `app`. There is no cdylib/ABI boundary.

The flow is:

1. The host app owns all state (conversations, drafts, flags).
2. Every frame it calls `build_ui` with a **snapshot** of that state.
3. `build_ui` builds a brand-new element tree from the snapshot.
4. The engine lays out, paints, and dispatches events on that tree.

Because the tree is rebuilt every frame, elements must be cheap to build and
must **not** own important state themselves — the snapshot is the source of
truth. Input elements keep transient UI state (focus, caret) and push changes
back through callbacks.

## The element model

Everything is an [`Element`](crate::elements::Element):

- `layout(constraint, ctx, app) -> Vector2F` — compute the size from a
  `SizeConstraint` (min/max box).
- `paint(origin, ctx, app)` — draw into the `Renderer`.
- `dispatch_event(event, ctx, app) -> bool` — handle mouse/keyboard; return
  `true` when the event was consumed.
- `size()` / `origin()` — cached layout results used by parents and hit
  testing.

Builders follow a consistent pattern: `Element::new(...)` then
`.with_*(...)` modifiers, ending with `.finish()` which boxes the element:

```rust
let button = Button::new(Text::new("Run").finish())
    .with_variant(ButtonVariant::Primary)
    .with_on_click(move || run())
    .finish(); // -> Box<dyn Element>
```

## Layout model

There is no flexbox engine like in the web; layout is a single-pass
constraint system with a few primitives.

### `SizeConstraint`

Every element receives `SizeConstraint { min, max }` (two `Vector2F`s).
A **tight** constraint (`min == max`) forces an exact size; a **loose**
constraint (`min = 0`, `max = limit`) lets the element choose its size.

### `Flex` — rows and columns

`Flex::row()` / `Flex::column()` lays children along one axis:

- `with_main_axis_size(MainAxisSize::Max)` — the flex fills the available
  main-axis space (the window width/height). With `Min` it hugs content.
- `with_cross_axis_alignment(CrossAxisAlignment::Stretch)` — children are
  given the full cross-axis size as their **maximum**. Important: `Stretch`
  only bounds the maximum; it does **not** force the child to fill.
- `with_main_axis_alignment(MainAxisAlignment::SpaceBetween)` — pushes
  children to the two ends, splitting leftover space between them.
- `with_spacing(n)` — gap between children.

### `Container` — background, border, padding

```rust
Container::new(child)
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_border(app.theme.color(ColorToken::Border).into())
    .with_corner_radius(radius)
    .with_padding(EdgeInsets::uniform(spacing))
    .finish()
```

`EdgeInsets::new(left, top, right, bottom)` is the parameter order.

### `Expanded` and `Spacer` — taking leftover space

- `Expanded::new(child)` (Flutter-style `flex: 1`) gives the child all
  remaining main-axis space with a tight constraint. Use it to pin a list to
  the middle of a column with a footer at the bottom.
- `Spacer::new()` is the zero-painting version: it claims leftover space in a
  `MainAxisSize::Max` parent and pushes following siblings to the far edge.

```rust
// Header at top, transcript fills the middle, composer pinned to the bottom.
Flex::column()
    .with_main_axis_size(MainAxisSize::Max)
    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
    .with_child(header)
    .with_child(Expanded::new(scrollable_list).finish())
    .with_child(composer)
    .finish()
```

### The three rules that cause most layout bugs

1. **`Container` hugs content.** `Container` returns `child + padding`, it
   does not fill `constraint.max`. To make a bar span the full width, put a
   `MainAxisSize::Max` `Flex` (or an `Expanded`) inside it, or wrap the
   container in a `MainAxisSize::Max` flex.
2. **A `MainAxisSize::Max` column that owns the window must give its body an
   `Expanded`.** Otherwise the body keeps its intrinsic height, overflows past
   the bottom, and footers (composer, status bars) are pushed off-screen.
   Example: the shell column wraps `SidebarLayout` in `Expanded` so the main
   area gets `window_height - topbar_height`.
3. **`Stretch` sets a max, not a size.** A stretched `TextInput`/`SearchInput`
   inside a column still hugs its text unless its inner row is
   `MainAxisSize::Max`. The `SearchInput` does this internally; `TextInput`
   relies on being wrapped in `Expanded`/tight constraints where full width
   is desired.

## Text

`Text` measures with the bundled Roboto family via the font atlas and wraps
at `constraint.max.x`:

```rust
Text::new("Hello")
    .with_font_size(15.0)          // default is 15.0
    .with_theme_color(ColorToken::Text, app)
    .with_max_lines(1)             // clamp to a single line (inputs)
    .with_weight(FontWeight::Medium)
    .finish()
```

`Label` is the uppercase muted caption used for section headers
(`LabelSize::Xs` = 12px, `LabelSize::Sm` = 13px).

The font atlas (`platform/text_atlas.rs`) rasterizes glyphs into a texture.
Quad height is derived from the font line box, so multi-line wrapped text
gets a correctly sized quad instead of being clipped to one line.

## Icons and avatars

- `Icon::new("search")` looks up an SVG embedded at compile time in
  `platform/icon_atlas.rs` (source SVGs live in `crates/goble-ui/assets/icons`).
  To add an icon: drop the SVG in `crates/goble-ui/assets/icons/`, add it to
  the `icon_bytes!` macro, and reference it by name.
- `Avatar::new("Ada Lovelace").with_size(28.0)` renders initials on a
  rounded background. Defaults to the accent color; pass explicit
  `with_theme_background` / `with_theme_foreground` for neutral grays.

## Theme

`Theme` (dark/light/midnight) holds colors, spacing, radius, and density.
Access resolved values through `app.theme`:

- `app.theme.color(ColorToken::Surface)`
- `app.theme.spacing_px(SpacingToken::Md)` (12px; Xs 4 / Sm 8 / Md 12 / Lg 16 / Xl 24)
- `app.theme.radius_px()` (8px default)

The design is intentionally gray/black: the blue accent resolves to a neutral
gray (`0x9a9a9a`), and UI highlights use `Selected`/`Hover`/`Muted`.

## Inputs

- `TextInput` — single-line text box (bordered, padded).
- `SearchInput` — `TextInput` with a leading search icon; its inner row is
  `MainAxisSize::Max`, so it fills the width of a stretched column.
- `TextArea` — multiline input (used by the composer).
- All inputs are single-line-capped (`with_max_lines(1)`) and push changes
  through `with_on_change`, focus through `with_on_focus_change`.

## Composing a full screen (checklist)

1. **State snapshot**: add fields to `UiSnapshot` (or `AiSnapshot`) in
   `app/src/ui/mod.rs`; add matching callbacks to `UiActions`.
2. **Builder module**: create `app/src/ui/<screen>.rs` with a
   `build_<screen>(app, state, actions) -> Box<dyn Element>` function. Compose
   primitive elements; never store state in elements.
3. **Wiring**: call the builder from `shell::build_main` (or a sheet/drawer in
   `app/src/ui/mod.rs::build_ui`).
4. **Layout checks**: the top-level column is `MainAxisSize::Max` +
   `Stretch`; any area that must fill leftover height is wrapped in
   `Expanded`; full-width bars contain a `MainAxisSize::Max` row.
5. **Tests**: add a layout smoke test (non-zero size) next to the element.

## Dev loop

- There is no live hot reload. `scripts/dev-ui.sh` builds and runs
  `goble-app`; with `cargo-watch` installed it rebuilds and restarts the app
  whenever any `app` or `crates` source file changes.
- Editing the snapshot/action structs is fine — there is no ABI boundary, so a
  normal rebuild picks everything up.

## macOS specifics

- The window uses the **real OS titlebar**: `titlebar_transparent(true)`,
  `title_hidden(true)`, `fullsize_content_view(true)` (see
  `platform/window.rs`). The app's topbar doubles as the titlebar background;
  traffic lights stay real and overlay the top-left.
- The topbar leaves a 76px left inset for the traffic lights and drops its
  vertical padding so the 36px toolbar buttons align beside them.
- Everything else (sidebar, chat, sheets) is platform-independent.

## Common patterns by example

Right-align a group of actions in a header:

```rust
Flex::row()
    .with_main_axis_size(MainAxisSize::Max)
    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
    .with_cross_axis_alignment(CrossAxisAlignment::Center)
    .with_child(identity)
    .with_child(actions_row) // small row of buttons
    .finish()
```

Pin the composer to the bottom of the chat area (see `views/chat_view.rs`):

```rust
Flex::column()
    .with_main_axis_size(MainAxisSize::Max)
    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
    .with_child(header)
    .with_child(Expanded::new(message_area).finish())
    .with_child(composer)
    .finish()
```

Make a full-width flat bar (see `app/src/ui/chat.rs`):

```rust
Container::new(
    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(...)
        .finish(),
)
.with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
.finish()
```
