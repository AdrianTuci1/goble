# 016 — Pagina Connectors (MCP)

## Status

[ ] Activ

## Context

Aplicația Tauri are `ConnectorsPage.tsx` pentru servere MCP: căutare, instalare, listare, activare/dezactivare de tool-uri, test call. Nativ nu există încă această vedere.

## Obiective

1. Creează o nouă `ActiveView::Connectors` și un buton în topbar/sidebar pentru navigare.
2. Implementează `ConnectorsViewPanel` în `goble-desktop-native/src/views/connectors.rs`.
3. Funcționalități:
   - Căutare MCP (`search_mcp_servers`).
   - Listare servere instalate (`list_mcp_servers`).
   - Instalare (`install_mcp_server`) cu selecție de secrete.
   - Update/delete server.
   - Discover tools (`discover_mcp_tools`) și toggle enabled tools.
   - Test call tool (`test_call_mcp_tool`).
4. UI: carduri `ConnectorCard`, formulare simple, liste de tool-uri.

## Criterii de acceptare

- Pagina Connectors este navigabilă din aplicație.
- Se pot lista, instala, edita și testa servere MCP.
- Build verde.

## Dependențe

- `009-topbar.md`

## Fișiere afectate

- `crates/goble-desktop-native/src/views/connectors.rs` (nou)
- `crates/goble-desktop-native/src/app.rs`
- `crates/goble-ui/src/elements/connector_card.rs`
- `crates/goble-ui/src/elements/shell.rs`
