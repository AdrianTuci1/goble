---
name: penpot
description: >
  Work with Penpot through the MCP connection: upload design tokens, build and
  update layouts, inspect designs, and keep design files in sync with code.
  Use when the user asks about Penpot, design tokens, creating or editing a
  Penpot layout, exporting from Penpot, or syncing design system values.
  Triggers: "penpot", "upload tokens", "sync design", "create layout in penpot",
  "design tokens", "/penpot".
---

# Penpot design sync

Work with Penpot via the MCP connection to upload tokens, build layouts, and
verify designs.

## Prerequisites

- The Penpot MCP server must be configured and enabled (usually in
  `~/.grok/config.toml`).
- The Penpot UI/plugin side must be open and connected to a file.

## Workflow

1. **Inspect**: use `penpot__execute_code` to explore the current file:
   - `penpotUtils.getPages()` for pages
   - `penpotUtils.tokenOverview()` for tokens
   - `penpotUtils.shapeStructure(penpot.root, 2)` for structure
2. **Upload tokens**: create or replace a `TokenSet` via
   `penpot.library.local.tokens.addSet()` and add tokens with `set.addToken()`.
   Map DTCG token types to Penpot token types:
   - color -> color
   - dimension (spacing) -> spacing
   - dimension (radius) -> borderRadius
   - fontFamily -> fontFamilies
   - fontSize -> fontSizes
   - fontWeight -> fontWeights
   - shadow -> shadow
   - number -> number
3. **Create/update layout**: build shapes with `penpot.createBoard()`,
   `penpot.createText()`, etc. Use flex/grid layouts for rows and columns.
   Apply tokens to shapes with `shape.applyToken(token, properties)` where
   applicable.
4. **Verify**: use `penpot__export_shape` with `shapeId: "page"` or a board id
   to render a PNG and confirm the result.
5. **Report**: tell the user which Penpot page/token set changed and note any
   tokens that could not be mapped.

## Penpot API tips

- Read the Penpot High-Level Overview once per session before using tools.
- Use `penpot__penpot_api_info` to look up types and members.
- Valid flex `justifyContent`/`alignItems` values in Penpot are: start, end,
  center, stretch, space-between, space-around, space-evenly. Do not use
  CSS-style names like flex-end.
- Hex colors must be uppercase with # prefix.
- `penpot.openPage(pageId)` is async; await it before adding shapes to a new
  page.

## Design principles

- Use semantic names for boards and shapes.
- Prefer tokens over hard-coded values.
- Keep annotations out of the canvas; use shape names for labels.
