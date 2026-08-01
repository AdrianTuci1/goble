import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Bot,
  Sparkles,
  Zap,
  Eye,
  Monitor,
  BrainCircuit,
  Terminal,
  Send,
  MoreHorizontal,
  Paperclip,
  Image as ImageIcon,
  Smile,
  Tag,
} from 'lucide-react';
import './ChatArea.css';
import { useStore, type ChatMessage, type WorkerInfo } from '../stores/appStore';
import {
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

interface ChatAreaProps {
  threadsActive?: boolean;
}

export default function ChatArea({ threadsActive }: ChatAreaProps) {
  const navigate = useNavigate();
  void threadsActive; // reserved for future layout variant
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
  const setRightSidebarOpen = useStore((s) => s.setRightSidebarOpen);
  const setRightSidebarTab = useStore((s) => s.setRightSidebarTab);
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
      navigate('/settings', { state: { tab: 'appearance' } });
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
      case 'create_workflow': {
        if (!params.name || !params.expression) {
          addMessage(chatId, {
            id: `${Date.now()}-need-more`,
            role: 'assistant',
            content: 'What should I name the workflow and how often should it run?',
            created_at: new Date().toISOString(),
          });
          return;
        }
        addStepMessage(addMessage, chatId, 'ran_command', `Creating workflow ${params.name}`);
        const wf = await createWorkflow(params.name, params.message || '', [], params.expression);
        const cmdStep = findLastStep(messages, 'ran_command');
        if (cmdStep) updateStepMessage(updateMessage, chatId, cmdStep.id, { status: 'done' });
        addMessage(chatId, {
          id: `${Date.now()}-wf`,
          role: 'assistant',
          content: `Created workflow **${wf.name}** (${wf.id}).`,
          created_at: new Date().toISOString(),
        });
        break;
      }
      case 'schedule_agent': {
        if (!params.agent || !params.message || paired.length === 0) {
          addMessage(chatId, {
            id: `${Date.now()}-need-more`,
            role: 'assistant',
            content: paired.length === 0
              ? 'No paired worker available. Pair a worker first in Settings.'
              : 'Which agent and what message should I schedule?',
            created_at: new Date().toISOString(),
          });
          return;
        }
        addStepMessage(addMessage, chatId, 'ran_command', `Scheduling ${params.agent}`);
        await scheduleAgent(paired[0].id, params.agent, params.expression || '0 0 * * *');
        const cmdStep = findLastStep(messages, 'ran_command');
        if (cmdStep) updateStepMessage(updateMessage, chatId, cmdStep.id, { status: 'done' });
        addMessage(chatId, {
          id: `${Date.now()}-sched`,
          role: 'assistant',
          content: `Scheduled **${params.agent}** as job.`,
          created_at: new Date().toISOString(),
        });
        break;
      }
      case 'run_command': {
        addStepMessage(addMessage, chatId, 'ran_command', params.message || sentInput);
        await runHarness(chatId, params.message || sentInput, `${activeProvider}/${activeModel}`);
        break;
      }
      default: {
        if (paired.length === 0) {
          addMessage(chatId, {
            id: `${Date.now()}-no-worker`,
            role: 'assistant',
            content: 'No paired worker available. Add a worker in Settings to execute commands.',
            created_at: new Date().toISOString(),
          });
          return;
        }
        addStepMessage(addMessage, chatId, 'connecting', 'Connecting to computer');
        await runHarness(chatId, sentInput, `${activeProvider}/${activeModel}`);
      }
    }
  }

  const sentInput = '';

  const pairedWorkers = workers.filter((w) => w.paired);
  const activeAgents = agents;

  function openInfo() {
    setRightSidebarTab('info');
    setRightSidebarOpen(true);
  }

  return (
    <div className="chat-area">
      <div className="chat-window">
        <div className="chat-header">
          <div className="chat-title-row">
            <span className="chat-title">
              {activeConversation?.title || 'New chat'}
            </span>
            <span className="chat-subtitle">
              {activeProvider}/{activeModel}
            </span>
          </div>
          <div className="chat-header-actions">
            {!modelConfigured && (
              <button
                className="chat-header-warning"
                onClick={() => navigate('/settings', { state: { tab: 'appearance' } })}
              >
                <Sparkles size={14} />
                Configure LLM
              </button>
            )}
            <button className="chat-header-btn" onClick={openInfo} title="Info">
              <MoreHorizontal size={16} />
            </button>
          </div>
        </div>

        <div className="chat-messages" ref={scrollRef}>
          {!activeChatId && (
            <div className="chat-empty">
              <div className="welcome-title">How can I help you today?</div>
              <div className="welcome-hint">Start a new chat from the sidebar or type a message below.</div>
            </div>
          )}
          {activeChatId && messages.map((m) => (
            <Message key={m.id} message={m} expanded={expandedSteps.has(m.id)} onToggle={() => {
              setExpandedSteps((prev) => {
                const next = new Set(prev);
                if (next.has(m.id)) next.delete(m.id);
                else next.add(m.id);
                return next;
              });
            }} />
          ))}
        </div>

        <div className="chat-composer">
          <div className="composer-toolbar">
            <div className="toolbar-left">
              <select
                className="composer-control"
                value={provider}
                onChange={(e) => setProvider(e.target.value)}
              >
                {LLM_PROVIDERS.map((p) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
              </select>
              <input
                className="composer-control composer-model"
                value={model}
                onChange={(e) => setModel(e.target.value)}
                placeholder="Model"
              />
              <select
                className="composer-control"
                value={workerId}
                onChange={(e) => setWorkerId(e.target.value)}
              >
                <option value="">Worker</option>
                {pairedWorkers.map((w) => (
                  <option key={w.id} value={w.id}>{w.name}</option>
                ))}
              </select>
              <select
                className="composer-control"
                value={agentId}
                onChange={(e) => setAgentId(e.target.value)}
              >
                <option value="">Agent</option>
                {activeAgents.map((a) => (
                  <option key={a.id} value={a.id}>{a.name}</option>
                ))}
              </select>
            </div>
          </div>

          <div className="composer-input-row">
            <button className="composer-attach">
              <Paperclip size={18} />
            </button>
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  handleSend();
                }
              }}
              placeholder="Message..."
              rows={1}
            />
            <button
              className="composer-send"
              onClick={handleSend}
              disabled={!input.trim() || isRunning}
            >
              <Send size={18} />
            </button>
          </div>

          <div className="composer-toolbar">
            <div className="toolbar-left">
              <button className="toolbar-btn" title="Tag">
                <Tag size={16} />
              </button>
              <button className="toolbar-btn" title="Image">
                <ImageIcon size={16} />
              </button>
              <button className="toolbar-btn" title="Emoji">
                <Smile size={16} />
              </button>
            </div>
            <div className="toolbar-right">
              <span className="composer-hint">Shift+Enter for new line</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function Message({ message, expanded, onToggle }: { message: ChatMessage; expanded: boolean; onToggle: () => void }) {
  if (message.role === 'step') {
    const step = tryParseStep(message.content);
    if (!step) return null;
    const Icon = STEP_ICONS[step.type] || Bot;
    const statusClass = step.status === 'done' ? 'chat-step-done' : step.status === 'error' ? 'chat-step-error' : 'chat-step-pending';
    return (
      <div className={`chat-message step`}>
        <button className={`chat-step ${statusClass}`} onClick={onToggle}>
          <div className="chat-step-header">
            <Icon size={16} />
            <span className="chat-step-title">{step.title}</span>
            {step.status === 'done' ? <Sparkles size={14} /> : step.status === 'error' ? <Zap size={14} /> : <Terminal size={14} />}
          </div>
          {expanded && step.details && (
            <div className="chat-step-details">{step.details}</div>
          )}
        </button>
      </div>
    );
  }

  if (message.role === 'tool') {
    const tool = tryParseTool(message.content);
    return (
      <div className="chat-message tool">
        <div className="message-role">Tool call</div>
        <div className="tool-call">
          <div className="tool-call-header">{tool?.name || 'Tool'}</div>
          {tool?.arguments && (
            <pre className="tool-call-args">{JSON.stringify(tool.arguments, null, 2)}</pre>
          )}
          {tool?.status === 'finished' && <div className="tool-result">{tool.result}</div>}
          {tool?.status === 'error' && <div className="tool-error">{tool.message}</div>}
        </div>
      </div>
    );
  }

  return (
    <div className={`chat-message ${message.role}`}>
      <div className="message-role">{message.role === 'user' ? 'You' : 'Assistant'}</div>
      <div className="message-content">{message.content}</div>
    </div>
  );
}

function parseCommand(text: string): { type: string; args: string } | null {
  const match = text.match(/^\/([a-zA-Z0-9_-]+)(?:\s+(.*))?$/);
  if (!match) return null;
  return { type: `/${match[1]}`, args: match[2] || '' };
}

async function handleCommand(chatId: string, type: string, args: string, paired: WorkerInfo[]) {
  // Reserved for slash commands (kept for future use)
  void chatId;
  void type;
  void args;
  void paired;
}

