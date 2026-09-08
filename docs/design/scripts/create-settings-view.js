// Create the Goble native Settings view in Penpot.
// Based on crates/goble-ui/src/views/settings_view.rs.
// The LLM page is a model list with an "Add model" button.
// The overlay for adding a model and the catalog of settings row types are
// separate components on the same page (not embedded in the main layout).

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
  const input = makeBoard(name, width, 32, COLORS.surfaceRaised);
  input.borderRadius = 8;
  input.addFlexLayout();
  input.flex.dir = 'row';
  input.flex.alignItems = 'center';
  input.flex.horizontalPadding = 12;
  input.appendChild(makeText(name + ' placeholder', placeholder, { fontSize: 12, color: COLORS.muted }));
  return input;
}

function makeSelect(name, label, width) {
  const select = makeBoard(name, width, 32, COLORS.surfaceRaised);
  select.borderRadius = 8;
  select.addFlexLayout();
  select.flex.dir = 'row';
  select.flex.alignItems = 'center';
  select.flex.justifyContent = 'space-between';
  select.flex.horizontalPadding = 12;
  select.appendChild(makeText(name + ' label', label, { fontSize: 12, color: COLORS.text }));
  select.appendChild(makeText(name + ' caret', '▾', { fontSize: 10, color: COLORS.muted }));
  return select;
}

function makeSwitch(name, checked) {
  const width = 40;
  const height = 22;
  const track = makeBoard(name, width, height, checked ? COLORS.accent : COLORS.border);
  track.borderRadius = height / 2;
  const thumb = makeBoard(name + ' thumb', 18, 18, COLORS.text);
  thumb.borderRadius = 9;
  thumb.x = checked ? width - 20 : 2;
  thumb.y = 2;
  track.appendChild(thumb);
  return track;
}

function makeButton(name, label, width, primary) {
  const btn = makeBoard(name, width, 32, primary ? COLORS.accent : COLORS.surface);
  btn.borderRadius = 8;
  btn.addFlexLayout();
  btn.flex.dir = 'row';
  btn.flex.alignItems = 'center';
  btn.flex.justifyContent = 'center';
  btn.appendChild(makeText(name + ' label', label, { fontSize: 12, color: primary ? '#0E0E0E' : COLORS.text }));
  return btn;
}

function makeSettingsRow(name, label, control, options = {}) {
  const row = makeBoard('Settings row ' + name, 920, 'auto' ? 64 : 64, COLORS.surface);
  row.borderRadius = 8;
  row.addFlexLayout();
  row.flex.dir = 'row';
  row.flex.alignItems = 'center';
  row.flex.justifyContent = 'space-between';
  row.flex.horizontalPadding = 16;
  row.flex.verticalPadding = 12;
  row.flex.horizontalSizing = 'fill';

  const labelCol = makeBoard('Label col ' + name, 400, 'auto' ? 40 : 40, COLORS.surface);
  labelCol.addFlexLayout();
  labelCol.flex.dir = 'column';
  labelCol.flex.verticalPadding = 4;
  labelCol.flex.rowGap = 2;
  labelCol.flex.horizontalSizing = 'auto';

  const titleRow = makeBoard('Title row ' + name, 300, 20, COLORS.surface);
  titleRow.addFlexLayout();
  titleRow.flex.dir = 'row';
  titleRow.flex.alignItems = 'center';
  titleRow.flex.columnGap = 4;
  titleRow.appendChild(makeText('Title ' + name, label, { fontSize: 12, color: COLORS.text }));

  if (options.tooltip) {
    titleRow.appendChild(makeText('Tooltip icon ' + name, 'ⓘ', { fontSize: 11, color: COLORS.muted }));
  }
  if (options.localOnly) {
    titleRow.appendChild(makeText('Local icon ' + name, '☁', { fontSize: 11, color: COLORS.muted }));
  }
  if (options.secondary) {
    titleRow.appendChild(makeText('Secondary ' + name, options.secondary, { fontSize: 12, color: COLORS.muted }));
  }

  labelCol.appendChild(titleRow);
  if (options.description) {
    labelCol.appendChild(makeText('Description ' + name, options.description, { fontSize: 11, color: COLORS.muted }));
  }

  row.appendChild(labelCol);
  row.appendChild(control);
  return row;
}

function makeModelCard(model) {
  const card = makeBoard('Model card ' + model.id, 920, 96, COLORS.surface);
  card.borderRadius = 8;
  card.addFlexLayout();
  card.flex.dir = 'column';
  card.flex.horizontalPadding = 16;
  card.flex.verticalPadding = 12;
  card.flex.rowGap = 8;
  card.flex.horizontalSizing = 'fill';

  const header = makeBoard('Model card header ' + model.id, 888, 20, COLORS.surface);
  header.addFlexLayout();
  header.flex.dir = 'row';
  header.flex.alignItems = 'center';
  header.flex.justifyContent = 'space-between';
  header.flex.horizontalSizing = 'fill';
  header.appendChild(makeText('Model provider ' + model.id, model.provider, { fontSize: 13, color: COLORS.text, fontWeight: '600' }));
  header.appendChild(makeText('Model endpoint ' + model.id, model.endpoint, { fontSize: 11, color: COLORS.muted }));
  card.appendChild(header);

  const keyRow = makeBoard('Model key row ' + model.id, 888, 16, COLORS.surface);
  keyRow.addFlexLayout();
  keyRow.flex.dir = 'row';
  keyRow.flex.alignItems = 'center';
  keyRow.flex.columnGap = 4;
  keyRow.appendChild(makeText('Key label ' + model.id, 'Key:', { fontSize: 11, color: COLORS.muted }));
  keyRow.appendChild(makeText('Key value ' + model.id, model.key, { fontSize: 11, color: COLORS.text }));
  card.appendChild(keyRow);

  const tags = makeBoard('Model tags ' + model.id, 888, 24, COLORS.surface);
  tags.addFlexLayout();
  tags.flex.dir = 'row';
  tags.flex.alignItems = 'center';
  tags.flex.columnGap = 8;
  for (const m of model.models) {
    const tag = makeBoard('Model tag ' + model.id + ' ' + m.name, 'auto' ? 80 : 80, 24, COLORS.surfaceRaised);
    tag.borderRadius = 6;
    tag.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
    tag.addFlexLayout();
    tag.flex.dir = 'row';
    tag.flex.alignItems = 'center';
    tag.flex.horizontalPadding = 8;
    tag.flex.columnGap = 4;
    tag.appendChild(makeText('Tag name ' + m.name, m.name, { fontSize: 11, color: COLORS.text }));
    tag.appendChild(makeText('Tag alias ' + m.alias, '→ ' + m.alias, { fontSize: 11, color: COLORS.muted }));
    tags.appendChild(tag);
  }
  card.appendChild(tags);

  return card;
}

function makeAddModelOverlay() {
  const overlay = makeBoard('Add model overlay', 520, 540, COLORS.surface);
  overlay.borderRadius = 8;
  overlay.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  overlay.addFlexLayout();
  overlay.flex.dir = 'column';
  overlay.flex.horizontalPadding = 24;
  overlay.flex.verticalPadding = 24;
  overlay.flex.rowGap = 16;

  overlay.appendChild(makeText('Overlay title', 'Add model', { fontSize: 18, color: COLORS.text, fontWeight: '600' }));
  overlay.appendChild(makeSettingsRow('API schema', 'API schema', makeSelect('Schema select', 'OpenAI ▾', 280), { description: 'The provider schema for this endpoint.' }));
  overlay.appendChild(makeSettingsRow('Endpoint URL', 'Endpoint URL', makeInput('Endpoint input', 'https://api.openai.com/v1', 280), { description: 'Base URL for the provider API.' }));
  overlay.appendChild(makeSettingsRow('API key', 'API key', makeInput('API key input', 'sk-...', 280), { description: 'Stored locally and never sent to the cluster.', localOnly: true }));

  // Model name / alias block with an "Add model" button to add more pairs.
  const modelsSection = makeBoard('Models section', 472, 160, COLORS.surface);
  modelsSection.borderRadius = 8;
  modelsSection.strokes = [{ strokeColor: COLORS.border, strokeWidth: 1, strokeStyle: 'solid', strokeAlignment: 'inner' }];
  modelsSection.addFlexLayout();
  modelsSection.flex.dir = 'column';
  modelsSection.flex.horizontalPadding = 16;
  modelsSection.flex.verticalPadding = 16;
  modelsSection.flex.rowGap = 12;
  modelsSection.flex.horizontalSizing = 'fill';

  modelsSection.appendChild(makeText('Models section title', 'Models', { fontSize: 13, color: COLORS.text, fontWeight: '600' }));

  const pairRow = makeBoard('Model pair row', 440, 32, COLORS.surface);
  pairRow.addFlexLayout();
  pairRow.flex.dir = 'row';
  pairRow.flex.alignItems = 'center';
  pairRow.flex.columnGap = 8;
  pairRow.flex.horizontalSizing = 'fill';
  pairRow.appendChild(makeInput('Model name input', 'Model name, e.g. gpt-4o', 208));
  pairRow.appendChild(makeInput('Model alias input', 'Alias, e.g. default', 208));
  modelsSection.appendChild(pairRow);

  modelsSection.appendChild(makeButton('Add model pair button', 'Add model', 120, false));
  overlay.appendChild(modelsSection);

  overlay.appendChild(makeButton('Save model button', 'Add model', 120, true));

  return overlay;
}

async function buildSettings() {
  let page = penpotUtils.getPageByName('Goble Settings');
  if (!page) {
    page = penpot.createPage();
    page.name = 'Goble Settings';
  }
  await penpot.openPage(page.id);

  const root = page.root;
  for (const child of [...root.children]) {
    try {
      child.remove();
    } catch (e) {}
  }

  // App shell with topbar only (no sidebar for Settings)
  const appShell = makeBoard('App shell', 1440, 900, COLORS.bg);
  appShell.addFlexLayout();
  appShell.flex.dir = 'column';
  appShell.flex.horizontalSizing = 'fix';
  appShell.flex.verticalSizing = 'fix';
  root.appendChild(appShell);

  const topbar = makeBoard('Topbar', 1440, 40, COLORS.surface);
  topbar.addFlexLayout();
  topbar.flex.dir = 'row';
  topbar.flex.alignItems = 'center';
  topbar.flex.justifyContent = 'space-between';
  topbar.flex.horizontalPadding = 12;
  appShell.appendChild(topbar);

  const topbarLeft = makeBoard('Topbar left', 120, 40, COLORS.surface);
  topbarLeft.addFlexLayout();
  topbarLeft.flex.dir = 'row';
  topbarLeft.flex.alignItems = 'center';
  topbarLeft.flex.columnGap = 8;
  topbar.appendChild(topbarLeft);
  topbarLeft.appendChild(makeIconButton('Menu button', '☰', { bg: COLORS.surface }));
  topbarLeft.appendChild(makeIconButton('Threads button', '✉', { bg: COLORS.surface }));

  const topbarRight = makeBoard('Topbar right', 120, 40, COLORS.surface);
  topbarRight.addFlexLayout();
  topbarRight.flex.dir = 'row';
  topbarRight.flex.alignItems = 'center';
  topbarRight.flex.justifyContent = 'end';
  topbarRight.flex.columnGap = 8;
  topbar.appendChild(topbarRight);
  topbarRight.appendChild(makeIconButton('Inbox button', '▤', { bg: COLORS.surface }));
  topbarRight.appendChild(makeIconButton('Settings button', '⚙', { bg: COLORS.selected }));

  // Settings content area
  const body = makeBoard('Settings body', 1440, 860, COLORS.bg);
  body.addFlexLayout();
  body.flex.dir = 'row';
  body.flex.horizontalPadding = 24;
  body.flex.verticalPadding = 24;
  body.flex.columnGap = 24;
  body.flex.horizontalSizing = 'fill';
  body.flex.verticalSizing = 'fill';
  appShell.appendChild(body);

  // Left nav
  const nav = makeBoard('Settings nav', 200, 812, COLORS.bg);
  nav.addFlexLayout();
  nav.flex.dir = 'column';
  nav.flex.rowGap = 4;
  nav.flex.horizontalSizing = 'fix';
  nav.flex.verticalSizing = 'fill';
  body.appendChild(nav);

  const navItems = ['Profile', 'LLM', 'Appearance', 'Account', 'Cluster', 'Workers', 'Keys'];
  for (const item of navItems) {
    const selected = item === 'LLM';
    const row = makeBoard('Nav item ' + item, 200, 36, selected ? COLORS.selected : COLORS.bg);
    row.borderRadius = 8;
    row.addFlexLayout();
    row.flex.dir = 'row';
    row.flex.alignItems = 'center';
    row.flex.horizontalPadding = 12;
    row.appendChild(makeText('Nav label ' + item, item, { fontSize: 13, color: selected ? COLORS.text : COLORS.muted, fontWeight: selected ? '600' : '400' }));
    nav.appendChild(row);
  }

  // Right pane (LLM page shown by default)
  const pane = makeBoard('Settings pane', 1172, 812, COLORS.bg);
  pane.addFlexLayout();
  pane.flex.dir = 'column';
  pane.flex.rowGap = 16;
  pane.flex.horizontalSizing = 'fill';
  pane.flex.verticalSizing = 'fill';
  body.appendChild(pane);

  // LLM page header
  const llmHeader = makeBoard('LLM header', 1172, 40, COLORS.bg);
  llmHeader.addFlexLayout();
  llmHeader.flex.dir = 'row';
  llmHeader.flex.alignItems = 'center';
  llmHeader.flex.justifyContent = 'space-between';
  llmHeader.flex.horizontalSizing = 'fill';
  llmHeader.appendChild(makeText('LLM title', 'LLM Models', { fontSize: 18, color: COLORS.text, fontWeight: '600' }));
  llmHeader.appendChild(makeButton('Add model button', 'Add model', 120, true));
  pane.appendChild(llmHeader);

  // Model list
  const models = [
    {
      id: 'openai',
      provider: 'OpenAI',
      endpoint: 'https://api.openai.com/v1',
      key: 'sk-••••••••',
      models: [{ name: 'gpt-4o', alias: 'default' }, { name: 'gpt-4o-mini', alias: 'fast' }],
    },
    {
      id: 'anthropic',
      provider: 'Anthropic',
      endpoint: 'https://api.anthropic.com',
      key: 'sk-••••••••',
      models: [{ name: 'claude-3-5-sonnet', alias: 'default' }],
    },
  ];

  const modelList = makeBoard('Model list', 920, 'auto' ? 220 : 220, COLORS.bg);
  modelList.addFlexLayout();
  modelList.flex.dir = 'column';
  modelList.flex.rowGap = 12;
  modelList.flex.horizontalSizing = 'fill';
  for (const model of models) {
    modelList.appendChild(makeModelCard(model));
  }
  pane.appendChild(modelList);

  // Catalog of all settings row types
  const rowTypesSection = makeBoard('Row types section', 1172, 'auto' ? 400 : 400, COLORS.bg);
  rowTypesSection.addFlexLayout();
  rowTypesSection.flex.dir = 'column';
  rowTypesSection.flex.rowGap = 16;
  rowTypesSection.flex.horizontalSizing = 'fill';
  rowTypesSection.appendChild(makeText('Row types title', 'Settings row types', { fontSize: 18, color: COLORS.text, fontWeight: '600' }));

  rowTypesSection.appendChild(makeSettingsRow('Text input', 'Name', makeInput('Name input', 'Ada Lovelace', 280), {
    description: 'Plain text input row.',
  }));
  rowTypesSection.appendChild(makeSettingsRow('Select', 'Provider', makeSelect('Provider select', 'OpenAI ▾', 200), {
    description: 'Dropdown/select row.',
  }));
  rowTypesSection.appendChild(makeSettingsRow('Switch', 'Dark mode', makeSwitch('Dark mode switch', true), {
    description: 'Toggle row.',
  }));
  rowTypesSection.appendChild(makeSettingsRow('Button', 'Action', makeButton('Action button', 'Run', 100, false), {
    description: 'Button as the control of the row.',
  }));
  rowTypesSection.appendChild(makeSettingsRow('Tooltip', 'Temperature', makeInput('Temperature input', '0.7', 120), {
    tooltip: true,
    secondary: '0.0 - 2.0',
    description: 'Row with an info tooltip and secondary text.',
  }));
  rowTypesSection.appendChild(makeSettingsRow('Local only', 'API key', makeInput('API key input', 'sk-...', 280), {
    localOnly: true,
    description: 'Row marked as local-only with a cloud-off icon.',
  }));

  pane.appendChild(rowTypesSection);

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
  applyTokenByName(nav, 'theme.' + THEME + '.bg', ['fill']);
  applyTokenByName(pane, 'theme.' + THEME + '.bg', ['fill']);

  // Separate component: add model overlay
  const overlay = makeAddModelOverlay();
  overlay.name = 'Add model overlay';
  overlay.x = 1500;
  overlay.y = 0;
  root.appendChild(overlay);

  return {
    page: page.name,
    pageId: page.id,
    structure: penpotUtils.shapeStructure(appShell, 2),
  };
}

return buildSettings();
