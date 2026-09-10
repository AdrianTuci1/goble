# Goble UI primitive catalog

A reduced catalog of ~58 primitives that covers the current Goble Desktop views while keeping the visual language and interaction model of `~/Projects/warp-new`.

The list is intentionally compact: it reuses the generic layout primitives already ported in `goble-ui` and adds only the visual/domain-specific widgets needed for chat, threads, agents, connectors, settings, vault, workers and execution history.

Each entry contains:
- **Purpose** – what it renders.
- **Props** – configurable data/behaviour.
- **States** – visual states it can be in.
- **Style tokens** – the `goble-ui` design tokens it consumes.
- **React equivalent** – the existing React component(s) it replaces.
- **Priority** – `P0` = needed for first end-to-end native screen, `P1` = needed to reach parity.

---

## 1. Layout / foundation (already implemented in `goble-ui`)

These primitives are already available in `crates/goble-ui/src/elements` and are the building blocks for everything else.

### 1. `Empty`
- **Purpose:** zero-size placeholder.
- **Props:** `size: Option<Size2F>`.
- **States:** –
- **Style tokens:** –
- **React equivalent:** `null` / spacer helper.
- **Priority:** P0.

### 2. `Rect`
- **Purpose:** solid rectangle, used for separators, selection backgrounds, hover fills.
- **Props:** `fill: Fill`, `border: Option<Border>`, `border_radius: Option<BorderRadius>`.
- **States:** –
- **Style tokens:** `surface`, `surface-raised`, `border`, `accent`.
- **React equivalent:** plain `div` with background/border.
- **Priority:** P0.

### 3. `Container`
- **Purpose:** single-child wrapper with padding/background/border.
- **Props:** `child: Box<dyn Element>`, `styles: UiComponentStyles`.
- **States:** –
- **Style tokens:** all surface/border/radius/padding tokens.
- **React equivalent:** generic `div` wrapper.
- **Priority:** P0.

### 4. `ConstrainedBox`
- **Purpose:** forces min/max width/height on a child.
- **Props:** `min: Size2F`, `max: Size2F`, `child`.
- **States:** –
- **Style tokens:** –
- **React equivalent:** CSS `min-width`, `max-width` wrappers.
- **Priority:** P0.

### 5. `Align`
- **Purpose:** positions a child inside available space.
- **Props:** `alignment: Alignment`, `child`.
- **States:** –
- **Style tokens:** –
- **React equivalent:** `align-items` / `justify-content` wrappers.
- **Priority:** P0.

### 6. `Clipped`
- **Purpose:** clips overflowing children.
- **Props:** `child`, optional `border_radius`.
- **States:** –
- **Style tokens:** –
- **React equivalent:** `overflow: hidden`.
- **Priority:** P0.

### 7. `Flex`
- **Purpose:** one-dimensional layout (row/column) with alignment and gap.
- **Props:** `axis: Axis`, `main_alignment: MainAxisAlignment`, `cross_alignment: CrossAxisAlignment`, `gap: f32`, `children`.
- **States:** –
- **Style tokens:** spacing scale `xs`–`xl` for `gap`.
- **React equivalent:** CSS flexbox wrappers.
- **Priority:** P0.

### 8. `Stack`
- **Purpose:** overlays children on top of each other.
- **Props:** `children`, `alignment: Alignment`.
- **States:** –
- **Style tokens:** –
- **React equivalent:** absolute-positioned overlays (composer inner buttons).
- **Priority:** P0.

### 9. `Spacer`
- **Purpose:** expands to fill available space along the main axis.
- **Props:** `flex: f32`.
- **States:** –
- **Style tokens:** –
- **React equivalent:** `flex: 1` div.
- **Priority:** P0.

### 10. `Divider`
- **Purpose:** 1px horizontal or vertical separator.
- **Props:** `axis: Axis`, `color: ColorU`, `thickness: f32`.
- **States:** –
- **Style tokens:** `border`.
- **React equivalent:** `border-bottom` / `border-right` div.
- **Priority:** P0.

### 11. `Padding`
- **Purpose:** uniform or asymmetric padding around a child.
- **Props:** `insets: EdgeInsets`, `child`.
- **States:** –
- **Style tokens:** spacing scale.
- **React equivalent:** padding wrappers.
- **Priority:** P0.

### 12. `Scrollable`
- **Purpose:** scrollable region for lists, message panes and settings content.
- **Props:** `child`, `axis: Axis`, `show_scrollbar: bool`.
- **States:** idle, scrolling, scrollbar-hover.
- **Style tokens:** `border` for scrollbar thumb.
- **React equivalent:** `overflow-y: auto` containers.
- **Priority:** P0.

### 13. `Border`
- **Purpose:** decorative border helper applied to any child.
- **Props:** `width`, `color`, `sides: [bool; 4]`, `border_radius`.
- **States:** –
- **Style tokens:** `border`.
- **React equivalent:** bordered div.
- **Priority:** P0.

---

## 2. Typography

### 14. `Text`
- **Purpose:** single-line or wrapped body text.
- **Props:** `text: String`, `font_size: f32`, `color: ColorU`, `line_height: f32`, `max_lines: Option<usize>`.
- **States:** –
- **Style tokens:** `text`, `muted`, font family from `DesignSystem`.
- **React equivalent:** `<span>`, message body text.
- **Priority:** P0.

### 15. `Label`
- **Purpose:** uppercase, small, muted caption used for section headers and form labels.
- **Props:** `text`, `size: LabelSize` (`xs` | `sm`).
- **States:** –
- **Style tokens:** `muted`, font size 11–12 px, letter-spacing 0.5 px.
- **React equivalent:** `<h4 className="settings-menu-title">`, `panel-label`.
- **Priority:** P0.

### 16. `Caption`
- **Purpose:** secondary, smaller text for metadata, hints and timestamps.
- **Props:** `text`, `color`.
- **States:** –
- **Style tokens:** `muted`.
- **React equivalent:** `trace-card-time`, `threads-message-time`.
- **Priority:** P0.

### 17. `Code`
- **Purpose:** monospaced pre-formatted block or inline snippet.
- **Props:** `text`, `inline: bool`, `max_height: Option<f32>`.
- **States:** –
- **Style tokens:** `surface`, `border`, mono font.
- **React equivalent:** `<pre>`, `panel-code`, `mcp-test-result`.
- **Priority:** P1.

---

## 3. Iconography

### 18. `Icon`
- **Purpose:** small SVG icon with consistent sizing and color.
- **Props:** `name: IconName`, `size: f32`, `color: ColorU`.
- **States:** normal, hover, disabled, active.
- **Style tokens:** `text`, `muted`, `accent`.
- **React equivalent:** `lucide-react` icons.
- **Priority:** P0.

### 19. `Avatar`
- **Purpose:** coloured circle/square with initials or icon.
- **Props:** `label: String`, `size: f32`, `shape: Circle | Squircle`, `background: ColorU`, `foreground: ColorU`.
- **States:** –
- **Style tokens:** accent hash colours, `surface-raised`.
- **React equivalent:** `threads-message-avatar`, `agent-card-avatar`, `workspace-item`.
- **Priority:** P0.

---

## 4. Surfaces / navigation

### 20. `Page`
- **Purpose:** top-level scrollable page shell.
- **Props:** `header: Option<Header>`, `children`, `max_width: Option<f32>`.
- **States:** –
- **Style tokens:** `bg`.
- **React equivalent:** `.page` / `.agents-page` / `.mcp-page`.
- **Priority:** P0.

### 21. `Header`
- **Purpose:** page title bar with optional leading/trailing actions.
- **Props:** `title`, `leading`, `trailing`, `height: f32`.
- **States:** –
- **Style tokens:** `surface`, `border`, `text`, `muted`.
- **React equivalent:** `.page-header`, `.threads-header`, `.normal-chat-header`.
- **Priority:** P0.

### 22. `Sidebar`
- **Purpose:** collapsible vertical navigation panel.
- **Props:** `width: f32`, `collapsed: bool`, `children`.
- **States:** collapsed, expanded.
- **Style tokens:** `surface`, `border`, `hover`, `selected`.
- **React equivalent:** `.main-sidebar`, `.threads-sidebar`.
- **Priority:** P0.

### 23. `SidebarItem`
- **Purpose:** selectable row with icon and label inside a sidebar.
- **Props:** `icon: IconName`, `label`, `selected: bool`, `badge: Option<String>`, `on_click`.
- **States:** default, hover, selected.
- **Style tokens:** `hover`, `selected`, `text`, `muted`.
- **React equivalent:** `.conversation-item`, `.channel-item`, `.settings-menu-item`.
- **Priority:** P0.

### 24. `RightPanel`
- **Purpose:** fixed-width secondary panel on the right.
- **Props:** `visible: bool`, `width: f32`, `children`, `on_close`.
- **States:** visible, hidden, animating.
- **Style tokens:** `surface`, `border`.
- **React equivalent:** `.right-sidebar`, `.participants-panel`.
- **Priority:** P0.

### 25. `TabBar`
- **Purpose:** horizontal row of tabs with selection indicator.
- **Props:** `tabs: Vec<TabData>`, `active: String`, `on_select`.
- **States:** –
- **Style tokens:** `border`, `selected`, `text`, `muted`.
- **React equivalent:** `.right-sidebar-tabs`, `.invite-user-tabs`.
- **Priority:** P0.

### 26. `Tab`
- **Purpose:** individual tab button.
- **Props:** `label`, `active: bool`, `on_click`.
- **States:** default, hover, active.
- **Style tokens:** `selected`, `border`.
- **React equivalent:** `.right-sidebar-tab`.
- **Priority:** P0.

### 27. `Toolbar`
- **Purpose:** compact horizontal strip of icon buttons / chips.
- **Props:** `leading`, `trailing`, `height: f32`.
- **States:** –
- **Style tokens:** `surface`, `border`.
- **React equivalent:** `.composer-toolbar`, `.threads-sidebar-nav`.
- **Priority:** P0.

### 28. `Drawer`
- **Purpose:** slide-in panel from the right with header, body and footer.
- **Props:** `open: bool`, `title`, `body`, `footer`, `on_close`, `width: f32`.
- **States:** closed, opening, open.
- **Style tokens:** `surface`, `border`, overlay shadow.
- **React equivalent:** `.mcp-drawer`, `.drawer`, `.participants-panel`.
- **Priority:** P0.

### 29. `Modal`
- **Purpose:** centered dialog with backdrop.
- **Props:** `open: bool`, `title`, `body`, `actions`, `on_close`.
- **States:** closed, open.
- **Style tokens:** `surface`, `border`, backdrop colour.
- **React equivalent:** `.threads-modal`, `.mcp-modal`.
- **Priority:** P1.

---

## 5. Input & controls

### 30. `Button`
- **Purpose:** primary/secondary/ghost text button.
- **Props:** `label`, `variant: ButtonVariant`, `icon: Option<IconName>`, `disabled: bool`, `loading: bool`, `on_click`.
- **States:** default, hover, pressed, disabled, loading.
- **Style tokens:** `surface`, `surface-raised`, `border`, `text`, `accent`, radius.
- **React equivalent:** `.btn`, `.mcp-card-btn`, `.settings-back`.
- **Priority:** P0.

### 31. `IconButton`
- **Purpose:** square/circular button that contains only an icon.
- **Props:** `icon`, `size`, `active: bool`, `disabled: bool`, `on_click`, `title`.
- **States:** default, hover, active, disabled.
- **Style tokens:** `hover`, `surface-raised`, `accent`.
- **React equivalent:** `.topbar-btn`, `.composer-icon-btn`, `.header-action`.
- **Priority:** P0.

### 32. `ToggleButton`
- **Purpose:** button that stays pressed/unpressed (toolbar mode switchers).
- **Props:** `icon`, `label`, `active: bool`, `on_click`.
- **States:** default, active.
- **Style tokens:** `selected`, `surface-raised`, `accent`.
- **React equivalent:** `.topbar-btn.active`, `.toolbar-btn.active`.
- **Priority:** P0.

### 33. `Chip`
- **Purpose:** small pill-shaped label, optionally interactive.
- **Props:** `label`, `icon: Option<IconName>`, `interactive: bool`, `missing: bool`, `on_click`, `on_remove`.
- **States:** default, hover, missing.
- **Style tokens:** `surface-raised`, `border`, `muted`, `text`, radius 999 px.
- **React equivalent:** `.composer-chip`, `.mcp-tag`, `.panel-tag`.
- **Priority:** P0.

### 34. `Switch`
- **Purpose:** boolean toggle.
- **Props:** `checked: bool`, `on_change`, `disabled: bool`.
- **States:** on, off, disabled.
- **Style tokens:** `border`, `surface-raised`, `accent`.
- **React equivalent:** `.mcp-toggle`.
- **Priority:** P1.

### 35. `Checkbox`
- **Purpose:** boolean item toggle inside lists/forms.
- **Props:** `checked: bool`, `label`, `on_change`, `disabled: bool`.
- **States:** unchecked, checked, disabled.
- **Style tokens:** `surface`, `border`, `accent`.
- **React equivalent:** `.mcp-secret-item input`, `.checkbox-label`.
- **Priority:** P0.

### 36. `TextInput`
- **Purpose:** single-line text field.
- **Props:** `value`, `placeholder`, `on_change`, `on_submit`, `disabled: bool`, `secure: bool`, `icon: Option<IconName>`.
- **States:** default, focus, disabled, error.
- **Style tokens:** `bg`, `surface`, `border`, `text`, `muted`, radius.
- **React equivalent:** `<input>` used everywhere.
- **Priority:** P0.

### 37. `TextArea`
- **Purpose:** multi-line resizable input.
- **Props:** `value`, `placeholder`, `rows`, `on_change`, `on_submit`, `disabled: bool`.
- **States:** default, focus, disabled.
- **Style tokens:** `bg`, `surface`, `border`, `text`, `muted`, radius.
- **React equivalent:** `composer-textarea`, `<textarea>`.
- **Priority:** P0.

### 38. `SearchInput`
- **Purpose:** input with leading search icon and submit action.
- **Props:** `value`, `placeholder`, `on_change`, `on_search`, `disabled`, `loading`.
- **States:** default, focus, loading.
- **Style tokens:** `surface`, `border`, `muted`.
- **React equivalent:** `.mcp-search-input`.
- **Priority:** P0.

### 39. `Select`
- **Purpose:** native-feeling dropdown selector.
- **Props:** `value`, `options`, `on_change`, `placeholder`.
- **States:** default, open, disabled.
- **Style tokens:** `surface`, `border`, `text`, `muted`, radius.
- **React equivalent:** `<select>` in forms.
- **Priority:** P0.

### 40. `DropdownMenu`
- **Purpose:** floating list of selectable rows anchored to a toggle.
- **Props:** `items`, `selected_id`, `align: Left | Right`, `on_select`, `on_close`.
- **States:** closed, open.
- **Style tokens:** `surface`, `border`, `hover`, shadow `md`.
- **React equivalent:** `ComposerDropdown`, `.mention-popover`, `.tag-picker-popover`.
- **Priority:** P0.

---

## 6. Chat

### 41. `ChatComposer`
- **Purpose:** full-width composer shell with textarea and floating toolbars.
- **Props:** `value`, `placeholder`, `variant`, `disabled`, `on_change`, `on_send`, `leading`, `trailing`.
- **States:** empty, typing, running.
- **Style tokens:** `surface`, `border`, `text`, `muted`.
- **React equivalent:** `ChatComposer`, `.chat-composer-normal`.
- **Priority:** P0.

### 42. `ComposerToolbar`
- **Purpose:** bottom-left and bottom-right action rows inside `ChatComposer`.
- **Props:** `left`, `right`.
- **States:** –
- **Style tokens:** `surface-raised`, `border`.
- **React equivalent:** `.composer-left`, `.composer-right`.
- **Priority:** P0.

### 43. `ChatMessageBubble`
- **Purpose:** message row for user, assistant and system cards.
- **Props:** `role: user | assistant | system`, `content`, `card: Option<CardContent>`, `actions`.
- **States:** default, hover.
- **Style tokens:** `surface-raised`, `border`, `text`, `muted`.
- **React equivalent:** `.normal-message`, `.normal-message-body`, `ApiKeyCard`.
- **Priority:** P0.

### 44. `RunningIndicator`
- **Purpose:** small pulsing dot with label shown while the model is streaming.
- **Props:** `label`, `active: bool`.
- **States:** active/inactive.
- **Style tokens:** success green, `muted`.
- **React equivalent:** `.normal-chat-status-bar`, `.threads-pending-run`.
- **Priority:** P0.

### 45. `QuickActionButton`
- **Purpose:** subtle inline action inside a message or card.
- **Props:** `label`, `on_click`, `disabled`.
- **States:** default, hover, disabled.
- **Style tokens:** `surface-raised`, `border`.
- **React equivalent:** `.normal-message-configure-btn`, `.msg-action`, `.trace-link`.
- **Priority:** P1.

---

## 7. Threads

### 46. `ThreadMessage`
- **Purpose:** full thread message row: avatar, header, body, tags, reactions, replies.
- **Props:** `author`, `content`, `timestamp`, `reactions`, `reply_to`, `is_own`, `actions`, `children`.
- **States:** default, hover, editing.
- **Style tokens:** `bg`, `hover`, `surface-raised`, `text`, `muted`.
- **React equivalent:** `.threads-message`.
- **Priority:** P0.

### 47. `ThreadComposer`
- **Purpose:** bottom input for threads with mention/tag toolbars.
- **Props:** `value`, `placeholder`, `reply_to`, `pending_tags`, `mention_suggestions`, `on_change`, `on_send`, `on_tag`, `on_mention`.
- **States:** empty, typing, mention-open, tag-open.
- **Style tokens:** `bg`, `surface`, `border`, `button`, `muted`.
- **React equivalent:** `.threads-composer`.
- **Priority:** P0.

### 48. `ThreadListItem`
- **Purpose:** row in the thread sidebar with icon, title and unread count.
- **Props:** `title`, `kind: channel | direct | chat`, `selected`, `unread_count`, `on_click`.
- **States:** default, hover, selected, has-unread.
- **Style tokens:** `hover`, `selected`, `badge`, `text`, `muted`.
- **React equivalent:** `.channel-item`, `.dm-item`.
- **Priority:** P0.

### 49. `WorkspaceRail`
- **Purpose:** narrow vertical strip of workspace avatars.
- **Props:** `workspaces`, `selected_id`, `on_add`, `on_select`.
- **States:** default, selected.
- **Style tokens:** `bg`, `border`.
- **React equivalent:** `.threads-workspace-sidebar`, `WorkspaceRail`.
- **Priority:** P1.

### 50. `ChannelListItem`
- **Purpose:** same as `ThreadListItem` but for channels with # icon.
- **Props:** `title`, `selected`, `unread_count`, `on_click`.
- **States:** default, hover, selected.
- **Style tokens:** `hover`, `selected`.
- **React equivalent:** `.channel-item`.
- **Priority:** P0.

### 51. `ParticipantRow`
- **Purpose:** row showing a thread participant and kind (user/agent).
- **Props:** `name`, `kind`, `on_remove`.
- **States:** –
- **Style tokens:** `text`, `muted`.
- **React equivalent:** `.participant-row`, `.mcp-drawer-row`.
- **Priority:** P0.

### 52. `ReactionChip`
- **Purpose:** emoji reaction pill with count and self-state.
- **Props:** `emoji`, `count`, `me: bool`, `on_toggle`.
- **States:** default, me.
- **Style tokens:** `surface-raised`, `border`, `accent`.
- **React equivalent:** `.reaction`.
- **Priority:** P1.

### 53. `MentionPicker`
- **Purpose:** autocomplete popover for @ mentions.
- **Props:** `query`, `suggestions`, `on_select`.
- **States:** open, empty.
- **Style tokens:** `surface`, `border`, `hover`, shadow.
- **React equivalent:** `.mention-popover`, `.mention-option`.
- **Priority:** P1.

---

## 8. Agents

### 54. `AgentCard`
- **Purpose:** rich list item with avatar, name, description, tags and actions.
- **Props:** `agent`, `selected`, `editing`, `on_select`, `on_chat`, `on_edit`, `on_delete`.
- **States:** default, hover, selected, editing.
- **Style tokens:** `surface`, `surface-raised`, `border`, `text`, `muted`.
- **React equivalent:** `.agent-card`, `.agent-list-item`.
- **Priority:** P0.

### 55. `AgentTagList`
- **Purpose:** horizontal list of small capability tags.
- **Props:** `tags`, `removable`.
- **States:** –
- **Style tokens:** `surface-raised`, `border`, `muted`.
- **React equivalent:** `.agent-card-tags`, `.panel-tags`.
- **Priority:** P0.

---

## 9. Connectors

### 56. `ConnectorCard`
- **Purpose:** card for an MCP server with icon, description, tags and action buttons.
- **Props:** `server`, `on_manage`, `on_discover`, `on_edit`, `on_delete`.
- **States:** default, hover.
- **Style tokens:** `surface`, `hover`, `border`, `muted`.
- **React equivalent:** `.mcp-card`, `.mcp-installed-card`.
- **Priority:** P0.

### 57. `ConnectorDrawer`
- **Purpose:** server management drawer with secrets/tools toggles and test area.
- **Props:** `server`, `vault_secrets`, `enabled_tools`, `on_toggle_secret`, `on_toggle_tool`, `on_discover`, `on_save`, `on_close`.
- **States:** open, saving, discovering.
- **Style tokens:** `surface`, `border`, `hover`.
- **React equivalent:** `McpServerDrawer`, `.mcp-drawer`.
- **Priority:** P0.

### 58. `StatusBadge`
- **Purpose:** small pill showing execution/MCP status.
- **Props:** `status: String`, `variant: neutral | success | warning | error`.
- **States:** –
- **Style tokens:** `surface-raised`, `muted`, success/error/warning colours.
- **React equivalent:** `.status-badge`, `.mcp-tag`, `.panel-history-item.running`.
- **Priority:** P0.

---

## Design tokens referenced by the catalog

All primitives above resolve to the same token set currently defined in `src/utils/designSystem.ts` and used by the React app:

- `bg` – main window background.
- `surface` – raised cards, sidebars, input backgrounds.
- `surface-raised` – hover states, chips, toolbars.
- `border` – separators and borders.
- `text` – primary text.
- `muted` – secondary text and icons.
- `accent` – selected state, running indicator, primary actions.
- `success`, `warning`, `error`, `badge` – semantic colours.
- `radius` – `sharp` / `default` / `rounded`, driven by `DesignSystem.radius`.
- `density` – spacing multiplier (`compact` / `default` / `spacious`).
- `font` – `system` / `mono` / `serif`.

The Rust equivalents will be added as part of task `004-design-tokens.md`.

---

## Next steps

1. Port the typography, icon and input primitives (`006-text-and-icons.md`, `007-button-input-primitives.md`).
2. Build the list, sidebar and card primitives (`008-list-and-sidebar.md`).
3. Assemble the first end-to-end native view: **Chat** using `Page`, `Header`, `ChatComposer`, `ChatMessageBubble`, `Sidebar`, `RightPanel`.
