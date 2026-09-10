// Create the Goble native Threads view in Penpot.
// Mirrors crates/goble-ui/src/views/threads_container.rs, thread_sidebar.rs,
// thread_view.rs, and thread_list_view.rs.
// Layout: topbar + left ThreadSidebar + vertical divider + right ThreadView.

const THEME = 'dark';

const COLORS = {
  bg: '#0E0E0E',
  surface: '#161616',
  surfaceRaised: '#1E1E1E',
  border: '#262626',
  text: '#E8E8E8',
  muted: '#8E8E8E',
  accent: '#9A9A9A',
  selected: '#2A2A2A',
  success: '#22C55E',
  warning: '#F59E0B',
  error: '#EF4444',
};

const SIZES = {
  topbarHeight: 40,
  sidebarWidth: 320,
  // Body is 1440 wide: sidebar (320) + divider (1) + thread view (1119).
  threadViewWidth: 1119,
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
  const size = options.size || 32;
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

function makeComposer() {
  // ThreadView uses the same ChatComposer as ChatView but without model,
  // profile or stop configured, so only the Attach button is shown.
  const wrap = makeBoard('Composer wrap', SIZES.threadViewWidth, SIZES.composerMinHeight, COLORS.bg);
  wrap.addFlexLayout();
  wrap.flex.dir = 'column';
  wrap.flex.horizontalPadding = 0;
  wrap.flex.verticalPadding = 12;
  wrap.flex.horizontalSizing = 'fill';
  wrap.flex.verticalSizing = 'fix';

  const card = makeBoard('Composer card', SIZES.threadViewWidth, SIZES.composerMinHeight, COLORS.surfaceRaised);
  card.borderRadius = 8;
  card.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  card.addFlexLayout();
  card.flex.dir = 'column';
  card.flex.horizontalPadding = 16;
  card.flex.verticalPadding = 12;
  card.flex.rowGap = 8;
  card.flex.horizontalSizing = 'fill';
  wrap.appendChild(card);

  card.appendChild(makeText('Textarea placeholder', 'Ask anything...', { fontSize: 13, color: COLORS.muted }));

  const footer = makeBoard('Composer footer', SIZES.threadViewWidth - 32, 28, COLORS.surfaceRaised);
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

  const attachPill = makeBoard('Attach pill', 80, 28, COLORS.surface);
  attachPill.borderRadius = 6;
  attachPill.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  attachPill.addFlexLayout();
  attachPill.flex.dir = 'row';
  attachPill.flex.alignItems = 'center';
  attachPill.flex.justifyContent = 'center';
  attachPill.flex.columnGap = 6;
  attachPill.appendChild(makeText('Attach icon', '+', { fontSize: 14, color: COLORS.muted }));
  attachPill.appendChild(makeText('Attach label', 'Attach', { fontSize: 12, color: COLORS.muted }));
  footerLeft.appendChild(attachPill);

  return wrap;
}

function makeTopbar() {
  const topbar = makeBoard('Topbar', 1440, SIZES.topbarHeight, COLORS.surface);
  topbar.addFlexLayout();
  topbar.flex.dir = 'row';
  topbar.flex.alignItems = 'center';
  topbar.flex.justifyContent = 'space-between';
  topbar.flex.horizontalPadding = 12;

  const topbarLeft = makeBoard('Topbar left', 120, SIZES.topbarHeight, COLORS.surface);
  topbarLeft.addFlexLayout();
  topbarLeft.flex.dir = 'row';
  topbarLeft.flex.alignItems = 'center';
  topbarLeft.flex.columnGap = 8;
  topbar.appendChild(topbarLeft);
  topbarLeft.appendChild(makeIconButton('Menu button', '☰', { bg: COLORS.surface }));
  topbarLeft.appendChild(makeIconButton('Threads button', '✉', { bg: COLORS.selected, color: COLORS.text }));

  const topbarRight = makeBoard('Topbar right', 120, SIZES.topbarHeight, COLORS.surface);
  topbarRight.addFlexLayout();
  topbarRight.flex.dir = 'row';
  topbarRight.flex.alignItems = 'center';
  topbarRight.flex.justifyContent = 'end';
  topbarRight.flex.columnGap = 8;
  topbar.appendChild(topbarRight);
  topbarRight.appendChild(makeIconButton('Inbox button', '▤', { bg: COLORS.surface }));
  topbarRight.appendChild(makeIconButton('Settings button', '⚙', { bg: COLORS.surface }));

  return topbar;
}

function makeThreadBadge(count) {
  const text = makeText('Badge count', String(count), { fontSize: 11, color: COLORS.text, fontWeight: '600' });
  const badge = makeBoard('Unread badge', 24, 18, COLORS.accent);
  badge.borderRadius = 9;
  badge.addFlexLayout();
  badge.flex.dir = 'row';
  badge.flex.alignItems = 'center';
  badge.flex.justifyContent = 'center';
  badge.appendChild(text);
  return badge;
}

function makeThreadListItem(thread, prefix, selected) {
  const row = makeBoard('Thread item ' + thread.title, 288, 40, selected ? COLORS.selected : COLORS.surface);
  row.borderRadius = 6;
  row.addFlexLayout();
  row.flex.dir = 'row';
  row.flex.alignItems = 'center';
  row.flex.justifyContent = 'space-between';
  row.flex.horizontalPadding = 12;

  const left = makeBoard('Thread item left ' + thread.title, 200, 24, selected ? COLORS.selected : COLORS.surface);
  left.addFlexLayout();
  left.flex.dir = 'row';
  left.flex.alignItems = 'center';
  left.flex.columnGap = 10;
  row.appendChild(left);

  const initial = thread.title.trim().charAt(0).toUpperCase() || '#';
  left.appendChild(makeText('Thread leading ' + thread.title, prefix + initial, { fontSize: 13, color: COLORS.text }));
  left.appendChild(makeText('Thread title ' + thread.title, thread.title, { fontSize: 13, color: COLORS.text }));

  const right = makeBoard('Thread item right ' + thread.title, 60, 24, selected ? COLORS.selected : COLORS.surface);
  right.addFlexLayout();
  right.flex.dir = 'row';
  right.flex.alignItems = 'center';
  right.flex.justifyContent = 'end';
  right.flex.columnGap = 8;
  row.appendChild(right);

  if (thread.unreadCount > 0) {
    right.appendChild(makeThreadBadge(thread.unreadCount));
  }
  right.appendChild(makeIconButton('Delete thread button ' + thread.title, '×', { size: 20, bg: selected ? COLORS.selected : COLORS.surface, color: COLORS.muted, fontSize: 14 }));

  return row;
}

function makeSectionHeader(name, count, collapsed) {
  const row = makeBoard('Section header ' + name, 288, 32, COLORS.surface);
  row.borderRadius = 6;
  row.addFlexLayout();
  row.flex.dir = 'row';
  row.flex.alignItems = 'center';
  row.flex.justifyContent = 'space-between';
  row.flex.horizontalPadding = 8;

  const left = makeBoard('Section header left ' + name, 160, 24, COLORS.surface);
  left.addFlexLayout();
  left.flex.dir = 'row';
  left.flex.alignItems = 'center';
  left.flex.columnGap = 8;
  row.appendChild(left);

  left.appendChild(makeText('Section chevron ' + name, collapsed ? '▶' : '▼', { fontSize: 11, color: COLORS.muted }));
  left.appendChild(makeText('Section label ' + name, name + ' (' + count + ')', { fontSize: 12, color: COLORS.muted }));

  return row;
}

function makeThreadSidebar() {
  const sidebar = makeBoard('Thread sidebar', SIZES.sidebarWidth, 860, COLORS.surface);
  sidebar.addFlexLayout();
  sidebar.flex.dir = 'column';
  sidebar.flex.horizontalPadding = 16;
  sidebar.flex.verticalPadding = 16;
  sidebar.flex.rowGap = 12;
  sidebar.flex.horizontalSizing = 'fix';
  sidebar.flex.verticalSizing = 'fill';

  const header = makeBoard('Thread sidebar header', 288, 24, COLORS.surface);
  header.addFlexLayout();
  header.flex.dir = 'row';
  header.flex.alignItems = 'center';
  header.flex.justifyContent = 'space-between';
  header.flex.horizontalSizing = 'fill';
  sidebar.appendChild(header);

  header.appendChild(makeText('Threads header label', 'Threads', { fontSize: 12, color: COLORS.text, fontWeight: '600' }));
  header.appendChild(makeIconButton('New thread button', '+', { size: 24, bg: COLORS.surface }));

  sidebar.appendChild(makeBoard('Threads divider', 288, 1, COLORS.border));

  const sections = [
    { name: 'Channels', kind: 'channel', prefix: '#', collapsed: false },
    { name: 'Direct messages', kind: 'direct', prefix: '@', collapsed: false },
    { name: 'Chats', kind: 'chat', prefix: '▶', collapsed: true },
  ];

  const threads = [
    { id: 'c1', title: 'General', kind: 'channel', unreadCount: 2, selected: true },
    { id: 'c2', title: 'Engineering', kind: 'channel', unreadCount: 0, selected: false },
    { id: 'd1', title: 'Ada', kind: 'direct', unreadCount: 0, selected: false },
    { id: 'd2', title: 'Bot', kind: 'direct', unreadCount: 5, selected: false },
    { id: 'ch1', title: 'Support', kind: 'chat', unreadCount: 1, selected: false },
  ];

  for (const section of sections) {
    const sectionThreads = threads.filter(t => t.kind === section.kind);
    sidebar.appendChild(makeSectionHeader(section.name, sectionThreads.length, section.collapsed));

    if (!section.collapsed) {
      const list = makeBoard('Thread list ' + section.name, 288, sectionThreads.length * 44, COLORS.surface);
      list.addFlexLayout();
      list.flex.dir = 'column';
      list.flex.rowGap = 4;
      list.flex.horizontalSizing = 'fill';

      for (const thread of sectionThreads) {
        list.appendChild(makeThreadListItem(thread, section.prefix, thread.selected));
      }

      if (sectionThreads.length === 0) {
        list.resize(288, 32);
        const empty = makeBoard('No items row', 288, 32, COLORS.surface);
        empty.addFlexLayout();
        empty.flex.dir = 'row';
        empty.flex.alignItems = 'center';
        empty.flex.horizontalPadding = 12;
        empty.appendChild(makeText('No items label', 'No items', { fontSize: 11, color: COLORS.muted }));
        list.appendChild(empty);
      }

      sidebar.appendChild(list);
    }
  }

  return sidebar;
}

function makeAvatar(name, role) {
  const bg = role === 'user' ? COLORS.accent : (role === 'assistant' ? COLORS.success : COLORS.warning);
  const initials = name.split(/\s+/).map(w => w[0]).join('').slice(0, 2).toUpperCase();
  const avatar = makeBoard('Avatar ' + name, 36, 36, bg);
  avatar.borderRadius = 18;
  avatar.addFlexLayout();
  avatar.flex.dir = 'row';
  avatar.flex.alignItems = 'center';
  avatar.flex.justifyContent = 'center';
  avatar.appendChild(makeText('Avatar initials ' + name, initials, { fontSize: 14, color: COLORS.text }));
  return avatar;
}

function makeGroupChatMessage(author, role, text, showHeader, timestamp) {
  const row = makeBoard('Group message ' + author, 540, showHeader ? 74 : 44, COLORS.bg);
  row.addFlexLayout();
  row.flex.dir = 'row';
  row.flex.alignItems = 'start';
  row.flex.columnGap = 12;
  row.flex.horizontalPadding = 0;

  if (showHeader) {
    row.appendChild(makeAvatar(author, role));
  } else {
    const spacer = makeBoard('Avatar spacer', 36, 1, COLORS.bg);
    spacer.opacity = 0;
    row.appendChild(spacer);
  }

  const rightColumn = makeBoard('Message right column ' + author, 492, showHeader ? 74 : 44, COLORS.bg);
  rightColumn.addFlexLayout();
  rightColumn.flex.dir = 'column';
  rightColumn.flex.rowGap = 4;
  rightColumn.flex.verticalPadding = 2;
  row.appendChild(rightColumn);

  if (showHeader) {
    const header = makeBoard('Message header ' + author, 492, 18, COLORS.bg);
    header.addFlexLayout();
    header.flex.dir = 'row';
    header.flex.alignItems = 'center';
    header.flex.columnGap = 8;
    header.appendChild(makeText('Message author ' + author, author, { fontSize: 12, color: COLORS.text, fontWeight: '600' }));
    if (timestamp) {
      header.appendChild(makeText('Message timestamp ' + author, timestamp, { fontSize: 10, color: COLORS.muted }));
    }
    rightColumn.appendChild(header);
  }

  rightColumn.appendChild(makeText('Message text ' + author, text, { fontSize: 13, color: COLORS.text }));

  return row;
}

function makeThreadView() {
  const view = makeBoard('Thread view', SIZES.threadViewWidth, 860, COLORS.bg);
  view.addFlexLayout();
  view.flex.dir = 'column';
  // No horizontal padding on the view: the header and composer span the full
  // thread width. The messages area carries its own inset padding instead.
  view.flex.horizontalPadding = 0;
  view.flex.verticalPadding = 0;
  view.flex.rowGap = 12;
  view.flex.horizontalSizing = 'fix';
  view.flex.verticalSizing = 'fill';

  const header = makeBoard('Thread view header', SIZES.threadViewWidth, 44, COLORS.surface);
  header.addFlexLayout();
  header.flex.dir = 'row';
  header.flex.alignItems = 'center';
  header.flex.justifyContent = 'space-between';
  header.flex.horizontalPadding = 16;
  header.appendChild(makeText('Thread title', 'General', { fontSize: 13, color: COLORS.text, fontWeight: '600' }));
  header.appendChild(makeIconButton('Thread info button', 'ℹ', { size: 28, bg: COLORS.surface, color: COLORS.muted }));
  view.appendChild(header);

  const messagesArea = makeBoard('Thread messages area', SIZES.threadViewWidth, 632, COLORS.bg);
  messagesArea.addFlexLayout();
  messagesArea.flex.dir = 'column';
  messagesArea.flex.rowGap = 12;
  messagesArea.flex.horizontalPadding = 16;
  messagesArea.flex.verticalPadding = 8;
  messagesArea.flex.horizontalSizing = 'fill';
  messagesArea.flex.verticalSizing = 'fill';
  view.appendChild(messagesArea);

  messagesArea.appendChild(makeGroupChatMessage('Ada', 'assistant', 'Hey team, I pushed the new wgpu renderer.', true, '10:42'));
  messagesArea.appendChild(makeGroupChatMessage('Ada', 'assistant', 'Threaded conversations are now rendered as grouped message bubbles.', false, null));
  messagesArea.appendChild(makeGroupChatMessage('You', 'user', 'Great, how does the sidebar look?', true, '10:43'));
  messagesArea.appendChild(makeGroupChatMessage('Ada', 'assistant', 'It mirrors ThreadSidebar with collapsible sections for Channels, DMs and Chats.', true, '10:43'));

  view.appendChild(makeComposer());

  return view;
}

async function buildThreadsView() {
  let page = penpotUtils.getPageByName('Goble Threads');
  if (!page) {
    page = penpot.createPage();
    page.name = 'Goble Threads';
  }
  await penpot.openPage(page.id);

  const root = page.root;
  for (const child of [...root.children]) {
    try {
      child.remove();
    } catch (e) {}
  }

  const appShell = makeBoard('App shell', 1440, 900, COLORS.bg);
  appShell.addFlexLayout();
  appShell.flex.dir = 'column';
  appShell.flex.horizontalSizing = 'fix';
  appShell.flex.verticalSizing = 'fix';
  root.appendChild(appShell);

  appShell.appendChild(makeTopbar());

  const body = makeBoard('Threads body', 1440, 860, COLORS.bg);
  body.addFlexLayout();
  body.flex.dir = 'row';
  body.flex.horizontalSizing = 'fix';
  body.flex.verticalSizing = 'fix';
  appShell.appendChild(body);

  const sidebar = makeThreadSidebar();
  sidebar.x = 0;
  sidebar.y = 0;
  body.appendChild(sidebar);

  const divider = makeBoard('Threads divider', 1, 860, COLORS.border);
  divider.x = SIZES.sidebarWidth;
  divider.y = 0;
  body.appendChild(divider);

  const threadView = makeThreadView();
  threadView.x = SIZES.sidebarWidth + 1;
  threadView.y = 0;
  body.appendChild(threadView);

  function applyTokenByName(shape, name, props) {
    const token = penpotUtils.findTokenByName(name);
    if (token) {
      shape.applyToken(token, props);
    }
  }

  applyTokenByName(appShell, 'theme.' + THEME + '.bg', ['fill']);
  applyTokenByName(body, 'theme.' + THEME + '.bg', ['fill']);
  applyTokenByName(sidebar, 'theme.' + THEME + '.surface', ['fill']);
  applyTokenByName(threadView, 'theme.' + THEME + '.bg', ['fill']);

  return {
    page: page.name,
    pageId: page.id,
    structure: penpotUtils.shapeStructure(appShell, 2),
  };
}

return buildThreadsView();
