# Plan migrare agent-guide UI -> goble Tauri

## Obiectiv
Replica 1:1 a interfeței demo din repo-ul `agent-guide` în aplicația Tauri `goble-desktop`, pe branch `feature/agent-guide-ui`.

## Structura țintă (fiecare modul conține logica sa)

```
src/
  index.css              -> design tokens globali (themes, accent, font, density, radius)
  utils/designSystem.ts  -> helper aplica claselor design system
  stores/appStore.ts     -> starea: profile, design, threads, agents, history, identity
  mocks/
    threadsData.ts       -> date demo pentru workspace-uri, canale, proiecte, DMs, mesaje
    agentsData.ts        -> agenți demo
    flowsData.ts         -> flows/demo pentru acțiuni (multi-variant, secrets, form, confirmation)
  components/
    TitleBar.tsx/.css
    Sidebar.tsx/.css
    ChatArea.tsx/.css
    RightSidebar.tsx/.css
  pages/
    ThreadsPage.tsx/.css
    AgentsPage.tsx/.css
    SettingsPage.tsx/.css
```

## Funcționalități obligatorii

1. **Shell comun**
   - Title bar cu traffic-light spacer, toggle sidebar, buton threads (hash), buton settings.
   - Main view: sidebar + routes (chat, threads, agents, settings) + right sidebar.
   - Render mode indicator în dreapta sus.

2. **Chat view** (ChatArea)
   - Mesaje user (bubble albastru), agent (bubble stânga) + step cards.
   - Composer default (floating input) + inline composer pentru carduri.
   - Cards: multi-variant, secrets, form, confirmation, code-change.
   - Butoane toolbar: atașament, image, emoji, tag.
   - Running status bar cu dot pulsing, pause/stop/cancel.
   - Panou lateral dreapta cu Info / History.

3. **Threads view** (ThreadsPage)
   - Workspace rail 64px cu initials/color, selected border, add workspace.
   - Sidebar 240px cu nav items: Home, Threads, Inbox, Projects, Agents, Vault, Settings, Help.
   - Secțiuni: Channels, Direct messages, Projects, Agents.
   - Header cu nume canal, lock, acțiuni (search, members, info).
   - Mesaje grupate, reply, reactions, tags, thread replies.
   - Inbox view, Projects view cu proiecte/groups/canale, modal add/edit workspace/channel/project.
   - Identity manager (RSA PEM, authorize/revoke, private channels, fingerprint).

4. **Agents view** (AgentsPage)
   - Lista de agenți cu avatar, nume, buton Add agent.

5. **Settings view** (SettingsPage)
   - Sidebar cu grupuri: Personal, Communities, App.
   - Panouri: Profile, Keys, Members, Appearance + placeholders.
   - Appearance: theme, accent, font, density, radius.
   - Keys: workspace tabs, status access, upload PEM, download/remove key.
   - Members: add member (name, role, key), list, revoke, toggle private channels.

6. **Design system**
   - Variabilele CSS în index.css: --ds-bg, --ds-surface, --ds-surface-raised, --ds-border, --ds-text, --ds-muted, --ds-accent, --ds-hover, --ds-selected, --ds-radius, --ds-font.
   - Theme dark/light/midnight, accent blue/green/purple/orange, font system/mono/serif, density compact/default/spacious, radius sharp/default/rounded.

## Checklist

- [x] Inventariere agent-guide
- [ ] Actualizare index.css design tokens
- [ ] Rescriere App.tsx
- [ ] Rescriere TitleBar
- [ ] Rescriere Sidebar
- [ ] Rescriere ChatArea (mesaje + composer + cards + flows)
- [ ] Rescriere RightSidebar
- [ ] Rescriere ThreadsPage (workspaces + sidebar + content + modals + identity)
- [ ] Rescriere AgentsPage
- [ ] Rescriere SettingsPage
- [ ] Mock data (threads, agents, flows)
- [ ] Actualizare store
- [ ] Build & tests green
- [ ] Commit & push PR
