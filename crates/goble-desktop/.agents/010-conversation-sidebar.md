# 010 — Sidebar de conversație custom

## Context
Sidebar-ul stânga din aplicația nativă trebuie să arate premium și să conțină toate elementele descrise: search, create, carduri de conversație/agent, hover menu cu delete și footer de plugins.

## Obiective
1. Rafiază `ConversationSidebar` (`crates/goble-ui/src/elements/conversation_sidebar.rs`):
   - Header cu `SearchInput` rotunjit și buton `+` pentru conversație/agent nou.
   - Listă scrollabilă de carduri de conversație.
   - Footer "Plugins" cu intrări pentru Agent Mode, Connectors, Team.
2. Rafiază `ConversationListItem` (`crates/goble-ui/src/elements/conversation_list_item.rs`):
   - Avatar cu initials și culoare deterministă.
   - Name + timestamp în dreapta sus.
   - Status icon + ultimul răspuns truncat.
   - Fundal la hover și la selecție.
   - Buton trei puncte care apare la hover; click deschide meniu cu Delete.
3. Adaugă componente helper dacă este necesar (de ex. `SidebarHeader`, `PluginsFooter`).
4. Păstrează API-ul public compatibil cu `ShellView` și `ChatViewPanel`.

## Criterii de acceptare
- Sidebar-ul afișează search, create, listă și plugins.
- Cardurile au hover, selecție, status icon și meniu delete.
- `cargo test -p goble-ui` trece.
- Preview example și `goble-desktop-native` compilează.

## Fișiere afectate
- `crates/goble-ui/src/elements/conversation_sidebar.rs`
- `crates/goble-ui/src/elements/conversation_list_item.rs`
- `crates/goble-ui/src/elements.rs` (eventuale noi exporturi)
- `crates/goble-ui/examples/preview.rs`
- `crates/goble-desktop-native/src/views/chat.rs`
