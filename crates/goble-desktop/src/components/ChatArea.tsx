import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import './ChatArea.css';
import { useStore, type ChatMessage } from '../stores/appStore';
import {
  createChat,
  runAgent,
  addChatMessage,
  onChatUpdated,
  onAgentLog,
  onAgentStarted,
  onAgentFinished,
} from '../tauri/api';
import { uid, hslHash, getInitials } from '../utils/designSystem';
import { flowsData, type FlowInfo } from '../mocks/flowsData';
import { agentsData, type Agent } from '../mocks/agentsData';

export default function ChatArea() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const agentId = params.get('agent');
  const flowId = params.get('flow');

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [input, setInput] = useState('');
  const [typing, setTyping] = useState(false);
  const [renderMode, setRenderMode] = useState<{ mode: string; label: string } | null>(null);
  const [selectedVariant, setSelectedVariant] = useState<{ question: string; choices: string[]; handler: (choice: string) => void } | null>(null);
  const [confirmation, setConfirmation] = useState<{ id: string; title: string; message: string; actions: string[]; handler: (action: string) => void } | null>(null);
  const [activeTrace, setActiveTrace] = useState<string | null>(null);
  void activeTrace;

  const activeConversationId = useStore((s) => s.activeConversationId);
  const conversations = useStore((s) => s.conversations);
  const allMessages = useStore((s) => s.messages);
  const messages = allMessages[activeConversationId || ''] || [];
  const setActiveConversation = useStore((s) => s.setActiveConversation);
  const addConversation = useStore((s) => s.addConversation);
  const setMessages = useStore((s) => s.setMessages);
  const addMessage = useStore((s) => s.addMessage);
  const updateMessage = useStore((s) => s.updateMessage);
  const setRightSidebarOpen = useStore((s) => s.setRightSidebarOpen);
  const setRightSidebarTab = useStore((s) => s.setRightSidebarTab);
  const setHistoryDetailId = useStore((s) => s.setHistoryDetailId);
  const setSelectedFlowId = useStore((s) => s.setSelectedFlowId);

  const activeConversation = conversations.find((c) => c.id === activeConversationId);

  useEffect(() => {
    function onNewChat() {
      handleNewChat();
    }
    window.addEventListener('goble:new-chat', onNewChat);
    return () => window.removeEventListener('goble:new-chat', onNewChat);
  }, []);

  useEffect(() => {
    if (!activeConversationId) {
      const first = conversations[0];
      if (first) setActiveConversation(first.id);
    }
  }, [activeConversationId, conversations]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    if (agentId) {
      const agent = agentsData.find((a: Agent) => a.id === agentId);
      if (agent) startAgentChat(agent.id, agent.name);
      navigate('/chat', { replace: true });
    } else if (flowId) {
      startFlowChat(flowId);
      navigate('/chat', { replace: true });
    }
  }, [agentId, flowId]);

  useEffect(() => {
    const unlistenPromises: Promise<(() => void) | null>[] = [];

    async function setupListeners() {
      const promises = [
        onChatUpdated((event) => {
          const payload = event.payload as { chat_id?: string; message?: ChatMessage };
          if (payload.chat_id && payload.message) {
            addMessage(payload.chat_id, payload.message);
          }
        }),
        onAgentLog((event) => {
          const payload = event.payload as { trace_id?: string; message?: string };
          if (payload.trace_id && payload.message) {
            updateMessage(activeConversationId || '', payload.trace_id, (prev) => prev + payload.message);
          }
        }),
        onAgentStarted((event) => {
          const payload = event.payload as { trace_id?: string; agent_id?: string };
          if (payload.trace_id && payload.agent_id) {
            setActiveTrace(payload.trace_id);
            addMessage(activeConversationId || '', {
              id: payload.trace_id,
              role: 'assistant',
              content: '',
              created_at: new Date().toISOString(),
            });
            setTyping(true);
            setRightSidebarOpen(true);
            setRightSidebarTab('history');
            setHistoryDetailId(payload.trace_id);
          }
        }),
        onAgentFinished((event) => {
          const payload = event.payload as { trace_id?: string; status?: string };
          if (payload.trace_id) {
            setTyping(false);
            updateMessage(activeConversationId || '', payload.trace_id, (prev) =>
              prev ? prev : payload.status || 'Done'
            );
          }
        }),
      ];
      for (const promise of promises) {
        unlistenPromises.push(
          promise.catch((err) => {
            console.error('Failed to register listener', err);
            return null;
          })
        );
      }
    }

    setupListeners();

    return () => {
      unlistenPromises.forEach(async (promise) => {
        const unlisten = await promise;
        unlisten?.();
      });
    };
  }, [activeConversationId]);

  async function handleNewChat() {
    try {
      const chatId = await createChat('New chat', 'openai', 'gpt-4o-mini');
      addConversation({ id: chatId, title: 'New chat', provider: 'openai', model: 'gpt-4o-mini', updated_at: new Date().toISOString() });
      setActiveConversation(chatId);
      setMessages(chatId, []);
      inputRef.current?.focus();
    } catch {
      const chatId = uid();
      addConversation({ id: chatId, title: 'New chat', provider: 'openai', model: 'gpt-4o-mini', updated_at: new Date().toISOString() });
      setActiveConversation(chatId);
      setMessages(chatId, []);
      inputRef.current?.focus();
    }
  }

  async function startAgentChat(agentId: string, title: string) {
    if (!activeConversationId) await handleNewChat();
    const chatId = activeConversationId || uid();
    if (!activeConversation) {
      addConversation({ id: chatId, title, provider: 'openai', model: 'gpt-4o-mini', updated_at: new Date().toISOString() });
      setActiveConversation(chatId);
    }
    addMessage(chatId, {
      id: uid(),
      role: 'system',
      content: `Started ${title} agent.`,
      created_at: new Date().toISOString(),
    });
    try {
      await runAgent('local', chatId, agentId, 'start');
    } catch {
      setTimeout(() => simulateFlow(chatId, agentId), 300);
    }
  }

  async function startFlowChat(flowId: string) {
    if (!activeConversationId) await handleNewChat();
    const chatId = activeConversationId || uid();
    const flow = flowsData.find((f: FlowInfo) => f.id === flowId);
    if (!activeConversation) {
      addConversation({ id: chatId, title: flow?.title || flowId, provider: 'openai', model: 'gpt-4o-mini', updated_at: new Date().toISOString() });
      setActiveConversation(chatId);
    }
    setSelectedFlowId(flowId);
    setRightSidebarOpen(true);
    setRightSidebarTab('info');
    simulateFlow(chatId, flowId);
  }

  async function simulateFlow(chatId: string, flowId: string) {
    const agent = agentsData.find((a: Agent) => a.id === flowId);
    if (agent) {
      addMessage(chatId, {
        id: uid(),
        role: 'assistant',
        content: agent.description,
        created_at: new Date().toISOString(),
      });
    }
    const flow = flowsData.find((f: FlowInfo) => f.id === flowId);
    if (flow) {
      addMessage(chatId, {
        id: uid(),
        role: 'assistant',
        content: `Flow: **${flow.title}**\nCreated by ${flow.meta.createdBy}\nIntegrations: ${flow.meta.integrations.join(', ')}\nSchedule: ${flow.meta.cron}`,
        created_at: new Date().toISOString(),
      });
    }
  }

  async function handleSend() {
    if (!input.trim() || !activeConversationId) return;
    const text = input.trim();
    setInput('');
    addMessage(activeConversationId, {
      id: uid(),
      role: 'user',
      content: text,
      created_at: new Date().toISOString(),
    });
    try {
      await addChatMessage(activeConversationId, 'user', text);
    } catch {
      // fallback: demo mode, generate a local reply
    }
    setTyping(true);
    setTimeout(() => {
      setTyping(false);
      addMessage(activeConversationId, {
        id: uid(),
        role: 'assistant',
        content: 'Received: ' + text,
        created_at: new Date().toISOString(),
      });
    }, 600);
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function handleCancel() {
    setTyping(false);
    setRenderMode(null);
    setActiveTrace(null);
  }

  function handleVariantChoice(choice: string) {
    if (!selectedVariant) return;
    setSelectedVariant(null);
    addMessage(activeConversationId || '', {
      id: uid(),
      role: 'user',
      content: choice,
      created_at: new Date().toISOString(),
    });
    selectedVariant.handler(choice);
  }

  function handleConfirm(action: string) {
    if (!confirmation) return;
    setConfirmation(null);
    addMessage(activeConversationId || '', {
      id: uid(),
      role: 'user',
      content: action,
      created_at: new Date().toISOString(),
    });
    confirmation.handler(action);
  }

  if (!activeConversationId) {
    return (
      <div className="chat-view empty">
        <div className="chat-welcome">
          <div className="chat-welcome-logo">G</div>
          <h2>Welcome to Goble</h2>
          <p>Choose an agent from the sidebar or start a new chat.</p>
          <button className="btn" onClick={handleNewChat}>Start new chat</button>
        </div>
      </div>
    );
  }

  return (
    <div className="chat-view">
      <div className="chat-header">
        <div className="chat-header-info">
          <span className="chat-header-title">{activeConversation?.title || 'Chat'}</span>
          <span className="chat-header-meta">{activeConversation?.model || 'gpt-4o-mini'}</span>
        </div>
        <div className="chat-header-actions">
          <button className="chat-header-btn" onClick={() => { setRightSidebarOpen(true); setRightSidebarTab('info'); }} title="Info">ℹ️</button>
          <button className="chat-header-btn" onClick={() => { setRightSidebarOpen(true); setRightSidebarTab('history'); }} title="History">📜</button>
        </div>
      </div>

      <div className="chat-messages">
        {messages.length === 0 && (
          <div className="chat-empty">
            <p>Send a message to start the conversation.</p>
          </div>
        )}
        {messages.map((m) => (
          <MessageBubble key={m.id} message={m} />
        ))}
        {typing && (
          <div className="message assistant">
            <div className="message-avatar" style={{ background: '#9ca3af' }}>AI</div>
            <div className="message-content">
              <div className="typing-indicator">
                <span /><span /><span />
              </div>
            </div>
          </div>
        )}
        {renderMode && (
          <div className="render-mode">
            <span className="render-dot" />
            <span className="render-label">{renderMode.label}</span>
          </div>
        )}
        {selectedVariant && (
          <div className="variant-card">
            <div className="variant-question">{selectedVariant.question}</div>
            <div className="variant-choices">
              {selectedVariant.choices.map((choice) => (
                <button key={choice} className="variant-choice" onClick={() => handleVariantChoice(choice)}>
                  {choice}
                </button>
              ))}
            </div>
          </div>
        )}
        {confirmation && (
          <div className="confirmation-card">
            <div className="confirmation-title">{confirmation.title}</div>
            <div className="confirmation-message">{confirmation.message}</div>
            <div className="confirmation-actions">
              {confirmation.actions.map((action) => (
                <button key={action} className={`btn ${action === 'Cancel' ? 'secondary' : ''}`} onClick={() => handleConfirm(action)}>
                  {action}
                </button>
              ))}
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      <div className="chat-composer">
        <div className="composer-row">
          <input
            ref={inputRef}
            className="composer-input"
            placeholder="Message..."
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
          />
          <button className="composer-send" onClick={handleSend} disabled={!input.trim() || typing}>
            ↑
          </button>
        </div>
        <div className="composer-toolbar">
          <div className="composer-toolbar-left">
            <button title="Mention">@</button>
            <button title="Attach">📎</button>
            <button title="Emoji">☺</button>
            <button title="Tag">#</button>
            <button title="Format">Aa</button>
          </div>
          {typing && <button className="composer-cancel" onClick={handleCancel}>Cancel</button>}
        </div>
      </div>
    </div>
  );
}

function MessageBubble({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';
  const author = isUser ? 'You' : isSystem ? 'System' : 'Assistant';
  const color = isUser ? '#22c55e' : isSystem ? '#6b7280' : '#9ca3af';
  const initials = getInitials(author);

  return (
    <div className={`message ${isUser ? 'user' : isSystem ? 'system' : 'assistant'}`}>
      <div className="message-avatar" style={{ background: color }} title={author}>
        {initials}
      </div>
      <div className="message-body">
        <div className="message-meta">
          <span className="message-author">{author}</span>
        </div>
        <div className="message-content">
          <RichText text={message.content} />
        </div>
      </div>
    </div>
  );
}

function RichText({ text }: { text: string }) {
  if (!text) return null;
  if (text.startsWith('```') || text.includes('`')) {
    return <pre className="code-block">{text}</pre>;
  }
  return <div className="rich-text" dangerouslySetInnerHTML={{ __html: simpleHtml(text) }} />;
}

function simpleHtml(md: string) {
  return md
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/\u003e/g, '&gt;')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/\n/g, '<br />');
}

export { uid, hslHash, getInitials };
