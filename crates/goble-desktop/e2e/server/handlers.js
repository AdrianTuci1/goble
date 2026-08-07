import EventEmitter from 'events';
import { DeepSeekHarness } from './harness.js';

let nextId = 1;

function uid() {
  return `e2e-${nextId++}`;
}
const realHarness = new DeepSeekHarness();

const state = {
  providers: {},
  workers: [],
  agents: [],
  mcpServers: [],
  chats: [],
  messages: {},
  vaultSecrets: {},
  vaultPassphrase: undefined,
  deviceIdentity: undefined,
  clusterIdentity: undefined,
  pendingMessages: [], // messages sent before a model was configured
  submissions: [],
  mode: process.env.GOBLE_E2E_REAL === '1' ? 'real' : 'mock',
};

const emitter = new EventEmitter();

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function broadcast(event, payload) {
  emitter.emit(event, payload);
}

function classifyIntentMock(text) {
  const lower = text.toLowerCase();
  if (lower.startsWith('/')) {
    const parts = lower.slice(1).split(' ');
    const command = parts[0];
    switch (command) {
      case 'create_agent':
      case 'agent':
        return { intent: 'create_agent', params: { name: parts[1] || 'New agent', prompt: text.slice(command.length + 2) } };
      case 'install_mcp':
      case 'mcp':
        return { intent: 'install_mcp', params: { source: 'npm', value: parts[1] || '@modelcontextprotocol/server-everything' } };
      case 'schedule':
      case 'schedule_agent':
        return { intent: 'schedule_agent', params: { agent: parts[1] || '', expression: parts[2] || '0 9 * * *' } };
      case 'workflow':
      case 'create_workflow':
        return { intent: 'create_workflow', params: { name: parts[1] || 'New workflow', agents: parts.slice(2) } };
      default:
        return { intent: 'chat', params: { message: text } };
    }
  }
  if (lower.includes('create agent') || lower.includes('make an agent') || lower.includes('new agent')) {
    return { intent: 'create_agent', params: { name: 'Recruiter', prompt: text, tools: ['web_search'] } };
  }
  if (lower.includes('mcp') || lower.includes('connector') || lower.includes('install')) {
    return { intent: 'install_mcp', params: { source: 'npm', value: '@modelcontextprotocol/server-everything' } };
  }
  if (lower.includes('schedule') || lower.includes('cron') || lower.includes('repeat')) {
    return { intent: 'schedule_agent', params: { agent: '', expression: '0 9 * * *' } };
  }
  if (lower.includes('workflow') || lower.includes('pipeline')) {
    return { intent: 'create_workflow', params: { name: 'Pipeline', agents: [] } };
  }
  return { intent: 'chat', params: { message: text } };
}

async function streamText(chatId, text) {
  const words = text.split(' ');
  for (const word of words) {
    broadcast('harness:event', { chat_id: chatId, event: { type: 'AssistantDelta', payload: word + ' ' } });
    await delay(10);
  }
  broadcast('harness:event', { chat_id: chatId, event: { type: 'AssistantDelta', payload: '' } });
  broadcast('harness:event', { chat_id: chatId, event: { type: 'Done' } });
}

async function simulateHarness(chatId, prompt) {
  const lower = (prompt || '').toLowerCase();

  // Test-mode prompts are always handled locally so specs can verify UI behaviors
  // independently of whether a real LLM provider is available.
  if (lower.startsWith('[test:hello]')) {
    await streamText(chatId, 'Hello! I am ready to help.');
    return;
  }
  if (lower.startsWith('[test:variant]')) {
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'AskUser',
        question: 'Which approach should I use?',
        quick_replies: ['Use web search', 'Use local files', 'Ask for clarification'],
      },
    });
    return;
  }
  if (lower.startsWith('[test:form]')) {
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'AskUser',
        question: 'Remote runtime credentials',
        fields: [
          { name: 'host', label: 'Host', type: 'text' },
          { name: 'user', label: 'User', type: 'text' },
          { name: 'private_key', label: 'SSH private key', type: 'password' },
        ],
      },
    });
    return;
  }
  if (lower.startsWith('[test:secret]')) {
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'AskUser',
        question: 'Authorize the MCP server',
        fields: [
          { name: 'mcp_id', label: 'MCP server', type: 'text' },
          { name: 'api_key', label: 'API key', type: 'password' },
          { name: 'scope', label: 'Scope', type: 'text' },
        ],
      },
    });
    return;
  }
  if (lower.startsWith('[test:agent]')) {
    const name = prompt.replace(/^\[test:agent\]\s*/i, '').trim() || 'Test agent';
    const id = uid();
    state.agents.push({ id, name, description: `${name} agent created via test harness.`, prompt, tools: [] });
    await streamText(chatId, `Created agent "${name}".`);
    return;
  }
  if (lower.startsWith('[test:mcp]')) {
    const requested = prompt.replace(/^\[test:mcp\]\s*/i, '').trim().toLowerCase() || 'github';
    const presetId = requested;
    await streamText(chatId, `I can install the ${presetId} MCP server for you. I'll need an API key to authenticate it.`);
    await delay(100);
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'AskUser',
        question: `Install ${presetId} MCP server`,
        metadata: { mcp: presetId },
        fields: [
          { name: 'api_key', label: 'API key', type: 'password' },
          { name: 'scope', label: 'Scope', type: 'text' },
        ],
      },
    });
    return;
  }

  if (state.mode === 'real') {
    try {
      for await (const ev of realHarness.stream(prompt)) {
        broadcast('harness:event', { chat_id: chatId, event: ev });
      }
    } catch (err) {
      console.error('[e2e harness] real model error', err);
      broadcast('harness:event', { chat_id: chatId, event: { type: 'Error', message: err.message } });
    }
    return;
  }

  if (lower.includes('onboarding') || lower.includes('intro') || lower.includes('overview') || lower.includes('what can you do')) {
    await streamText(chatId, 'Welcome! I am Goble, your AI operations assistant. I can help you configure models, set up local or remote runtimes, create agents, install MCP connectors, and run workflows. What would you like to do first?');
    return;
  }

  if (lower.includes('remote') || lower.includes('runtime') || lower.includes('worker') || lower.includes('vps')) {
    await streamText(chatId, 'To configure a remote runtime, please provide the SSH credentials in the composer below.');
    await delay(100);
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'AskUser',
        question: 'Remote runtime credentials',
        fields: [
          { name: 'host', label: 'Host', type: 'text' },
          { name: 'user', label: 'User', type: 'text' },
          { name: 'private_key', label: 'SSH private key', type: 'password' },
        ],
      },
    });
    return;
  }

  if (lower.includes('variant') || lower.includes('choose') || lower.includes('option') || lower.includes('multiple')) {
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'AskUser',
        question: 'Which approach should I use?',
        quick_replies: ['Use web search', 'Use local files', 'Ask for clarification'],
      },
    });
    return;
  }

  // Generic MCP discovery / install intent for real-mode and full mock flow.
  if (lower.startsWith('find mcp') || lower.startsWith('search mcp')) {
    const query = prompt.replace(/^.*?mcp\s*/i, '').trim() || 'github';
    await streamText(chatId, `Searching MCP registry for "${query}"...`);
    await delay(100);
    const results = await searchMcpRegistry(query);
    if (results.length === 0) {
      await streamText(chatId, `No MCP servers found for "${query}".`);
      return;
    }
    const top = results.slice(0, 5);
    state.submissions.push({ type: 'mcp_search_results', results: top, ts: Date.now() });
    const summary = top.map((r, i) => `${i + 1}. **${r.name}** - ${r.description || 'no description'}`).join('\n');
    await streamText(chatId, `Found ${results.length} servers. Top results:\n${summary}`);
    // For a single strong match that requires auth, ask to install.
    const best = top.find((r) => r.auth_required) || top[0];
    await delay(50);
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'AskUser',
        question: `Install ${best.name}?`,
        metadata: { mcp: best.id, name: best.name, source: best.source_kind, source_value: best.name, install: true },
        quick_replies: ['Install', 'Cancel'],
      },
    });
    return;
  }

  if (lower.startsWith('install mcp') || lower.startsWith('add mcp')) {
    const requested = prompt.replace(/^.*?mcp\s*/i, '').trim() || 'github';
    const searchQuery = requested.replace(/^@modelcontextprotocol\//, '').replace(/^server-/, '');
    const results = await searchMcpRegistry(searchQuery);
    const match = results.find((r) => r.id === requested || r.name === requested || r.name.includes(requested));
    const mcpId = match ? match.id : requested;
    const mcpName = match ? match.name : requested;
    const source = match ? match.source_kind : 'npm';
    const sourceValue = match ? match.name : `@modelcontextprotocol/server-${requested}`;
    const requiresAuth = match ? match.auth_required : true;
    if (requiresAuth) {
      await streamText(chatId, `I can install the ${mcpName} MCP server for you. I'll need an API key to authenticate it.`);
      await delay(100);
      broadcast('harness:event', {
        chat_id: chatId,
        event: {
          type: 'AskUser',
          question: `Install ${mcpName} MCP server`,
          metadata: { mcp: mcpId, name: mcpName, source, source_value: sourceValue, install: true },
          fields: [
            { name: 'api_key', label: 'API key', type: 'password' },
            { name: 'scope', label: 'Scope', type: 'text' },
          ],
        },
      });
    } else {
      installMcpFromChat(chatId, mcpId, mcpName, source, sourceValue, {});
    }
    return;
  }

  if (lower.includes('cv') || lower.includes('role') || lower.includes('job') || lower.includes('recruiter')) {
    await streamText(chatId, 'I will search for roles that match your CV and compile a shortlist. Starting web search now.');
    await delay(100);
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'ToolCallStarted',
        name: 'web_search',
        arguments: { query: 'remote software engineer roles matching CV skills' },
      },
    });
    await delay(100);
    broadcast('harness:event', { chat_id: chatId, event: { type: 'AssistantDelta', payload: 'Found 3 roles. ' } });
    await delay(50);
    broadcast('harness:event', { chat_id: chatId, event: { type: 'Done' } });
    return;
  }

  if (lower.includes('server') || lower.includes('node') || lower.includes('nodejs') || lower.includes('express')) {
    await streamText(chatId, 'I will build a Node.js server with the components you requested. Here is the plan:');
    await delay(100);
    broadcast('harness:event', {
      chat_id: chatId,
      event: {
        type: 'AskUser',
        question: 'Which components should the server include?',
        quick_replies: ['Express + SQLite', 'Express + PostgreSQL', 'Fastify + Prisma'],
      },
    });
    return;
  }

  await streamText(chatId, 'I understand. How can I help you move forward?');
}

let pendingReplays = [];

function scheduleReplay(chatId, prompt) {
  pendingReplays.push({ chatId, prompt });
}

export function runReplays() {
  const replays = pendingReplays;
  pendingReplays = [];
  for (const { chatId, prompt } of replays) {
    simulateHarness(chatId, prompt);
  }
}

function resumePendingChats() {
  const pending = state.pendingMessages;
  state.pendingMessages = [];
  for (const { chatId, prompt } of pending) {
    scheduleReplay(chatId, prompt);
  }
}

function storePendingMessage(chatId, prompt) {
  state.pendingMessages.push({ chatId, prompt, ts: Date.now() });
}


async function searchMcpRegistry(query) {
  // Try real npm search first
  try {
    const resp = await fetch(`https://registry.npmjs.org/-/v1/search?text=${encodeURIComponent(query + ' mcp')}&size=10`);
    if (resp.ok) {
      const data = await resp.json();
      const objects = data.objects || [];
      const results = objects
        .map((obj) => {
          const pkg = obj.package || {};
          const name = pkg.name || '';
          const desc = pkg.description || 'MCP server';
          return {
            id: name.replace(/\//g, '-').replace(/@/g, ''),
            name,
            description: desc,
            capabilities: [],
            auth_required: /api.?key|token|secret|auth|password/i.test(`${name} ${desc}`),
            source_kind: 'npm',
          };
        })
        .filter((r) => /mcp|modelcontextprotocol/i.test(r.name));
      if (results.length > 0) return results;
    }
  } catch (e) {
    console.warn('[e2e] npm search failed', e.message);
  }
  // Fallback to well-known presets
  const presets = [
    { id: 'server-github', name: '@modelcontextprotocol/server-github', description: 'Official GitHub MCP server', capabilities: ['git'], auth_required: true, source_kind: 'npm' },
    { id: 'server-everything', name: '@modelcontextprotocol/server-everything', description: 'Demo MCP server with many tools', capabilities: ['tools'], auth_required: false, source_kind: 'npm' },
    { id: 'server-filesystem', name: '@modelcontextprotocol/server-filesystem', description: 'Official filesystem MCP server', capabilities: ['filesystem'], auth_required: false, source_kind: 'npm' },
    { id: 'server-postgres', name: '@modelcontextprotocol/server-postgres', description: 'Official PostgreSQL MCP server', capabilities: ['database'], auth_required: true, source_kind: 'npm' },
    { id: 'server-puppeteer', name: '@modelcontextprotocol/server-puppeteer', description: 'Official browser MCP server', capabilities: ['browser'], auth_required: false, source_kind: 'npm' },
    { id: 'server-sequential-thinking', name: '@modelcontextprotocol/server-sequential-thinking', description: 'Sequential thinking MCP server', capabilities: ['tools'], auth_required: false, source_kind: 'npm' },
    { id: 'server-slack', name: '@modelcontextprotocol/server-slack', description: 'Official Slack MCP server', capabilities: ['messaging'], auth_required: true, source_kind: 'npm' },
  ];
  const q = query.toLowerCase();
  return presets.filter((p) => p.id.includes(q) || p.name.includes(q) || p.description.toLowerCase().includes(q));
}

function installMcpFromChat(chatId, id, name, source, sourceValue, values) {
  const secretIds = [];
  if (values.api_key) {
    const secretName = `${id}-api-key`;
    state.vaultSecrets[secretName] = values.api_key;
    secretIds.push(secretName);
  }
  const discovered = source === 'npm' && /github|slack|postgres/i.test(name)
    ? ['list_repos', 'get_issue', 'search_issues']
    : ['tools/list'];
  const mcp = {
    id,
    name,
    source,
    source_value: sourceValue,
    auth_required: secretIds.length > 0,
    discovered_tools: discovered,
    secret_ids: secretIds,
    enabled_tools: discovered,
    capabilities: ['tools'],
  };
  const idx = state.mcpServers.findIndex((m) => m.id === id);
  if (idx >= 0) state.mcpServers[idx] = mcp;
  else state.mcpServers.push(mcp);
  broadcast('harness:event', { chat_id: chatId, event: { type: 'AssistantDelta', payload: `Installed ${name} MCP server${secretIds.length ? ' and stored credentials' : ''}. It is available in Connectors. ` } });
  broadcast('harness:event', { chat_id: chatId, event: { type: 'Done' } });
}

export const handlers = {
  getState() {
    return state;
  },

  on(event, cb) {
    emitter.on(event, cb);
    return () => emitter.off(event, cb);
  },

  emit(event, payload) {
    emitter.emit(event, payload);
  },

  async invoke(cmd, args) {
    switch (cmd) {
      case 'set_llm_setting': {
        const req = args.req || args;
        state.providers[req.provider] = {
          api_key: req.api_key,
          base_url: req.base_url,
          model: req.model,
          temperature: req.temperature ?? 0.7,
        };
        resumePendingChats();
        return undefined;
      }
      case 'get_llm_setting': {
        const provider = args.provider || (args.req && args.req.provider);
        const p = state.providers[provider];
        if (!p) return null;
        return { provider, api_key: p.api_key, base_url: p.base_url, model: p.model, temperature: p.temperature };
      }
      case 'list_workers': {
        return state.workers;
      }
      case 'add_worker': {
        const req = args.req || args;
        const id = uid();
        state.workers.push({ id, name: req.name, url: req.url, paired: false });
        return { id, name: req.name, url: req.url, paired: false };
      }
      case 'pair_worker': {
        const req = args.req || args;
        const w = state.workers.find((x) => x.id === req.worker_id);
        if (w) w.paired = true;
        return true;
      }
      case 'install_worker': {
        return { platform: { os: 'linux', arch: 'x86_64', family: 'unix' }, asset_url: 'https://example.com/goblin.tar.gz', install_log: 'installed' };
      }
      case 'create_agent': {
        const req = args.req || args;
        const id = uid();
        const agent = { id, name: req.name, description: req.description, prompt: req.prompt, tools: req.tools || [] };
        state.agents.push(agent);
        return { id, name: agent.name, spec: { id: { 0: id }, name: agent.name, description: agent.description || '', prompt: agent.prompt, tools: agent.tools, triggers: [], mcp_ids: [] }, created_at: new Date().toISOString(), updated_at: new Date().toISOString() };
      }
      case 'list_agents': {
        return state.agents.map((a) => ({ id: a.id, name: a.name, spec: { id: { 0: a.id }, name: a.name, description: a.description || '', prompt: a.prompt, tools: a.tools, triggers: [], mcp_ids: [] }, created_at: new Date().toISOString(), updated_at: new Date().toISOString() }));
      }
      case 'delete_mcp_server': {
        const req = args.req || args;
        state.mcpServers = state.mcpServers.filter((m) => m.id !== req.id);
        return true;
      }
      case 'list_mcp_servers': {
        return state.mcpServers;
      }
      case 'install_mcp_server': {
        const req = args.req || args;
        const id = req.id || uid();
        const mcp = { id, name: req.name, source: req.source, source_value: req.source_value ?? null, auth_required: false, discovered_tools: [], secret_ids: req.secret_ids || [], enabled_tools: [] };
        state.mcpServers.push(mcp);
        return id;
      }
      case 'search_mcp_servers': {
        const req = args.req || args;
        return searchMcpRegistry(req.query || 'github');
      }
      case 'discover_mcp_tools': {
        const req = args.req || args;
        return (state.mcpServers.find((m) => m.id === req.id)?.discovered_tools) || [];
      }
      case 'update_mcp_server_meta': {
        const req = args.req || args;
        const mcp = state.mcpServers.find((m) => m.id === req.id);
        if (mcp) {
          mcp.secret_ids = req.secret_ids || mcp.secret_ids;
          mcp.enabled_tools = req.enabled_tools || mcp.enabled_tools;
        }
        return req.id;
      }
      case 'create_chat': {
        const title = args.title || (args.req && args.req.title) || 'New chat';
        const id = uid();
        const chat = { id, title, provider: args.provider || (args.req && args.req.provider) || null, model: args.model || (args.req && args.req.model) || null, updated_at: new Date().toISOString() };
        state.chats.push(chat);
        return id;
      }
      case 'list_chats': {
        return state.chats;
      }
      case 'chat_messages': {
        return state.messages[args.chat_id] || [];
      }
      case 'add_chat_message': {
        const chatId = args.chat_id || (args.req && args.req.chat_id);
        const role = args.role || (args.req && args.req.role);
        const content = args.content || (args.req && args.req.content);
        if (!state.messages[chatId]) state.messages[chatId] = [];
        state.messages[chatId].push({ id: uid(), role, content, created_at: new Date().toISOString() });
        return undefined;
      }
      case 'set_chat_model': {
        const req = args.req || args;
        const chat = state.chats.find((c) => c.id === req.chat_id);
        if (chat) {
          chat.provider = req.provider;
          chat.model = req.model;
        }
        return undefined;
      }
      case 'run_harness': {
        const req = args.req || args;
        const chatId = req.chat_id || req.chatId;
        const prompt = req.prompt || req.input;
        const hasProvider = Object.keys(state.providers).length > 0;
        if (!hasProvider) {
          storePendingMessage(chatId, prompt);
          return undefined;
        }
        await simulateHarness(chatId, prompt);
        return undefined;
      }
      case 'composer_submit': {
        const req = args.req || args;
        const chatId = req.chat_id || req.chatId;
        await streamText(chatId, 'Got it. I have saved the information.');
        return undefined;
      }
      case 'classify_intent': {
        const req = args.req || args;
        return classifyIntentMock(req.text);
      }
      case 'list_harness_tools': {
        return [
          { name: 'web_search', description: 'Search the web', parameters: { type: 'object', properties: { query: { type: 'string' } } } },
          { name: 'read_url', description: 'Read a URL', parameters: { type: 'object', properties: { url: { type: 'string' } } } },
          { name: 'write_file', description: 'Write a file', parameters: { type: 'object', properties: { path: { type: 'string' }, content: { type: 'string' } } } },
        ];
      }
      case 'get_cluster_identity': {
        return state.clusterIdentity || null;
      }
      case 'create_cluster': {
        const req = args.req || args;
        state.clusterIdentity = { cluster_name: req.name, ca_cert_pem: 'MOCK-CA', device_serial: 'MOCK-DEVICE' };
        return state.clusterIdentity;
      }
      case 'get_device_identity': {
        return state.deviceIdentity || null;
      }
      case 'generate_device_identity': {
        const req = args.req || args;
        const id = uid();
        const mode = (req.deployment_mode && req.deployment_mode.mode) || 'local';
        state.deviceIdentity = {
          id,
          cluster_name: req.cluster_name,
          cert_pem: 'MOCK-CERT',
          key_pem: 'MOCK-KEY',
          ca_cert_pem: 'MOCK-CA',
          role: 'Owner',
          is_owner: true,
          deployment_mode: mode,
          deployment_config: req.deployment_mode || {},
          deployment_status: { mode },
          created_at: new Date().toISOString(),
        };
        return state.deviceIdentity;
      }
      case 'list_clusters': {
        return state.deviceIdentity ? [{ id: state.deviceIdentity.id, cluster_name: state.deviceIdentity.cluster_name, role: state.deviceIdentity.role, is_owner: state.deviceIdentity.is_owner, deployment_mode: state.deviceIdentity.deployment_mode, deployment_status: state.deviceIdentity.deployment_status }] : [];
      }
      case 'set_vault_secret': {
        const req = args.req || args;
        state.vaultSecrets[req.name] = req.value;
        return undefined;
      }
      case 'unlock_vault': {
        const req = args.req || args;
        state.vaultPassphrase = req.passphrase;
        return Object.keys(state.vaultSecrets);
      }
      case 'list_vault_secrets': {
        return Object.keys(state.vaultSecrets).map((k) => ({ key: k, updated_at: new Date().toISOString() }));
      }
      case 'submit_variant': {
        const { option, message_id } = args;
        state.submissions.push({ type: 'variant', option, message_id, ts: Date.now() });
        return true;
      }
      case 'submit_form_card': {
        const { values, message_id } = args;
        state.submissions.push({ type: 'form', values, message_id, ts: Date.now() });
        return true;
      }
      case 'submit_secret_card': {
        const { values, message_id, metadata } = args;
        state.submissions.push({ type: 'secret', values, message_id, ts: Date.now() });
        for (const [key, value] of Object.entries(values)) {
          if (value) state.vaultSecrets[key] = value;
        }
        // If this secret submission carries MCP install metadata, trigger the MCP install flow.
        if (metadata && metadata.mcp && values.api_key) {
          const mcpId = metadata.mcp;
          const secretName = `${mcpId}-api-key`;
          state.vaultSecrets[secretName] = values.api_key;
          const discovered = metadata.source === 'npm' && /github|slack|postgres/i.test(metadata.name || mcpId)
            ? ['list_repos', 'get_issue', 'search_issues']
            : ['tools/list'];
          const mcp = {
            id: mcpId,
            name: metadata.name || mcpId,
            source: metadata.source || 'npm',
            source_value: metadata.source_value || `@modelcontextprotocol/server-${mcpId}`,
            auth_required: true,
            discovered_tools: discovered,
            secret_ids: [secretName],
            enabled_tools: discovered,
            capabilities: ['tools'],
          };
          const idx = state.mcpServers.findIndex((m) => m.id === mcpId);
          if (idx >= 0) state.mcpServers[idx] = mcp;
          else state.mcpServers.push(mcp);
          broadcast('harness:event', { chat_id: 'unknown', event: { type: 'AssistantDelta', payload: `Installed ${mcp.name} MCP server and authenticated it. I can now use it in conversations. ` } });
          broadcast('harness:event', { chat_id: 'unknown', event: { type: 'Done' } });
        }
        return true;
      }
      case 'submit_form_card': {
        const { values, message_id } = args;
        state.submissions.push({ type: 'form', values, message_id, ts: Date.now() });
        return true;
      }
      case 'submit_variant': {
        const { option, message_id, metadata } = args;
        state.submissions.push({ type: 'variant', option, message_id, metadata, ts: Date.now() });
        // If the variant is an MCP install confirmation, ask for credentials next.
        if (metadata && metadata.install && option === 'Install') {
          const mcpId = metadata.mcp || metadata.name;
          const mcpName = metadata.name || mcpId;
          broadcast('harness:event', {
            chat_id: 'unknown',
            event: {
              type: 'AskUser',
              question: `Install ${mcpName} MCP server`,
              metadata,
              fields: [
                { name: 'api_key', label: 'API key', type: 'password' },
                { name: 'scope', label: 'Scope', type: 'text' },
              ],
            },
          });
        }
        return true;
      }
      case 'reset_state': {
        state.providers = {};
        state.workers = [];
        state.agents = [];
        state.mcpServers = [];
        state.chats = [];
        state.messages = {};
        state.vaultSecrets = {};
        state.vaultPassphrase = undefined;
        state.deviceIdentity = undefined;
        state.clusterIdentity = undefined;
        state.pendingMessages = [];
        state.submissions = [];
        return true;
      }
      default: {
        console.warn(`[e2e mock] unhandled command: ${cmd}`, args);
        return undefined;
      }
    }
  },
};
