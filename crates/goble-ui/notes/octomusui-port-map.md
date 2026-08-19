# Goble UI — port map from `octomusui` / `octomusui_core`

## Decizii de principiu

- **Nu copiem codul sursă direct** pentru că `octomusui_core` depinde de crate-uri AGPL/specifice Warp (`octomus_util`, `markdown_parser`, `settings_value`, logica avansată de text etc.).
- **Reimplementăm un framework minimal inspirat din Warp**, păstrând API-ul familiar (`Element`, `Flex`, `Stack`, `Container`, `UiComponentStyles`).
- **Dependințele externe** sunt înlocuite cu crate-uri generice de pe crates.io:
  - `pathfinder_geometry` / `pathfinder_color` → `euclid` + tipuri proprii (`Vector2F`, `RectF`, `ColorU`).
  - `pathfinder_color::ColorU` → `palette` pentru conversia sRGB→linear.
  - `winit`/`wgpu` rămân pentru fereastră și renderer (deja în workspace).

## Tipuri reimplementate

| Warp (`octomusui_core`)              | Goble (`goble-ui`)                              | Fișier                                     |
|--------------------------------------|-------------------------------------------------|---------------------------------------------|
| `Element` trait                      | `Element` trait                                 | `src/elements.rs`                           |
| `Point`                              | `Point`                                         | `src/elements.rs`                           |
| `Axis`, `AxisOrientation`            | `Axis`, `AxisOrientation`                       | `src/elements.rs`                           |
| `MainAxisAlignment`, `CrossAxisAlignment`, `MainAxisSize` | `MainAxisAlignment`, `CrossAxisAlignment`, `MainAxisSize` | `src/elements.rs` |
| `Fill`                               | `Fill` (simplificat, fără gradient)              | `src/elements.rs`                           |
| `Margin`, `Padding`, `Border`        | `Margin`, `Padding`, `Border`                   | `src/elements.rs`                           |
| `Vector2FExt`                        | `Vector2FExt`                                  | `src/elements.rs`                           |
| `SizeConstraint`                     | `SizeConstraint`                                | `src/elements.rs`                           |
| `ColorU`                             | `ColorU`                                        | `src/color.rs`                              |
| `Vector2F`, `RectF`, `Size2F`, `Point2D` | `Vector2F`, `RectF`, `Size2F`, `PointF`      | `src/geometry.rs` (bazat pe `euclid`)       |
| `Empty`                              | `Empty`                                         | `src/elements/empty.rs`                     |
| `Rect`                               | `Rect`                                          | `src/elements/rect.rs`                      |
| `Container`                          | `Container`                                     | `src/elements/container.rs`                   |
| `ConstrainedBox`                     | `ConstrainedBox`                              | `src/elements/constrained_box.rs`           |
| `Align`                              | `Align` + `Alignment`                           | `src/elements/align.rs`                     |
| `Clipped`                            | `Clipped` (fără clipping real încă)             | `src/elements/clipped.rs`                   |
| `Flex`                               | `Flex`                                          | `src/elements/flex.rs`                        |
| `Stack`                              | `Stack` (simplificat, fără `Positioned`)        | `src/elements/stack.rs`                     |
| `UiComponentStyles`                  | `UiComponentStyles`                             | `src/style.rs`                              |

## Ce nu am portat încă

- `Positioned`, `OffsetPositioning`, `Overlay` (vor veni pentru tooltip/popover).
- `Scrollable`, `NewScrollable`, `ClippedScrollable` (vor fi necesare pentru liste).
- `Text`, `Icon`, `Image` (necesită renderer și fonturi).
- `Hoverable`, `EventHandler`, `SelectableArea` (vor fi adăugate odată cu input-ul).
- `Button`, `Checkbox`, `Switch`, `Chip`, `TextInput`, `List` (task-ul `007`).
- Rendererul `wgpu`, fereastra `winit` și event loop-ul (task-ul `006`/`platform`).

## Licențe

- Codul reimplementat în `goble-ui` este sub licența MIT a proiectului Goble.
- Nu am copiat codul sub licență AGPL din `app/src`.
- `octomusui_core`/`octomusui` sunt MIT, dar dependentele lor locale sunt AGPL; de aceea am evitat să le copiem.

## Build

```bash
cd /Users/adrian.tucicovencogmail.com/Projects/goble
cargo check -p goble-ui
```

Rezultat: `goble-ui` compilează fără erori (cu funcționalitate partială).
