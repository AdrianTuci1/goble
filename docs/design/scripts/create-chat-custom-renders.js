// Create separate components for custom chat renders on the Goble Native App page.
// Mirrors crates/goble-ui/src/views/chat_view.rs, ask_user.rs, chat_message_bubble.rs
// and terminal_block.rs. These are placed outside the main app shell at x=1500.

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

function makeInput(name, placeholder, width) {
  const input = makeBoard(name, width, 36, COLORS.surfaceRaised);
  input.borderRadius = 8;
  input.addFlexLayout();
  input.flex.dir = 'row';
  input.flex.alignItems = 'center';
  input.flex.horizontalPadding = 12;
  input.appendChild(makeText(name + ' placeholder', placeholder, { fontSize: 12, color: COLORS.muted }));
  return input;
}

function makeCheckbox(name, label, width) {
  const row = makeBoard(name, width, 28, COLORS.surface);
  row.addFlexLayout();
  row.flex.dir = 'row';
  row.flex.alignItems = 'center';
  row.flex.columnGap = 8;
  const box = makeBoard(name + ' box', 16, 16, COLORS.surfaceRaised);
  box.borderRadius = 4;
  box.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  row.appendChild(box);
  row.appendChild(makeText(name + ' label', label, { fontSize: 12, color: COLORS.text }));
  return row;
}

function makeButton(name, label, width, primary) {
  const btn = makeBoard(name, width, 32, primary ? COLORS.accent : COLORS.surface);
  btn.borderRadius = 8;
  btn.addFlexLayout();
  btn.flex.dir = 'row';
  btn.flex.alignItems = 'center';
  btn.flex.justifyContent = 'center';
  btn.appendChild(makeText(name + ' label', label, { fontSize: 12, color: primary ? COLORS.bg : COLORS.text }));
  return btn;
}

function makeAskUserCard() {
  const card = makeBoard('AskUserCard', 520, 360, COLORS.surface);
  card.borderRadius = 8;
  card.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  card.addFlexLayout();
  card.flex.dir = 'column';
  card.flex.horizontalPadding = 16;
  card.flex.verticalPadding = 16;
  card.flex.rowGap = 12;

  const header = makeBoard('AskUser header', 488, 24, COLORS.surface);
  header.addFlexLayout();
  header.flex.dir = 'row';
  header.flex.alignItems = 'center';
  header.flex.justifyContent = 'space-between';
  const titleRow = makeBoard('AskUser title row', 200, 24, COLORS.surface);
  titleRow.addFlexLayout();
  titleRow.flex.dir = 'row';
  titleRow.flex.alignItems = 'center';
  titleRow.flex.columnGap = 8;
  titleRow.appendChild(makeText('Warning icon', '▲', { fontSize: 14, color: COLORS.warning }));
  titleRow.appendChild(makeText('AskUser title', 'Agent asks', { fontSize: 13, color: COLORS.text, fontWeight: '600' }));
  header.appendChild(titleRow);
  header.appendChild(makeButton('Skip button', 'Skip', 52, false));
  card.appendChild(header);

  card.appendChild(makeText('AskUser question', 'Which environment should I deploy to?', { fontSize: 12, color: COLORS.text }));

  card.appendChild(makeCheckbox('Reply production', 'Production', 488));
  card.appendChild(makeCheckbox('Reply staging', 'Staging', 488));

  card.appendChild(makeInput('AskUser answer input', 'Or type an answer…', 488));

  const credName = makeInput('Credential name input', 'Credential name (e.g. github_token)…', 488);
  card.appendChild(credName);

  const credRow = makeBoard('Credential row', 488, 40, COLORS.surface);
  credRow.addFlexLayout();
  credRow.flex.dir = 'row';
  credRow.flex.alignItems = 'center';
  credRow.flex.columnGap = 8;
  credRow.appendChild(makeText('Key icon', '🔑', { fontSize: 14, color: COLORS.muted }));
  const credValue = makeInput('Credential value input', 'Credential (optional)…', 466);
  credRow.appendChild(credValue);
  card.appendChild(credRow);

  const footer = makeBoard('AskUser footer', 488, 32, COLORS.surface);
  footer.addFlexLayout();
  footer.flex.dir = 'row';
  footer.flex.alignItems = 'center';
  footer.flex.justifyContent = 'end';
  footer.appendChild(makeButton('Send answer button', 'Send answer', 100, true));
  card.appendChild(footer);

  return card;
}

function makeTerminalBlock() {
  const card = makeBoard('TerminalBlock', 520, 180, COLORS.surfaceRaised);
  card.borderRadius = 8;
  card.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  card.addFlexLayout();
  card.flex.dir = 'column';
  card.flex.horizontalPadding = 12;
  card.flex.verticalPadding = 10;
  card.flex.rowGap = 8;

  const header = makeBoard('Terminal header', 496, 20, COLORS.surfaceRaised);
  header.addFlexLayout();
  header.flex.dir = 'row';
  header.flex.alignItems = 'center';
  header.flex.justifyContent = 'space-between';
  const titleRow = makeBoard('Terminal title row', 200, 20, COLORS.surfaceRaised);
  titleRow.addFlexLayout();
  titleRow.flex.dir = 'row';
  titleRow.flex.alignItems = 'center';
  titleRow.flex.columnGap = 8;
  titleRow.appendChild(makeText('Terminal icon', '▸_', { fontSize: 12, color: COLORS.muted }));
  titleRow.appendChild(makeText('Terminal title', 'npm run build', { fontSize: 12, color: COLORS.muted }));
  header.appendChild(titleRow);
  header.appendChild(makeText('Terminal status', 'done', { fontSize: 12, color: COLORS.success }));
  card.appendChild(header);

  card.appendChild(makeText('Terminal command', '❯ npm run build', { fontSize: 13, color: COLORS.accent }));
  card.appendChild(makeText('Terminal output', 'Compiled successfully in 1.2s', { fontSize: 13, color: COLORS.text }));
  card.appendChild(makeText('Terminal success', 'test result: ok. 42 passed', { fontSize: 13, color: COLORS.success }));
  card.appendChild(makeText('Terminal warning', 'warning: unused variable', { fontSize: 13, color: COLORS.warning }));

  return card;
}

function makeToolCallCard() {
  const card = makeBoard('ToolCall card', 520, 60, COLORS.surfaceRaised);
  card.borderRadius = 8;
  card.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  card.addFlexLayout();
  card.flex.dir = 'row';
  card.flex.alignItems = 'center';
  card.flex.columnGap = 12;
  card.flex.horizontalPadding = 16;
  card.flex.verticalPadding = 12;

  card.appendChild(makeText('CPU icon', '◈', { fontSize: 14, color: COLORS.muted }));
  const meta = makeBoard('ToolCall meta', 200, 32, COLORS.surfaceRaised);
  meta.addFlexLayout();
  meta.flex.dir = 'column';
  meta.flex.rowGap = 2;
  meta.appendChild(makeText('ToolCall label', 'tool', { fontSize: 10, color: COLORS.muted }));
  meta.appendChild(makeText('ToolCall name', 'ls {}', { fontSize: 12, color: COLORS.text }));
  card.appendChild(meta);

  return card;
}

function makeQueuedPromptCard() {
  const card = makeBoard('Queued prompt card', 520, 120, COLORS.surface);
  card.borderRadius = 8;
  card.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  card.addFlexLayout();
  card.flex.dir = 'column';
  card.flex.horizontalPadding = 16;
  card.flex.verticalPadding = 12;
  card.flex.rowGap = 8;

  const header = makeBoard('Queued header', 488, 24, COLORS.surface);
  header.addFlexLayout();
  header.flex.dir = 'row';
  header.flex.alignItems = 'center';
  header.flex.justifyContent = 'space-between';
  header.appendChild(makeText('Queued title', 'Pending message', { fontSize: 12, color: COLORS.muted }));
  header.appendChild(makeIconButton('Dismiss queued button', '✕', { size: 20, bg: COLORS.surface, color: COLORS.muted, fontSize: 12 }));
  card.appendChild(header);

  card.appendChild(makeText('Queued prompt text', 'Run the full test suite and report any failures.', { fontSize: 13, color: COLORS.text }));

  const footer = makeBoard('Queued footer', 488, 32, COLORS.surface);
  footer.addFlexLayout();
  footer.flex.dir = 'row';
  footer.flex.alignItems = 'center';
  footer.flex.justifyContent = 'end';
  footer.appendChild(makeButton('Send now button', 'Send now', 90, true));
  card.appendChild(footer);

  return card;
}

function makeCredentialComposer() {
  const card = makeBoard('Credential composer', 520, 280, COLORS.surfaceRaised);
  card.borderRadius = 8;
  card.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  card.addFlexLayout();
  card.flex.dir = 'column';
  card.flex.horizontalPadding = 16;
  card.flex.verticalPadding = 16;
  card.flex.rowGap = 12;

  card.appendChild(makeText('Composer placeholder', 'Ask anything...', { fontSize: 13, color: COLORS.muted }));

  const footer = makeBoard('Credential composer footer', 488, 28, COLORS.surfaceRaised);
  footer.addFlexLayout();
  footer.flex.dir = 'row';
  footer.flex.alignItems = 'center';
  footer.flex.justifyContent = 'space-between';
  card.appendChild(footer);

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
  footer.appendChild(attachPill);

  footer.appendChild(makeButton('Send answer button', 'Send answer', 100, true));

  card.appendChild(makeInput('Credential name input', 'Credential name (e.g. github_token)…', 488));

  const credRow = makeBoard('Credential row', 488, 40, COLORS.surfaceRaised);
  credRow.addFlexLayout();
  credRow.flex.dir = 'row';
  credRow.flex.alignItems = 'center';
  credRow.flex.columnGap = 8;
  credRow.appendChild(makeText('Key icon', '🔑', { fontSize: 14, color: COLORS.muted }));
  credRow.appendChild(makeInput('Credential value input', 'Credential...', 466));
  card.appendChild(credRow);

  return card;
}

async function buildCustomRenders() {
  let page = penpotUtils.getPageByName('Goble Native App');
  if (!page) {
    page = penpot.createPage();
    page.name = 'Goble Native App';
  }
  await penpot.openPage(page.id);

  // Clean up previous custom-render components placed outside the shell.
  const root = page.root;
  const toRemove = [];
  for (const child of root.children) {
    if (child.name && (
      child.name === 'AskUserCard' ||
      child.name === 'TerminalBlock' ||
      child.name === 'ToolCall card' ||
      child.name === 'Queued prompt card' ||
      child.name === 'Credential composer'
    )) {
      toRemove.push(child);
    }
  }
  for (const child of toRemove) {
    try {
      child.remove();
    } catch (e) {}
  }

  const components = [
    { builder: makeAskUserCard, x: 1500, y: 450 },
    { builder: makeTerminalBlock, x: 1500, y: 830 },
    { builder: makeToolCallCard, x: 1500, y: 1020 },
    { builder: makeQueuedPromptCard, x: 1500, y: 1090 },
    { builder: makeCredentialComposer, x: 1500, y: 1220 },
  ];

  const created = [];
  for (const item of components) {
    const shape = item.builder();
    shape.x = item.x;
    shape.y = item.y;
    root.appendChild(shape);
    created.push({ name: shape.name, x: shape.x, y: shape.y, id: shape.id });
  }

  function applyTokenByName(shape, name, props) {
    const token = penpotUtils.findTokenByName(name);
    if (token) {
      shape.applyToken(token, props);
    }
  }

  for (const item of created) {
    const shape = penpotUtils.findShapeById(item.id);
    if (shape) {
      applyTokenByName(shape, 'theme.' + THEME + '.surface', ['fill']);
    }
  }

  return {
    page: page.name,
    created: created,
  };
}

return buildCustomRenders();
