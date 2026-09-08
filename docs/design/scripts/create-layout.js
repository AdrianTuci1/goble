// Create the main Goble native (wgpu/Rust) app layout in Penpot.
// Mirrors crates/goble-ui/src/elements/shell.rs, topbar.rs, conversation_sidebar.rs,
// chat_header.rs, chat_composer.rs, and agent_card.rs.
// The left sidebar contains agents/routines; the right area is the chat view.
// Chat history is a button that opens a panel anchored to the button.

const THEME = 'dark';

const COLORS = {
  bg: '#0E0E0E',
  surface: '#161616',
  surfaceRaised: '#1E1E1E',
  border: '#262626',
  text: '#E8E8E8',
  muted: '#8E8E8E',
  accent: '#9A9A9A',
  hover: '#1E1E1E',
  selected: '#2A2A2A',
  error: '#EF4444',
  success: '#22C55E',
  warning: '#F59E0B',
};

const SIZES = {
  topbarHeight: 40,
  sidebarWidth: 260,
  buttonSize: 32,
  composerMinHeight: 160,
};

function makeBoard(name, width, height, fill) {
  const board = penpot.createBoard();
  board.name = name;
  board.resize(width, height);
  if (fill) {
    board.fills = [{ fillColor: fill, fillOpacity: 1 }];
  }
  return board;
}

function makeText(name, content, options = {}) {
  const text = penpot.createText(content);
  if (!text) throw new Error('Failed to create text: ' + content);
  text.name = name;
  text.fontSize = String(options.fontSize || 14);
  text.fontWeight = String(options.fontWeight || '400');
  text.fills = [{ fillColor: options.color || COLORS.text, fillOpacity: 1 }];
  text.growType = 'auto-width';
  return text;
}

function makeIconButton(name, icon, options = {}) {
  const size = options.size || SIZES.buttonSize;
  const bg = options.bg || COLORS.surface;
  const btn = makeBoard(name, size, size, bg);
  btn.borderRadius = size / 4;
  const label = makeText(name + ' icon', icon, {
    fontSize: options.fontSize || 16,
    color: options.color || COLORS.muted,
  });
  btn.addFlexLayout();
  btn.flex.dir = 'row';
  btn.flex.alignItems = 'center';
  btn.flex.justifyContent = 'center';
  btn.appendChild(label);
  return btn;
}

function makePill(name, label, icon, options = {}) {
  const height = 28;
  const pill = makeBoard(name, options.width || 80, height, options.bg || COLORS.surface);
  pill.borderRadius = 6;
  pill.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  pill.addFlexLayout();
  pill.flex.dir = 'row';
  pill.flex.alignItems = 'center';
  pill.flex.justifyContent = 'center';
  pill.flex.columnGap = 6;
  pill.flex.horizontalPadding = 10;
  if (icon) {
    pill.appendChild(makeText(name + ' icon', icon, { fontSize: 14, color: options.iconColor || COLORS.muted }));
  }
  pill.appendChild(makeText(name + ' label', label, { fontSize: 12, color: options.color || COLORS.muted }));
  return pill;
}

function makeComposer() {
  // Composer wrap: fills bottom gutter, padding md sm md md.
  const wrap = makeBoard('Composer wrap', 1180, SIZES.composerMinHeight, COLORS.bg);
  wrap.addFlexLayout();
  wrap.flex.dir = 'column';
  wrap.flex.horizontalPadding = 16;
  wrap.flex.verticalPadding = 12;
  wrap.flex.horizontalSizing = 'fill';
  wrap.flex.verticalSizing = 'fix';

  // Raised card with 1px border and 8px radius.
  const card = makeBoard('Composer card', 1148, SIZES.composerMinHeight, COLORS.surfaceRaised);
  card.borderRadius = 8;
  card.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  card.addFlexLayout();
  card.flex.dir = 'column';
  card.flex.horizontalPadding = 16;
  card.flex.verticalPadding = 12;
  card.flex.rowGap = 8;
  card.flex.horizontalSizing = 'fill';
  wrap.appendChild(card);

  // TextArea placeholder.
  const textareaPlaceholder = makeText('Textarea placeholder', 'Ask anything...', {
    fontSize: 13,
    color: COLORS.muted,
  });
  card.appendChild(textareaPlaceholder);

  // Footer: attach on the left, model/profile/stop on the right.
  const footer = makeBoard('Composer footer', 1116, 28, COLORS.surfaceRaised);
  footer.addFlexLayout();
  footer.flex.dir = 'row';
  footer.flex.alignItems = 'center';
  footer.flex.justifyContent = 'space-between';
  footer.flex.horizontalSizing = 'fill';
  card.appendChild(footer);

  const footerLeft = makeBoard('Footer left', 80, 28, COLORS.surfaceRaised);
  footerLeft.addFlexLayout();
  footerLeft.flex.dir = 'row';
  footerLeft.flex.alignItems = 'center';
  footerLeft.flex.columnGap = 8;
  footer.appendChild(footerLeft);
  footerLeft.appendChild(makePill('Attach button', 'Attach', '+'));

  const footerRight = makeBoard('Footer right', 260, 28, COLORS.surfaceRaised);
  footerRight.addFlexLayout();
  footerRight.flex.dir = 'row';
  footerRight.flex.alignItems = 'center';
  footerRight.flex.justifyContent = 'end';
  footerRight.flex.columnGap = 8;
  footer.appendChild(footerRight);

  footerRight.appendChild(makePill('Model pill', 'gpt-4o', '✦'));
  footerRight.appendChild(makePill('Profile pill', 'Account', '●'));
  footerRight.appendChild(makePill('Stop button', 'Stop', '■', { iconColor: COLORS.error, color: COLORS.error }));

  return wrap;
}

function makeAgentItem(agent) {
  const item = makeBoard('Agent item ' + agent.name, 236, 64, agent.active ? COLORS.selected : COLORS.surface);
  item.borderRadius = 8;
  item.addFlexLayout();
  item.flex.dir = 'row';
  item.flex.alignItems = 'center';
  item.flex.horizontalPadding = 8;
  item.flex.columnGap = 8;

  const avatar = makeBoard('Avatar ' + agent.name, 40, 40, COLORS.accent);
  avatar.borderRadius = 20;
  avatar.addFlexLayout();
  avatar.flex.dir = 'row';
  avatar.flex.alignItems = 'center';
  avatar.flex.justifyContent = 'center';
  avatar.appendChild(makeText('Avatar label ' + agent.name, agent.name[0], { fontSize: 14, color: COLORS.text }));
  item.appendChild(avatar);

  const meta = makeBoard('Agent meta ' + agent.name, 172, 48, agent.active ? COLORS.selected : COLORS.surface);
  meta.addFlexLayout();
  meta.flex.dir = 'column';
  meta.flex.verticalPadding = 4;
  meta.flex.rowGap = 2;
  item.appendChild(meta);

  meta.appendChild(makeText('Agent name ' + agent.name, agent.name, { fontSize: 13, color: COLORS.text, fontWeight: '600' }));
  meta.appendChild(makeText('Agent desc ' + agent.name, agent.desc, { fontSize: 11, color: COLORS.muted }));

  return item;
}

function makeHistoryButton() {
  return makeIconButton('History button', '↩', { size: 28, bg: COLORS.surface });
}

function makeTuiMessage(role, text) {
  // Agentic TUI transcript: the user's prompt is echoed with a `❯` prefix and
  // the agent's reply is plain flowing text. No avatar, author name or
  // timestamp, and no bubble/card background.
  if (role === 'user') {
    const row = makeBoard('TUI user message', 1148, 24, COLORS.bg);
    row.addFlexLayout();
    row.flex.dir = 'row';
    row.flex.alignItems = 'center';
    row.flex.columnGap = 8;
    row.appendChild(makeText('TUI prompt glyph', '❯', { fontSize: 13, color: COLORS.accent, fontWeight: '600' }));
    row.appendChild(makeText('TUI user text', text, { fontSize: 13, color: COLORS.text }));
    return row;
  }
  const row = makeBoard('TUI assistant message', 1148, 24, COLORS.bg);
  row.addFlexLayout();
  row.flex.dir = 'row';
  row.flex.alignItems = 'center';
  row.appendChild(makeText('TUI assistant text', text, { fontSize: 13, color: COLORS.text }));
  return row;
}

function makeHistoryPanel() {
  const panel = makeBoard('History panel', 220, 180, COLORS.surfaceRaised);
  panel.borderRadius = 8;
  panel.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  panel.addFlexLayout();
  panel.flex.dir = 'column';
  panel.flex.horizontalPadding = 8;
  panel.flex.verticalPadding = 8;
  panel.flex.rowGap = 4;

  const historyItems = ['Previous chat', 'Another chat', 'Draft from yesterday'];
  for (const item of historyItems) {
    const row = makeBoard('History item ' + item, 204, 28, COLORS.surfaceRaised);
    row.borderRadius = 6;
    row.addFlexLayout();
    row.flex.dir = 'row';
    row.flex.alignItems = 'center';
    row.flex.horizontalPadding = 8;
    row.appendChild(makeText('History label ' + item, item, { fontSize: 12, color: COLORS.text }));
    panel.appendChild(row);
  }

  return panel;
}

function makeChatSidebar() {
  const sidebar = makeBoard('Chat sidebar', 280, 860, COLORS.surface);
  sidebar.addFlexLayout();
  sidebar.flex.dir = 'column';
  sidebar.flex.horizontalPadding = 16;
  sidebar.flex.verticalPadding = 16;
  sidebar.flex.rowGap = 16;
  sidebar.flex.horizontalSizing = 'fix';
  sidebar.flex.verticalSizing = 'fill';

  sidebar.appendChild(makeText('Computer Use label', 'Computer Use', { fontSize: 11, color: COLORS.muted }));

  const preview = makeBoard('Computer Use preview', 248, 120, COLORS.surfaceRaised);
  preview.borderRadius = 8;
  preview.addFlexLayout();
  preview.flex.dir = 'column';
  preview.flex.alignItems = 'center';
  preview.flex.justifyContent = 'center';
  preview.flex.rowGap = 8;
  preview.appendChild(makeText('Terminal icon', '▸_', { fontSize: 32, color: COLORS.muted }));
  preview.appendChild(makeText('Active label', 'Active', { fontSize: 12, color: COLORS.success }));
  sidebar.appendChild(preview);

  const divider = makeBoard('Routines divider', 248, 1, COLORS.border);
  sidebar.appendChild(divider);

  const routinesHeader = makeBoard('Routines header', 248, 24, COLORS.surface);
  routinesHeader.addFlexLayout();
  routinesHeader.flex.dir = 'row';
  routinesHeader.flex.alignItems = 'center';
  routinesHeader.flex.justifyContent = 'space-between';
  routinesHeader.appendChild(makeText('Routines label', 'Routines', { fontSize: 11, color: COLORS.muted }));
  routinesHeader.appendChild(makeIconButton('New routine button', '+', { size: 24, bg: COLORS.surface }));
  sidebar.appendChild(routinesHeader);

  const routinesList = makeBoard('Routines list', 248, 200, COLORS.surface);
  routinesList.addFlexLayout();
  routinesList.flex.dir = 'column';
  routinesList.flex.rowGap = 4;
  routinesList.flex.horizontalSizing = 'fill';

  const routines = [
    { title: 'Morning social', schedule: 'Every day 8 AM', enabled: true },
    { title: 'Outbound weekly', schedule: 'Fridays 10 AM', enabled: false },
    { title: 'Nightly build', schedule: 'Daily 11 PM', enabled: true },
  ];
  for (const routine of routines) {
    const row = makeBoard('Routine item ' + routine.title, 248, 44, COLORS.surface);
    row.borderRadius = 6;
    row.addFlexLayout();
    row.flex.dir = 'row';
    row.flex.alignItems = 'start';
    row.flex.columnGap = 10;
    row.flex.horizontalPadding = 8;
    row.flex.verticalPadding = 8;

    const dot = makeBoard('Routine dot ' + routine.title, 6, 6, routine.enabled ? COLORS.accent : COLORS.muted);
    dot.borderRadius = 3;
    row.appendChild(dot);

    const meta = makeBoard('Routine meta ' + routine.title, 220, 28, COLORS.surface);
    meta.addFlexLayout();
    meta.flex.dir = 'column';
    meta.flex.rowGap = 2;
    meta.appendChild(makeText('Routine title ' + routine.title, routine.title, { fontSize: 12, color: COLORS.text }));
    meta.appendChild(makeText('Routine schedule ' + routine.title, routine.schedule, { fontSize: 11, color: COLORS.muted }));
    row.appendChild(meta);

    routinesList.appendChild(row);
  }
  sidebar.appendChild(routinesList);

  return sidebar;
}

async function buildLayout() {
  let page = penpotUtils.getPageByName('Goble Native App');
  if (!page) {
    page = penpot.createPage();
    page.name = 'Goble Native App';
  }
  await penpot.openPage(page.id);

  const root = page.root;
  for (const child of [...root.children]) {
    try {
      child.remove();
    } catch (e) {}
  }

  // App shell
  const appShell = makeBoard('App shell', 1440, 900, COLORS.bg);
  appShell.addFlexLayout();
  appShell.flex.dir = 'column';
  appShell.flex.horizontalSizing = 'fix';
  appShell.flex.verticalSizing = 'fix';
  root.appendChild(appShell);

  // Topbar
  const topbar = makeBoard('Topbar', 1440, SIZES.topbarHeight, COLORS.surface);
  topbar.addFlexLayout();
  topbar.flex.dir = 'row';
  topbar.flex.alignItems = 'center';
  topbar.flex.justifyContent = 'space-between';
  topbar.flex.horizontalPadding = 12;
  appShell.appendChild(topbar);

  const topbarLeft = makeBoard('Topbar left', 120, SIZES.topbarHeight, COLORS.surface);
  topbarLeft.addFlexLayout();
  topbarLeft.flex.dir = 'row';
  topbarLeft.flex.alignItems = 'center';
  topbarLeft.flex.columnGap = 8;
  topbar.appendChild(topbarLeft);
  topbarLeft.appendChild(makeIconButton('Menu button', '☰', { bg: COLORS.surface }));
  topbarLeft.appendChild(makeIconButton('Threads button', '✉', { bg: COLORS.surface }));

  const topbarRight = makeBoard('Topbar right', 120, SIZES.topbarHeight, COLORS.surface);
  topbarRight.addFlexLayout();
  topbarRight.flex.dir = 'row';
  topbarRight.flex.alignItems = 'center';
  topbarRight.flex.justifyContent = 'end';
  topbarRight.flex.columnGap = 8;
  topbar.appendChild(topbarRight);
  topbarRight.appendChild(makeIconButton('Inbox button', '▤', { bg: COLORS.surface }));
  topbarRight.appendChild(makeIconButton('Settings button', '⚙', { bg: COLORS.surface }));

  // Body: agent sidebar overlay + chat content
  const body = makeBoard('Body', 1440, 860, COLORS.bg);
  appShell.appendChild(body);

  // Chat content (starts at x=260)
  const content = makeBoard('Chat content', 1180, 860, COLORS.bg);
  content.x = SIZES.sidebarWidth;
  content.y = 0;
  content.addFlexLayout();
  content.flex.dir = 'column';
  content.flex.horizontalSizing = 'fix';
  content.flex.verticalSizing = 'fix';
  body.appendChild(content);

  // Chat header (ChatHeader component)
  const chatHeader = makeBoard('Chat header', 1180, 44, COLORS.surface);
  chatHeader.addFlexLayout();
  chatHeader.flex.dir = 'row';
  chatHeader.flex.alignItems = 'center';
  chatHeader.flex.justifyContent = 'space-between';
  chatHeader.flex.horizontalPadding = 16;
  chatHeader.flex.verticalPadding = 12;
  content.appendChild(chatHeader);

  const headerLeft = makeBoard('Header left', 300, 28, COLORS.surface);
  headerLeft.addFlexLayout();
  headerLeft.flex.dir = 'row';
  headerLeft.flex.alignItems = 'center';
  headerLeft.flex.columnGap = 8;
  chatHeader.appendChild(headerLeft);

  headerLeft.appendChild(makeText('Chat title', 'New conversation', { fontSize: 12, color: COLORS.text }));
  headerLeft.appendChild(makeHistoryButton());

  // Sidebar toggle button (left-panel-open)
  chatHeader.appendChild(makeIconButton('Sidebar toggle', '▸', { size: 28, bg: COLORS.surface }));

  // Messages area
  const messagesArea = makeBoard('Messages area', 1180, 560, COLORS.bg);
  messagesArea.addFlexLayout();
  messagesArea.flex.dir = 'column';
  messagesArea.flex.rowGap = 12;
  messagesArea.flex.horizontalPadding = 16;
  messagesArea.flex.verticalPadding = 16;
  messagesArea.flex.horizontalSizing = 'fill';
  messagesArea.flex.verticalSizing = 'fill';
  content.appendChild(messagesArea);

  messagesArea.appendChild(makeTuiMessage('user', 'Hello, can you help with the native UI layout?'));
  messagesArea.appendChild(makeTuiMessage('assistant', 'Sure! The native app uses a topbar, an agent sidebar, and a chat view with a scrollable message area and a full composer.'));

  // Composer (ChatComposer component)
  content.appendChild(makeComposer());

  // Agent sidebar overlay
  const sidebar = makeBoard('Agent sidebar', SIZES.sidebarWidth, 860, COLORS.surface);
  sidebar.x = 0;
  sidebar.y = 0;
  sidebar.addFlexLayout();
  sidebar.flex.dir = 'column';
  sidebar.flex.topPadding = 12;
  sidebar.flex.leftPadding = 12;
  sidebar.flex.rightPadding = 12;
  sidebar.flex.bottomPadding = 12;
  sidebar.flex.rowGap = 12;
  sidebar.flex.horizontalSizing = 'fix';
  sidebar.flex.verticalSizing = 'fill';
  body.appendChild(sidebar);

  // Sidebar header: search + new agent button
  const sidebarHeader = makeBoard('Sidebar header', 236, 40, COLORS.surface);
  sidebarHeader.addFlexLayout();
  sidebarHeader.flex.dir = 'row';
  sidebarHeader.flex.alignItems = 'center';
  sidebarHeader.flex.columnGap = 8;
  sidebar.appendChild(sidebarHeader);

  const searchInput = makeBoard('Search input', 188, 32, COLORS.surfaceRaised);
  searchInput.borderRadius = 8;
  searchInput.addFlexLayout();
  searchInput.flex.dir = 'row';
  searchInput.flex.alignItems = 'center';
  searchInput.flex.horizontalPadding = 8;
  searchInput.appendChild(makeText('Search placeholder', 'Search agents...', { fontSize: 12, color: COLORS.muted }));
  sidebarHeader.appendChild(searchInput);

  sidebarHeader.appendChild(makeIconButton('New agent button', '+', { size: 32, bg: COLORS.surfaceRaised }));

  // Agent list
  const agentList = makeBoard('Agent list', 236, 600, COLORS.surface);
  agentList.addFlexLayout();
  agentList.flex.dir = 'column';
  agentList.flex.rowGap = 8;
  agentList.flex.horizontalSizing = 'fill';
  agentList.flex.verticalSizing = 'auto';
  sidebar.appendChild(agentList);

  const agents = [
    { name: 'Ada', desc: 'Code review agent', active: true },
    { name: 'Coder', desc: 'Build & test agent', active: false },
    { name: 'Planner', desc: 'Task planning agent', active: false },
    { name: 'Research', desc: 'Web research agent', active: false },
  ];
  for (const agent of agents) {
    agentList.appendChild(makeAgentItem(agent));
  }

  // Apply tokens
  function applyTokenByName(shape, name, props) {
    const token = penpotUtils.findTokenByName(name);
    if (token) {
      shape.applyToken(token, props);
    }
  }

  applyTokenByName(appShell, 'theme.' + THEME + '.bg', ['fill']);
  applyTokenByName(topbar, 'theme.' + THEME + '.surface', ['fill']);
  applyTokenByName(topbarLeft, 'theme.' + THEME + '.surface', ['fill']);
  applyTokenByName(topbarRight, 'theme.' + THEME + '.surface', ['fill']);
  applyTokenByName(body, 'theme.' + THEME + '.bg', ['fill']);
  applyTokenByName(content, 'theme.' + THEME + '.bg', ['fill']);
  applyTokenByName(sidebar, 'theme.' + THEME + '.surface', ['fill']);
  applyTokenByName(sidebarHeader, 'theme.' + THEME + '.surface', ['fill']);
  applyTokenByName(agentList, 'theme.' + THEME + '.surface', ['fill']);
  applyTokenByName(chatHeader, 'theme.' + THEME + '.surface', ['fill']);

  // Separate components: open states and sidebars are not embedded in the main layout.
  const historyPanelComponent = makeHistoryPanel();
  historyPanelComponent.name = 'History panel open';
  historyPanelComponent.x = 1500;
  historyPanelComponent.y = 0;
  root.appendChild(historyPanelComponent);

  const chatSidebarComponent = makeChatSidebar();
  chatSidebarComponent.name = 'Chat sidebar open';
  chatSidebarComponent.x = 1500;
  chatSidebarComponent.y = 220;
  root.appendChild(chatSidebarComponent);

  return {
    page: page.name,
    pageId: page.id,
    structure: penpotUtils.shapeStructure(appShell, 2),
  };
}

return buildLayout();
