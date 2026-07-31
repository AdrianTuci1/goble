import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Bot,
  Cpu,
  HardHat,
  Sparkles,
  Settings,
  ChevronDown,
  ChevronRight,
  Zap,
  Eye,
  Monitor,
  BrainCircuit,
  Terminal,
} from 'lucide-react';
import './ChatArea.css';
import { useStore, type ChatMessage, type WorkerInfo } from '../stores/appStore';
import {
  cancelHarness,
  createChat,
  chatMessages,
  addChatMessage,
  runAgent,
  runHarness,
  setChatModel,
  onHarnessEvent,
  LLM_PROVIDERS,
  getLlmSetting,
  classifyIntent,
  createAgent,
  installMcpServer,
  scheduleAgent,
  createWorkflow,
  searchMcpServers,
} from '../tauri/api';
import type { HarnessEventPayload } from '../tauri/api';

const EMPTY_MESSAGES: ChatMessage[] = [];

interface ToolCallPayload {
  id: string;
  name?: string;
  arguments?: Record<string, unknown>;
  status?: 'finished' | 'error';
  result?: string;
  message?: string;
}

interface StepPayload {
  type: 'thinking' | 'connecting' | 'ran_command' | 'viewed' | 'using_agent' | 'done' | 'error';
  title: string;
  status?: 'pending' | 'done' | 'error';
  details?: string;
  expanded?: boolean;
}

interface IntentParams {
  name?: string;
  prompt?: string;
  tools?: string[];
  source?: string;
  value?: string;
  query?: string;
  agent?: string;
  expression?: string;
  agents?: string[];
  message?: string;
}

interface ClassifiedIntent {
  intent: string;
  params: IntentParams;
}

const STEP_ICONS: Record<string, React.ElementType> = {
  thinking: BrainCircuit,
  connecting: Monitor,
  ran_command: Terminal,
  viewed: Eye,
  using_agent: Zap,
  done: Bot,
  error: Bot,
};

function tryParseTool(content: string): ToolCallPayload | undefined {
  try {
    const parsed = JSON.parse(content);
    if (parsed && typeof parsed === 'object' && 'id' in parsed) {
      return parsed as ToolCallPayload;
    }
  } catch {
    // not a tool payload
  }
  return undefined;
}

function tryParseStep(content: string): StepPayload | undefined {
  try {
    const parsed = JSON.parse(content);
    if (parsed && typeof parsed === 'object' && 'type' in parsed) {
      return parsed as StepPayload;
    }
  } catch {
    // not a step payload
  }
  return undefined;
}

function addStepMessage(
  addMessage: (chatId: string, message: ChatMessage) => void,
  chatId: string,
  type: StepPayload['type'],
  title: string,
  status?: StepPayload['status'],
  details?: string,
) {
  addMessage(chatId, {
    id: `${Date.now()}-${type}-${Math.random().toString(36).slice(2, 7)}`,
    role: 'step',
    content: JSON.stringify({ type, title, status, details, expanded: false }),
    created_at: new Date().toISOString(),
  });
}

function updateStepMessage(
  updateMessage: (chatId: string, messageId: string, content: string | ((prev: string) => string)) => void,
  chatId: string,
  messageId: string,
  updates: Partial<StepPayload>,
) {
  updateMessage(chatId, messageId, (prev) => {
    const step = tryParseStep(prev) || { type: 'thinking', title: prev, status: 'pending' };
    return JSON.stringify({ ...step, ...updates });
  });
}

function findLastStep(messages: ChatMessage[], type: StepPayload['type']): ChatMessage | undefined {
  return [...messages].reverse().find((m) => {
    if (m.role !== 'step') return false;
    const step = tryParseStep(m.content);
    return step?.type === type;
  });
}

export default function ChatArea() {
  const navigate = useNavigate();
  const activeChatId = useStore((s) => s.activeConversationId);
  const setActiveChatId = useStore((s) => s.setActiveConversation);
  const conversations = useStore((s) => s.conversations);
  const addConversation = useStore((s) => s.addConversation);
  const updateConversation = useStore((s) => s.updateConversation);
  const messages = useStore((s) =>
    activeChatId ? s.messages[activeChatId] || EMPTY_MESSAGES : EMPTY_MESSAGES,
  );
  const setMessages = useStore((s) => s.setMessages);
  const addLog = useStore((s) => s.addLog);
  const addMessage = useStore((s) => s.addMessage);
  const updateMessage = useStore((s) => s.updateMessage);
  const [input, setInput] = useState('');
  const [isRunning, setIsRunning] = useState(false);
  const [workerId, setWorkerId] = useState('');
  const [agentId, setAgentId] = useState('');
  const [provider, setProvider] = useState('openai');
  const [model, setModel] = useState('gpt-4o-mini');
  const [modelConfigured, setModelConfigured] = useState(true);
  const [expandedSteps, setExpandedSteps] = useState<Set<string>>(new Set());
  const workers = useStore((s) => s.workers);
  const agents = useStore((s) => s.agents);
  const scrollRef = useRef<HTMLDivElement>(null);
  const pendingIds = useRef<Set<string>>(new Set());
  const seeded = useRef(false);

  const activeConversation = activeChatId
    ? conversations.find((c) => c.id === activeChatId)
    : undefined;
  const activeProvider = activeConversation?.provider || provider;
  const activeModel = activeConversation?.model || model;

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  useEffect(() => {
    if (activeChatId) {
      chatMessages(activeChatId).then((msgs) => setMessages(activeChatId, msgs));
    }
  }, [activeChatId, setMessages]);

  useEffect(() => {
    async function checkModel() {
      const setting = await getLlmSetting(provider);
      setModelConfigured(Boolean(setting && (setting.api_key || provider === 'ollama') && setting.model));
    }
    checkModel();
  }, [provider]);

  useEffect(() => {
    let unsub: (() => void) | undefined;
    onHarnessEvent((e) => {
      const { chat_id, event } = e.payload as HarnessEventPayload;
      if (chat_id !== activeChatId) return;
      switch (event.type) {
        case 'AssistantDelta': {
          const text = String(event.payload ?? '');
          const key = `streaming-${chat_id}`;
          updateMessage(chat_id, key, (prev) => prev + text);
          break;
        }
        case 'ToolCallStarted': {
          const tool = event as unknown as ToolCallPayload;
          const id = `tool-${tool.id}`;
          addMessage(chat_id, {
            id,
            role: 'tool',
            content: JSON.stringify({
              id: tool.id,
              name: tool.name,
              arguments: tool.arguments,
            }),
            created_at: new Date().toISOString(),
          });
          pendingIds.current.add(id);
          break;
        }
        case 'ToolCallFinished': {
          const tool = event as unknown as ToolCallPayload;
          const id = `tool-${tool.id}`;
          updateMessage(
            chat_id,
            id,
            JSON.stringify({
              id: tool.id,
              name: tool.name,
              status: 'finished',
              result: tool.result,
            }),
          );
          break;
        }
        case 'ToolCallError': {
          const tool = event as unknown as ToolCallPayload;
          const id = `tool-${tool.id}`;
          updateMessage(
            chat_id,
            id,
            JSON.stringify({
              id: tool.id,
              name: tool.name,
              status: 'error',
              message: tool.message,
            }),
          );
          break;
        }
        case 'Done': {
          for (const id of pendingIds.current) {
            pendingIds.current.delete(id);
          }
          chatMessages(chat_id).then((msgs) => setMessages(chat_id, msgs));
          break;
        }
      }
    }).then((u) => (unsub = u));
    return () => unsub?.();
  }, [activeChatId, addMessage, updateMessage, setMessages]);

  useEffect(() => {
    if (seeded.current || conversations.length > 0) return;
    seeded.current = true;
    seedMockConversations();
  }, [conversations.length]);

  async function seedMockConversations() {
    const now = new Date().toISOString();
    const chatId1 = await createChat('Create a coding agent', 'openai', 'gpt-4o-mini');
    addConversation({ id: chatId1, title: 'Create a coding agent', provider: 'openai', model: 'gpt-4o-mini', updated_at: now });
    addMessage(chatId1, {
      id: 'mock-u-1',
      role: 'user',
      content: 'Create a coding agent that reviews pull requests and leaves helpful comments.',
      created_at: now,
    });
    addMessage(chatId1, {
      id: 'mock-s-1',
      role: 'step',
      content: JSON.stringify({ type: 'thinking', title: 'Thinking', status: 'done' }),
      created_at: now,
    });
    addMessage(chatId1, {
      id: 'mock-s-2',
      role: 'step',
      content: JSON.stringify({ type: 'ran_command', title: 'Created agent PR Reviewer', status: 'done' }),
      created_at: now,
    });
    addMessage(chatId1, {
      id: 'mock-a-1',
      role: 'assistant',
      content: 'I created the agent **PR Reviewer**. It will analyze diffs and leave comments on style, logic, and tests.',
      created_at: now,
    });

    const chatId2 = await createChat('Run PR Reviewer', 'openai', 'gpt-4o-mini');
    addConversation({ id: chatId2, title: 'Run PR Reviewer', provider: 'openai', model: 'gpt-4o-mini', updated_at: now });
    addMessage(chatId2, {
      id: 'mock-u-2',
      role: 'user',
      content: 'Run PR Reviewer on the latest PR in the repo.',
      created_at: now,
    });
    addMessage(chatId2, {
      id: 'mock-s-3',
      role: 'step',
      content: JSON.stringify({ type: 'thinking', title: 'Thinking', status: 'done' }),
      created_at: now,
    });
    addMessage(chatId2, {
      id: 'mock-s-4',
      role: 'step',
      content: JSON.stringify({ type: 'connecting', title: 'Connecting to computer', status: 'done' }),
      created_at: now,
    });
    addMessage(chatId2, {
      id: 'mock-s-5',
      role: 'step',
      content: JSON.stringify({ type: 'using_agent', title: 'Using agent PR Reviewer', status: 'done' }),
      created_at: now,
    });
    addMessage(chatId2, {
      id: 'mock-a-2',
      role: 'assistant',
      content: 'Dispatched PR Reviewer on the paired worker. You can follow its execution in the Agents tab.',
      created_at: now,
    });

    setActiveChatId(chatId1);
  }

  async function ensureChat() {
    let chatId = activeChatId;
    const usedProvider = activeProvider;
    const usedModel = activeModel;
    if (!chatId) {
      chatId = await createChat('New chat', usedProvider, usedModel);
      addConversation({
        id: chatId,
        title: 'New chat',
        provider: usedProvider,
        model: usedModel,
        updated_at: new Date().toISOString(),
      });
      setActiveChatId(chatId);
    } else {
      const chat = conversations.find((c) => c.id === chatId);
      if (!chat?.provider || !chat?.model) {
        await setChatModel(chatId, usedProvider, usedModel);
        updateConversation(chatId, { provider: usedProvider, model: usedModel });
      }
    }
    return { chatId, usedProvider, usedModel };
  }

  async function handleSend() {
    if (!input.trim()) return;
    if (!modelConfigured) {
      navigate('/settings', { state: { tab: 'llm' } });
      return;
    }
    const { chatId } = await ensureChat();
    await addChatMessage(chatId, 'user', input);
    addMessage(chatId, {
      id: `${Date.now()}`,
      role: 'user',
      content: input,
      created_at: new Date().toISOString(),
    });
    const sentInput = input;
    setInput('');
    addLog(`user sent message in chat ${chatId}`);
    await handleUserTurn(chatId, sentInput);
  }

  async function handleUserTurn(chatId: string, sentInput: string) {
    setIsRunning(true);
    const paired = workers.filter((w) => w.paired);

    if (sentInput.startsWith('/')) {
      const parsed = parseCommand(sentInput);
      if (parsed && parsed.type !== '/') {
        await handleCommand(chatId, parsed.type.slice(1), parsed.args, paired);
      } else {
        await runHarness(chatId, sentInput, `${activeProvider}/${activeModel}`);
      }
      setIsRunning(false);
      return;
    }

    if (workerId && agentId) {
      addStepMessage(addMessage, chatId, 'connecting', 'Connecting to computer');
      addStepMessage(addMessage, chatId, 'using_agent', `Using agent ${agents.find((a) => a.id === agentId)?.name || agentId}`);
      await runAgent(workerId, chatId, agentId, sentInput);
      addStepMessage(addMessage, chatId, 'done', 'Done');
      setIsRunning(false);
      return;
    }

    addStepMessage(addMessage, chatId, 'thinking', 'Thinking');
    try {
      const classified = await classifyIntent(activeProvider, activeModel, sentInput);
      await executeIntent(chatId, classified, paired);
    } catch (e) {
      const step = findLastStep(messages, 'thinking');
      if (step) {
        updateStepMessage(updateMessage, chatId, step.id, { status: 'error', details: String(e) });
      }
      addMessage(chatId, {
        id: `${Date.now()}-err`,
        role: 'assistant',
        content: `I could not understand that: ${String(e)}`,
        created_at: new Date().toISOString(),
      });
    }
    setIsRunning(false);
  }

  async function executeIntent(chatId: string, classified: ClassifiedIntent, paired: WorkerInfo[]) {
    const step = findLastStep(messages, 'thinking');
    if (step) {
      updateStepMessage(updateMessage, chatId, step.id, { status: 'done' });
    }
    const { intent, params } = classified;

    switch (intent) {
      case 'create_agent': {
        if (!params.name || !params.prompt) {
          addMessage(chatId, {
            id: `${Date.now()}-need-more`,
            role: 'assistant',
            content: 'What should I name the agent and what should it do?',
            created_at: new Date().toISOString(),
          });
          return;
        }
        addStepMessage(addMessage, chatId, 'ran_command', `Creating agent ${params.name}`);
        const agent = await createAgent(params.name, params.prompt, params.message || '', params.tools || []);
        const cmdStep = findLastStep(messages, 'ran_command');
        if (cmdStep) updateStepMessage(updateMessage, chatId, cmdStep.id, { status: 'done' });
        addMessage(chatId, {
          id: `${Date.now()}-agent`,
          role: 'assistant',
          content: `Created agent **${agent.name}** (${agent.id}).`,
          created_at: new Date().toISOString(),
        });
        break;
      }
      case 'install_mcp': {
        if (!params.source || !params.value) {
          addMessage(chatId, {
            id: `${Date.now()}-need-more`,
            role: 'assistant',
            content: 'What MCP server should I install? (e.g. npm, github, local)',
            created_at: new Date().toISOString(),
          });
          return;
        }
        addStepMessage(addMessage, chatId, 'ran_command', `Installing MCP server from ${params.source}`);
        const id = await installMcpServer(crypto.randomUUID(), `${params.source} ${params.value}`, params.source, params.value);
        const cmdStep = findLastStep(messages, 'ran_command');
        if (cmdStep) updateStepMessage(updateMessage, chatId, cmdStep.id, { status: 'done' });
        addMessage(chatId, {
          id: `${Date.now()}-mcp`,
          role: 'assistant',
          content: `Installed MCP server ${id}.`,
          created_at: new Date().toISOString(),
        });
        break;
      }
      case 'search_mcp': {
        if (!params.query) {
          addMessage(chatId, {
            id: `${Date.now()}-need-more`,
            role: 'assistant',
            content: 'What kind of MCP server are you looking for?',
            created_at: new Date().toISOString(),
          });
          return;
        }
        addStepMessage(addMessage, chatId, 'viewed', `Searching MCP registry for "${params.query}"`);
        const results = await searchMcpServers(params.query);
        const cmdStep = findLastStep(messages, 'viewed');
        if (cmdStep) updateStepMessage(updateMessage, chatId, cmdStep.id, { status: 'done' });
        addMessage(chatId, {
          id: `${Date.now()}-mcp`,
          role: 'assistant',
          content: results.length
            ? `Found MCP servers:\n${results.map((r) => `- ${r.name} (${r.source})`).join('\n')}`
            : 'No MCP servers found.',
          created_at: new Date().toISOString(),
        });
        break;
      }
      case 'schedule_agent': {
        if (paired.length === 0) {
          showNeedWorker(chatId);
          return;
        }
        if (!params.agent || !params.expression) {
          addMessage(chatId, {
            id: `${Date.now()}-need-more`,
            role: 'assistant',
            content: 'Which agent and how often should it run?',
            created_at: new Date().toISOString(),
          });
          return;
        }
        const found = resolveAgent(params.agent);
        if (!found) {
          addMessage(chatId, {
            id: `${Date.now()}-unknown`,
            role: 'assistant',
            content: `I do not know an agent named "${params.agent}".`,
            created_at: new Date().toISOString(),
          });
          return;
        }
        addStepMessage(addMessage, chatId, 'connecting', `Connecting to ${paired[0].name}`);
        addStepMessage(addMessage, chatId, 'ran_command', `Scheduling ${found.name}`);
        await scheduleAgent(paired[0].id, found.id, params.expression);
        const cmdStep = findLastStep(messages, 'ran_command');
        if (cmdStep) updateStepMessage(updateMessage, chatId, cmdStep.id, { status: 'done' });
        addMessage(chatId, {
          id: `${Date.now()}-cron`,
          role: 'assistant',
          content: `Scheduled **${found.name}** on ${paired[0].name} with cron \`${params.expression}\`.`,
          created_at: new Date().toISOString(),
        });
        break;
      }
      case 'create_workflow': {
        if (paired.length === 0) {
          showNeedWorker(chatId);
          return;
        }
        if (!params.name || !params.expression || !params.agents?.length) {
          addMessage(chatId, {
            id: `${Date.now()}-need-more`,
            role: 'assistant',
            content: 'What is the workflow name, schedule, and which agents should run?',
            created_at: new Date().toISOString(),
          });
          return;
        }
        const steps = params.agents.map((a) => {
          const found = resolveAgent(a);
          return {
            id: crypto.randomUUID(),
            name: a,
            agent_id: { 0: found ? found.id : a },
            input_template: `Run ${a}`,
            depends_on: [],
          };
        });
        addStepMessage(addMessage, chatId, 'ran_command', `Creating workflow ${params.name}`);
        const workflow = await createWorkflow(params.name, params.message || '', steps, params.expression);
        const cmdStep = findLastStep(messages, 'ran_command');
        if (cmdStep) updateStepMessage(updateMessage, chatId, cmdStep.id, { status: 'done' });
        addMessage(chatId, {
          id: `${Date.now()}-workflow`,
          role: 'assistant',
          content: `Created workflow **${workflow.name}** with ${steps.length} step(s).`,
          created_at: new Date().toISOString(),
        });
        break;
      }
      case 'run_agent': {
        if (paired.length === 0) {
          showNeedWorker(chatId);
          return;
        }
        if (!params.agent) {
          addMessage(chatId, {
            id: `${Date.now()}-need-more`,
            role: 'assistant',
            content: 'Which agent should I run?',
            created_at: new Date().toISOString(),
          });
          return;
        }
        const found = resolveAgent(params.agent);
        if (!found) {
          addMessage(chatId, {
            id: `${Date.now()}-unknown`,
            role: 'assistant',
            content: `I do not know an agent named "${params.agent}".`,
            created_at: new Date().toISOString(),
          });
          return;
        }
        addStepMessage(addMessage, chatId, 'connecting', 'Connecting to computer');
        addStepMessage(addMessage, chatId, 'using_agent', `Using agent ${found.name}`);
        await runAgent(paired[0].id, chatId, found.id, params.prompt || params.message || '');
        const usingStep = findLastStep(messages, 'using_agent');
        if (usingStep) updateStepMessage(updateMessage, chatId, usingStep.id, { status: 'done' });
        addMessage(chatId, {
          id: `${Date.now()}-run`,
          role: 'assistant',
          content: `Dispatched **${found.name}** on ${paired[0].name}.`,
          created_at: new Date().toISOString(),
        });
        break;
      }
      case 'chat':
      default: {
        addStepMessage(addMessage, chatId, 'thinking', 'Thinking');
        await runHarness(chatId, classified.params.message || sentInputPlaceholder, `${activeProvider}/${activeModel}`);
        break;
      }
    }
  }

  function resolveAgent(nameOrId: string) {
    return agents.find((a) => a.id === nameOrId || a.name.toLowerCase() === nameOrId.toLowerCase());
  }

  function showNeedWorker(chatId: string) {
    addMessage(chatId, {
      id: `${Date.now()}-needs-worker`,
      role: 'assistant',
      content: 'No paired worker available. Add and pair a worker in Settings to run agents or schedule workflows.',
      created_at: new Date().toISOString(),
    });
  }

  function parseCommand(text: string): { type: string; args: string } | null {
    if (!text.startsWith('/')) return null;
    const space = text.indexOf(' ');
    if (space === -1) return { type: text, args: '' };
    return { type: text.slice(0, space), args: text.slice(space + 1).trim() };
  }

  async function handleCommand(chatId: string, cmd: string, args: string, paired: WorkerInfo[]) {
    if (['agent', 'mcp', 'cron', 'workflow', 'run'].includes(cmd) && paired.length === 0) {
      showNeedWorker(chatId);
      return;
    }

    try {
      if (cmd === 'agent') {
        const match = args.match(/^create\s+([^:]+):\s*(.+)$/s);
        if (!match) {
          addMessage(chatId, {
            id: `${Date.now()}-agent-help`,
            role: 'assistant',
            content: 'Usage: /agent create <name>: <prompt>',
            created_at: new Date().toISOString(),
          });
          return;
        }
        const [, name, prompt] = match;
        await executeIntent(chatId, { intent: 'create_agent', params: { name, prompt } }, paired);
      } else if (cmd === 'mcp') {
        const parts = args.split(' ').filter(Boolean);
        if (parts[0] === 'install' && parts.length >= 3) {
          const [, source, ...valueParts] = parts;
          await executeIntent(chatId, { intent: 'install_mcp', params: { source, value: valueParts.join(' ') } }, paired);
        } else if (parts[0] === 'search' && parts[1]) {
          await executeIntent(chatId, { intent: 'search_mcp', params: { query: parts.slice(1).join(' ') } }, paired);
        } else {
          addMessage(chatId, {
            id: `${Date.now()}-mcp-help`,
            role: 'assistant',
            content: 'Usage: /mcp install <source> <value> | /mcp search <query>',
            created_at: new Date().toISOString(),
          });
        }
      } else if (cmd === 'cron') {
        const parts = args.split(' ').filter(Boolean);
        if (parts.length < 3 || parts[0] !== 'add') {
          addMessage(chatId, {
            id: `${Date.now()}-cron-help`,
            role: 'assistant',
            content: 'Usage: /cron add <agent-id> "<expression>"',
            created_at: new Date().toISOString(),
          });
          return;
        }
        await executeIntent(chatId, { intent: 'schedule_agent', params: { agent: parts[1], expression: parts.slice(2).join(' ').replace(/^"|"$/g, '') } }, paired);
      } else if (cmd === 'workflow') {
        const parts = args.split(' ').filter(Boolean);
        if (parts[0] !== 'add' || parts.length < 4) {
          addMessage(chatId, {
            id: `${Date.now()}-workflow-help`,
            role: 'assistant',
            content: 'Usage: /workflow add <name> "<expression>" <agent>...',
            created_at: new Date().toISOString(),
          });
          return;
        }
        const [, name, expression, ...agentNames] = parts;
        await executeIntent(chatId, { intent: 'create_workflow', params: { name, expression, agents: agentNames } }, paired);
      } else if (cmd === 'run') {
        const parts = args.split(' ').filter(Boolean);
        if (parts.length < 2) {
          addMessage(chatId, {
            id: `${Date.now()}-run-help`,
            role: 'assistant',
            content: 'Usage: /run <agent-id> <prompt>',
            created_at: new Date().toISOString(),
          });
          return;
        }
        const target = parts[0];
        const prompt = parts.slice(1).join(' ');
        await executeIntent(chatId, { intent: 'run_agent', params: { agent: target, prompt } }, paired);
      } else {
        await runHarness(chatId, `/${cmd} ${args}`, `${activeProvider}/${activeModel}`);
      }
    } catch (e) {
      addMessage(chatId, {
        id: `${Date.now()}-cmd-err`,
        role: 'assistant',
        content: `Command failed: ${String(e)}`,
        created_at: new Date().toISOString(),
      });
    }
  }

  const sentInputPlaceholder = '';

  function startNewChat() {
    createChat('New chat', provider, model).then((id) => {
      addConversation({
        id,
        title: 'New chat',
        provider,
        model,
        updated_at: new Date().toISOString(),
      });
      setActiveChatId(id);
    });
  }

  function onProviderChange(p: string) {
    setProvider(p);
    const defaultModel = LLM_PROVIDERS.find((x) => x.id === p)?.defaultModel ?? '';
    setModel(defaultModel);
    if (activeChatId) {
      setChatModel(activeChatId, p, defaultModel).then(() =>
        updateConversation(activeChatId, { provider: p, model: defaultModel }),
      );
    }
  }

  function onModelChange(m: string) {
    setModel(m);
    if (activeChatId) {
      setChatModel(activeChatId, provider, m).then(() =>
        updateConversation(activeChatId, { model: m }),
      );
    }
  }

  function toggleStep(id: string) {
    setExpandedSteps((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
    const step = messages.find((m) => m.id === id);
    if (step) {
      updateStepMessage(updateMessage, activeChatId || '', id, { expanded: !expandedSteps.has(id) });
    }
  }

  function renderMessageContent(m: ChatMessage) {
    const { content, role } = m;
    if (role === 'tool') {
      const tool = tryParseTool(content);
      if (tool) {
        return (
          <div className="tool-call">
            <div className="tool-call-header">tool: {tool.name || tool.id}</div>
            {tool.arguments && (
              <pre className="tool-call-args">
                {String(JSON.stringify(tool.arguments, null, 2))}
              </pre>
            )}
            {tool.status === 'finished' && <div className="tool-result">✅ {tool.result}</div>}
            {tool.status === 'error' && <div className="tool-error">❌ {tool.message}</div>}
          </div>
        );
      }
    }
    if (role === 'step') {
      const step = tryParseStep(content);
      if (step) {
        const Icon = STEP_ICONS[step.type] || Bot;
        const expanded = expandedSteps.has(m.id) || step.expanded || false;
        return (
          <div className={`chat-step chat-step-${step.status || 'pending'}`}>
            <button className="chat-step-header" onClick={() => toggleStep(m.id)}>
              <Icon size={14} />
              <span className="chat-step-title">{step.title}</span>
              {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            </button>
            {expanded && step.details && (
              <div className="chat-step-details">{step.details}</div>
            )}
          </div>
        );
      }
    }
    return <div className="message-content">{content}</div>;
  }

  return (
    <div className="chat-area">
      <div className="chat-header">
        <div className="chat-title">
          {activeChatId
            ? conversations.find((c) => c.id === activeChatId)?.title || 'Chat'
            : 'New chat'}
        </div>
        <div className="chat-header-actions">
          {!modelConfigured && (
            <button
              className="chat-header-warning"
              onClick={() => navigate('/settings', { state: { tab: 'llm' } })}
              title="No LLM provider configured"
            >
              <Settings size={14} /> Configure model
            </button>
          )}
          {workers.filter((w) => w.paired).length === 0 && (
            <button
              className="chat-header-warning"
              onClick={() => navigate('/settings', { state: { tab: 'workers' } })}
              title="No paired worker"
            >
              <HardHat size={14} /> Add worker
            </button>
          )}
          <button onClick={startNewChat}>New chat</button>
        </div>
      </div>
      <div className="chat-messages" ref={scrollRef}>
        {messages.length === 0 && (
          <div className="chat-empty">Start a conversation with an agent or add a worker.</div>
        )}
        {messages.map((m) => (
          <div key={m.id} className={`chat-message ${m.role}`}>
            <div className="message-role">{m.role}</div>
            {renderMessageContent(m)}
          </div>
        ))}
      </div>
      <div className="chat-composer">
        <div className="composer-toolbar">
          <label className="composer-control" title="Provider">
            <Sparkles size={14} />
            <select value={provider} onChange={(e) => onProviderChange(e.target.value)}>
              {LLM_PROVIDERS.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </label>
          <label className="composer-control" title="Model">
            <Cpu size={14} />
            <input
              type="text"
              value={model}
              onChange={(e) => onModelChange(e.target.value)}
              placeholder="model"
            />
          </label>
          <label className="composer-control" title="Worker">
            <HardHat size={14} />
            <select value={workerId} onChange={(e) => setWorkerId(e.target.value)}>
              <option value="">worker</option>
              {workers.filter((w) => w.paired).map((w) => (
                <option key={w.id} value={w.id}>
                  {w.name}
                </option>
              ))}
            </select>
          </label>
          <label className="composer-control" title="Agent">
            <Bot size={14} />
            <select value={agentId} onChange={(e) => setAgentId(e.target.value)}>
              <option value="">agent</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="composer-input-row">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                handleSend();
              }
            }}
            placeholder="Type a message or /command..."
          />
          <button className="composer-send" onClick={handleSend}>Send</button>
          {isRunning && (
            <button
              className="composer-cancel"
              onClick={() => activeChatId && cancelHarness(activeChatId)}
            >
              Cancel
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
