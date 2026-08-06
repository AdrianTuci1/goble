import { useEffect, useRef, forwardRef, useImperativeHandle, useState } from 'react';
import { useSearchParams, Link } from 'react-router-dom';
import { useChatStore, type AppChatMessage, type ActionListItem } from '../store/chatStore';
import { useMainViewStore } from '../../../store/mainViewStore';
import { useChatApi } from '../ChatWindow';
import { onChatUpdated, onAgentLog, onAgentStarted, onAgentFinished, onHarnessEvent, runAgent } from '../../../../../shared';
import { uid } from '../../../../../shared';
import { agentsData, type Agent } from '../../../data/agentsData';
import { flowsData, type FlowInfo } from '../../../data/flowsData';
import './Chat.css';

export interface ChatHandle {
  stop: () => void;
}

const Chat = forwardRef<ChatHandle>(function Chat(_props, ref) {
  const [params] = useSearchParams();
  const agentId = params.get('agent');
  const flowId = params.get('flow');
  const chatApi = useChatApi();

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesRef = useRef<HTMLDivElement>(null);

  const {
    activeConversationId,
    conversations,
    messagesByChat,
    setActiveConversationId,
    setMessages,
    addMessage,
    updateMessage,
    updateMessageMeta,
    setTyping,
    setActiveTrace,
    setTransientChatId,
    clearTransientChat,
  } = useChatStore();
  const messages = activeConversationId ? messagesByChat[activeConversationId] || [] : [];
  const activeConversation = conversations.find((c) => c.id === activeConversationId);

  const { selectAgent, selectFlow, openRight, addExecution } = useMainViewStore();

  const [stateSteps, setStateSteps] = useState<{ id: string; mode: string; label: string; time: string }[]>([]);
  const [history, setHistory] = useState<{ time: string; label: string; mode: string; response: string }[]>([]);

  // Scroll to bottom when messages change.
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  // Auto-select first conversation if none selected.
  useEffect(() => {
    if (!activeConversationId && conversations.length > 0) {
      setActiveConversationId(conversations[0].id);
    }
  }, [activeConversationId, conversations, setActiveConversationId]);

  // New-chat global event.
  useEffect(() => {
    function onNewChat() {
      handleNewChat();
    }
    window.addEventListener('goble:new-chat', onNewChat);
    return () => window.removeEventListener('goble:new-chat', onNewChat);
  }, []);

  // Discard an empty/transient chat when leaving the chat view.
  useEffect(() => {
    return () => {
      useChatStore.getState().clearTransientChat();
    };
  }, []);

  // Start from agent/flow query params.
  useEffect(() => {
    if (agentId) {
      const agent = agentsData.find((a: Agent) => a.id === agentId);
      if (agent) startAgentChat(agent.id, agent.name);
    } else if (flowId) {
      startFlowChat(flowId);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, flowId]);

  // Tauri listeners.
  useEffect(() => {
    const unlistenPromises: Promise<(() => void) | null>[] = [];
    async function setup() {
      const promises = [
        onChatUpdated((event) => {
          const payload = event.payload as { chat_id?: string; message?: AppChatMessage };
          if (payload.chat_id && payload.message) addMessage(payload.chat_id, payload.message);
        }),
        onAgentLog((event) => {
          const payload = event.payload as { trace_id?: string; message?: string };
          if (payload.trace_id && payload.message && activeConversationId) {
            updateMessage(activeConversationId, payload.trace_id, (prev) => prev + payload.message);
          }
        }),
        onAgentStarted((event) => {
          const payload = event.payload as { trace_id?: string; agent_id?: string };
          if (payload.trace_id && activeConversationId) {
            setActiveTrace(payload.trace_id);
            addMessage(activeConversationId, {
              id: payload.trace_id,
              role: 'assistant',
              content: '',
              created_at: new Date().toISOString(),
              streaming: true,
            });
            setTyping(true);
            setRenderModeAndNotify('running');
            openRight('history');
          }
        }),
        onAgentFinished((event) => {
          const payload = event.payload as { trace_id?: string; status?: string };
          if (payload.trace_id && activeConversationId) {
            setTyping(false);
            updateMessageMeta(activeConversationId, payload.trace_id, { streaming: false });
            updateMessage(activeConversationId, payload.trace_id, (prev) => prev || payload.status || 'Done');
            setRenderModeAndNotify(null);
          }
        }),
        onHarnessEvent((event) => {
          const payload = event.payload as { chat_id?: string; event?: Record<string, unknown> } | undefined;
          if (!activeConversationId || !payload) return;
          if (payload.chat_id && payload.chat_id !== activeConversationId && payload.chat_id !== 'unknown') return;
          const chatId = activeConversationId;
          const ev = payload.event;
          if (!ev) return;
          switch (ev.type) {
            case 'AssistantDelta': {
              const text = (ev.payload as string | undefined) || '';
              const done = !text;
              addMessageChunk(chatId, text, done);
              break;
            }
            case 'ToolCallStarted': {
              const tool = (ev.name as string) || 'tool';
              const args = (ev.arguments || {}) as Record<string, unknown>;
              addToolCallInternal(chatId, tool, args);
              break;
            }
            case 'AskUser': {
              const question = (ev.question as string) || 'Please provide details';
              const quickReplies = (ev.quick_replies as string[] | undefined) || [];
              const fields = (ev.fields as AppChatMessage['fields'] | undefined) || [];
              const askMetadata = (ev.metadata as Record<string, unknown> | undefined) || {};
              let kind: AppChatMessage['kind'] = 'formCard';
              if (quickReplies.length > 0 && fields.length === 0) kind = 'variantCard';
              else if (fields.some((f) => f.type === 'password' || f.name.includes('secret') || f.name.includes('key') || f.name.includes('token'))) kind = 'secretCard';
              addMessage(chatId, {
                id: uid(),
                role: 'assistant',
                content: question,
                created_at: new Date().toISOString(),
                kind,
                title: question,
                options: quickReplies,
                fields,
                metadata: askMetadata,
              });
              break;
            }
            case 'Done':
              setRenderModeAndNotify(null);
              setTyping(false);
              setActiveTrace(null);
              break;
            case 'Error': {
              const msg = (ev.message as string) || 'Error';
              addMessageInternal(chatId, `Error: ${msg}`, 'system');
              setRenderModeAndNotify(null);
              setTyping(false);
              setActiveTrace(null);
              break;
            }
          }
        }),
      ];
      for (const promise of promises) {
        unlistenPromises.push(
          promise.catch((err) => {
            console.error('Failed to register listener', err);
            return null;
          }),
        );
      }
    }
    setup();
    return () => {
      unlistenPromises.forEach(async (p) => (await p)?.());
    };
  }, [activeConversationId]);

  // Expose imperative API to ChatWindow.
  useImperativeHandle(ref, () => ({
    stop: () => stop(),
  }));

  function setRenderModeAndNotify(mode: string | null) {
    chatApi?.setRenderMode(mode);
    if (mode) {
      const label = renderModeLabel(mode);
      const time = new Date().toLocaleTimeString();
      setStateSteps((prev) => [...prev, { id: uid(), mode, label, time }]);
      setHistory((prev) => [
        ...prev,
        { time, label, mode, response: renderModeResponse(mode) },
      ]);
    } else {
      archiveExecution();
    }
  }

  function archiveExecution() {
    if (history.length === 0) return;
    const title = activeConversation?.title || 'New chat';
    addExecution(title, [...stateSteps]);
    setHistory([]);
    setStateSteps([]);
  }

  async function handleNewChat() {
    archiveExecution();
    clearTransientChat();
    const chatId = uid();
    setActiveConversationId(chatId);
    setTransientChatId(chatId);
    setMessages(chatId, []);
    setRenderModeAndNotify(null);
    chatApi?.setComposerMode('default');
  }

  async function startAgentChat(agentId: string, title: string) {
    if (!activeConversationId) await handleNewChat();
    const chatId = activeConversationId || uid();
    selectAgent(agentId);
    openRight('info');
    addMessage(chatId, { id: uid(), role: 'system', content: `Started ${title} agent.`, created_at: new Date().toISOString() });
    try {
      await runAgent('local', chatId, agentId, 'start');
    } catch {
      setTimeout(() => simulateReply(chatId, agentId), 300);
    }
  }

  async function startFlowChat(flowId: string) {
    if (!activeConversationId) await handleNewChat();
    const chatId = activeConversationId || uid();
    selectFlow(flowId);
    openRight('info');
    simulateReply(chatId, flowId);
  }

  function simulateReply(chatId: string, id: string) {
    const agent = agentsData.find((a: Agent) => a.id === id);
    if (agent) {
      addMessage(chatId, { id: uid(), role: 'assistant', content: agent.description, created_at: new Date().toISOString() });
    }
    const flow = flowsData.find((f: FlowInfo) => f.id === id);
    if (flow) {
      addMessage(chatId, {
        id: uid(),
        role: 'assistant',
        content: `Flow: **${flow.title}**\nCreated by ${flow.meta.createdBy}\nIntegrations: ${flow.meta.integrations.join(', ')}\nSchedule: ${flow.meta.cron}`,
        created_at: new Date().toISOString(),
      });
    }
  }

  // Internal helpers for chat content.
  function addMessageInternal(chatId: string, content: string, role: 'user' | 'assistant' | 'system' = 'assistant', kind: AppChatMessage['kind'] = 'text') {
    addMessage(chatId, { id: uid(), role, content, created_at: new Date().toISOString(), kind });
  }

  function addMessageChunk(chatId: string, text: string, done: boolean) {
    const store = useChatStore.getState();
    const traceId = store.activeTrace;
    if (!traceId) {
      if (text) addMessageInternal(chatId, text, 'assistant', 'text');
      return;
    }
    const msgs = store.messagesByChat[chatId] || [];
    const exists = msgs.find((m) => m.id === traceId);
    if (!exists) {
      addMessage(chatId, { id: traceId, role: 'assistant', content: text, created_at: new Date().toISOString(), streaming: !done });
    } else {
      updateMessage(chatId, traceId, (prev) => prev + text);
    }
    if (done) {
      updateMessageMeta(chatId, traceId, { streaming: false });
      setActiveTrace(null);
    }
  }

  function addToolCallInternal(chatId: string, tool: string, args: Record<string, unknown>) {
    addMessage(chatId, {
      id: uid(),
      role: 'assistant',
      content: '',
      created_at: new Date().toISOString(),
      kind: 'toolCall',
      tool,
      args,
    });
  }

  function stop() {
    setRenderModeAndNotify(null);
    addMessageInternal(activeConversationId || uid(), 'Stopped.', 'system');
  }

  function renderModeLabel(mode: string) {
    const labels: Record<string, string> = {
      thinking: 'Thinking about the request',
      searching: 'Searching through files',
      generating: 'Generating a response',
      running: 'Running commands',
      syncing: 'Syncing changes',
      connecting: 'Connecting to environment',
      'analyzing-image': 'Analyzing the image',
      planning: 'Planning the steps',
    };
    return labels[mode] || `Running: ${mode}`;
  }

  function renderModeResponse(mode: string) {
    return `Agent state: ${renderModeLabel(mode)}.\n\nThis is the full execution context captured at this timestamp for observability. In a production integration this would contain the complete model response, tool outputs, and decision trace.`;
  }

  if (!activeConversationId) {
    return null;
  }

  return (
    <div className="chat-view">
      <div ref={messagesRef} className="chat-messages">
        {messages.length === 0 && <div className="chat-empty"><p>Send a message to start the conversation.</p></div>}
        {messages.map((m) => (
          <MessageBubble key={m.id} message={m} />
        ))}
        <div ref={messagesEndRef} />
      </div>
    </div>
  );

  function MessageBubble({ message }: { message: AppChatMessage }) {
    const isUser = message.role === 'user';
    const isSystem = message.role === 'system';
    if (message.kind === 'configureLink') {
      return <ConfigureLinkMessage message={message} />;
    }
    if (isUser) {
      return (
        <div className="message user">
          <div className="message-content"><RichText text={message.content} /></div>
        </div>
      );
    }
    if (isSystem) {
      return (
        <div className="message system">
          <div className="message-content"><em>{message.content}</em></div>
        </div>
      );
    }

    switch (message.kind) {
      case 'codeBlock':
        return <CodeBlockMessage message={message} />;
      case 'codeChangeCard':
        return <CodeChangeCard message={message} />;
      case 'toolCall':
        return <ToolCallMessage message={message} />;
      case 'actionList':
        return <ActionListMessage message={message} />;
      case 'variantCard':
        return <VariantCardMessage message={message} />;
      case 'secretCard':
        return <SecretCardMessage message={message} />;
      case 'formCard':
        return <FormCardMessage message={message} />;
      default:
        return (
          <div className="message assistant">
            {message.streaming && <span className="streaming-indicator" />}
            <div className="message-content"><RichText text={message.content} /></div>
          </div>
        );
    }
  }
});

function CodeBlockMessage({ message }: { message: AppChatMessage }) {
  const [expanded, setExpanded] = useState(false);
  const code = message.code || '';
  return (
    <div className={`code-block-wrapper ${expanded ? 'expanded' : ''}`}>
      <pre className="code-block" data-lang={message.language || ''}>{code}</pre>
      <button className="code-block-toggle" onClick={() => setExpanded(!expanded)}>
        {expanded ? 'Show less' : 'Show more'}
      </button>
    </div>
  );
}

function CodeChangeCard({ message }: { message: AppChatMessage }) {
  const [activeIndex, setActiveIndex] = useState(0);
  const [expanded, setExpanded] = useState(false);
  const files = message.files || [];
  return (
    <div className={`code-change-card composer-card ${expanded ? '' : 'collapsed'}`}>
      <div className="code-change-topbar composer-topbar" onClick={() => setExpanded(!expanded)}>
        <div className="code-change-title">
          <span className="code-change-icon">✓</span>
          <h4>{message.description}</h4>
        </div>
        <div className="composer-topbar-actions">
          <button className="btn secondary code-change-toggle">{expanded ? '▲' : '▼'}</button>
        </div>
      </div>
      <div className="code-change-body" style={{ display: expanded ? 'block' : 'none' }}>
        <div className="code-change-tabs">
          {files.map((file, i) => (
            <button key={file.path} className={`code-change-tab ${i === activeIndex ? 'active' : ''}`} onClick={() => setActiveIndex(i)}>
              {file.path}
            </button>
          ))}
        </div>
        <div className="code-change-content">
          {files.map((file, i) => (
            <pre key={file.path} className={`code-change-pre ${i === activeIndex ? 'active' : ''}`} data-mode={file.mode}>
              {file.content}
            </pre>
          ))}
        </div>
      </div>
    </div>
  );
}

function ToolCallMessage({ message }: { message: AppChatMessage }) {
  const summary = toolSummary(message.tool || '', message.args);
  return <div className="tool-call">{summary}</div>;
}

function VariantCardMessage({ message }: { message: AppChatMessage }) {
  const options = message.options || [];
  async function choose(option: string) {
    try {
      await (window as any).__goble_e2e_invoke__?.('submit_variant', { option, message_id: message.id });
    } catch {
      // ignore in production
    }
  }
  return (
    <div className="composer-card variant-card" data-testid="variant-card">
      <div className="composer-topbar">
        <h4>{message.title || 'Choose an option'}</h4>
      </div>
      <div className="variant-options">
        {options.map((option) => (
          <button key={option} className="variant-option" data-option={option} onClick={() => choose(option)}>
            {option}
          </button>
        ))}
      </div>
    </div>
  );
}

function submitCard(cmd: string, message: AppChatMessage, formData: Record<string, string>) {
  try {
    (window as any).__goble_e2e_invoke__?.(cmd, { message_id: message.id, values: formData, metadata: message.metadata });
  } catch {
    // ignore in production
  }
}

function FormCardMessage({ message }: { message: AppChatMessage }) {
  const fields = message.fields || [];
  function onSubmit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const data: Record<string, string> = {};
    const form = e.currentTarget;
    fields.forEach((field) => {
      const input = form.elements.namedItem(field.name) as HTMLInputElement | null;
      if (input) data[field.name] = input.value;
    });
    submitCard('submit_form_card', message, data);
  }
  return (
    <div className="composer-card form-card" data-testid="form-card">
      <div className="composer-topbar">
        <h4>{message.title || 'Provide details'}</h4>
      </div>
      <form className="form-card-body" onSubmit={onSubmit}>
        {fields.map((field) => (
          <div key={field.name} className="form-card-field">
            <label>{field.label}</label>
            <input name={field.name} type={field.type || 'text'} data-field={field.name} />
          </div>
        ))}
        <button className="form-card-submit" type="submit" data-testid="form-card-submit">
          Submit
        </button>
      </form>
    </div>
  );
}

function SecretCardMessage({ message }: { message: AppChatMessage }) {
  const fields = message.fields || [];
  function onSubmit(e: React.FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const data: Record<string, string> = {};
    const form = e.currentTarget;
    fields.forEach((field) => {
      const input = form.elements.namedItem(field.name) as HTMLInputElement | null;
      if (input) data[field.name] = input.value;
    });
    submitCard('submit_secret_card', message, data);
  }
  return (
    <div className="composer-card secret-card" data-testid="secret-card">
      <div className="composer-topbar">
        <h4>{message.title || 'Authorize secret'}</h4>
      </div>
      <form className="form-card-body" onSubmit={onSubmit}>
        {fields.map((field) => (
          <div key={field.name} className="form-card-field">
            <label>{field.label}</label>
            <input name={field.name} type={field.type || 'text'} data-field={field.name} />
          </div>
        ))}
        <button className="form-card-submit" type="submit" data-testid="secret-card-submit">
          Save secret
        </button>
      </form>
    </div>
  );
}

function ActionListMessage({ message }: { message: AppChatMessage }) {
  const items = message.items || [];
  return (
    <div className="action-list">
      {items.map((item, index) => (
        <ActionItem key={item.id || index} messageId={message.id} item={item} index={index} />
      ))}
    </div>
  );
}

function ActionItem({ messageId, item, index }: { messageId: string; item: ActionListItem; index: number }) {
  function markDone() {
    const store = useChatStore.getState();
    const chatId = store.activeConversationId || '';
    const msgs = store.messagesByChat[chatId] || [];
    const msg = msgs.find((m) => m.id === messageId);
    if (!msg || !msg.items) return;
    const nextItems = msg.items.map((it, i) => (i === index ? { ...it, status: 'done' as const, statusText: 'Done' } : it));
    store.updateMessageMeta(chatId, messageId, { items: nextItems });
  }
  return (
    <div
      className={`action-item status-${item.status}${item.status === 'pending' ? ' active' : ''}`}
      onClick={item.status === 'pending' ? markDone : undefined}
      style={{ cursor: item.status === 'pending' ? 'pointer' : 'default' }}
    >
      <div className="action-header">
        <span className="action-icon" />
        <span className="action-label">{item.label}</span>
        <span className="action-status-text">{item.statusText || item.status}</span>
      </div>
      <div className="action-body" />
    </div>
  );
}

function RichText({ text }: { text: string }) {
  if (!text) return null;
  return <div className="rich-text" dangerouslySetInnerHTML={{ __html: simpleHtml(text) }} />;
}

function ConfigureLinkMessage({ message }: { message: AppChatMessage }) {
  const text = message.content;
  const parts = text.split('click here');
  return (
    <div className="message system configure-link-message">
      <div className="message-content">
        {parts[0]}
        <Link className="configure-link" to="/settings/providers">
          click here
        </Link>
        {parts[1]}
      </div>
    </div>
  );
}

function simpleHtml(md: string) {
  return md
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/\n/g, '<br />');
}

function toolSummary(tool: string, args: Record<string, unknown> | undefined) {
  const a = args || {};
  let arg = '';
  if (tool === 'run_shell') arg = (a.command as string) || '';
  else if (tool === 'read_file') arg = (a.path as string) || '';
  else if (tool === 'read_files') arg = Array.isArray(a.paths) ? (a.paths as string[]).join(', ') : '';
  else if (tool === 'browse' || tool === 'read_pages' || tool === 'fetch') arg = Array.isArray(a.urls) ? (a.urls as string[]).join(', ') : (a.url as string) || '';
  else if (tool === 'deploy') arg = [a.env && `env=${a.env}`, a.region && `region=${a.region}`].filter(Boolean).join(' ');
  return arg ? `${tool}: ${arg}` : tool;
}

export default Chat;
