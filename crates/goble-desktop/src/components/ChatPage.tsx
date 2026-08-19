import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Activity } from 'lucide-react';
import { useStore } from '../stores/appStore';
import {
  listChats,
  createChat,
  chatMessages,
  onChatUpdated,
  onHarnessEvent,
  setLlmSetting,
  LLM_PROVIDERS,
  type HarnessEventPayload,
  type ChatMessage,
} from '../tauri/api';
import ChatComposer from './ChatComposer';
import './ChatPage.css';

const EMPTY_MESSAGES: ChatMessage[] = [];

export default function ChatPage() {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const createdRef = useRef(false);
  const streamingIdsRef = useRef<Record<string, string>>({});
  const [running, setRunning] = useState(false);

  const conversations = useStore((s) => s.conversations);
  const activeConversationId = useStore((s) => s.activeConversationId);
  const messagesMap = useStore((s) => s.messages);
  const setConversations = useStore((s) => s.setConversations);
  const setActiveConversation = useStore((s) => s.setActiveConversation);
  const setMessages = useStore((s) => s.setMessages);
  const addMessage = useStore((s) => s.addMessage);
  const updateMessage = useStore((s) => s.updateMessage);
  const removeMessage = useStore((s) => s.removeMessage);
  const updateConversation = useStore((s) => s.updateConversation);
  const rightSidebarOpen = useStore((s) => s.rightSidebarOpen);
  const rightSidebarTab = useStore((s) => s.rightSidebarTab);
  const setRightSidebarOpen = useStore((s) => s.setRightSidebarOpen);
  const setRightSidebarTab = useStore((s) => s.setRightSidebarTab);

  const messages = activeConversationId
    ? messagesMap[activeConversationId] || EMPTY_MESSAGES
    : EMPTY_MESSAGES;
  const activeConversation = conversations.find((c) => c.id === activeConversationId);
  const navigate = useNavigate();

  function toggleObservability() {
    if (rightSidebarOpen && rightSidebarTab === 'history') {
      setRightSidebarOpen(false);
    } else {
      setRightSidebarTab('history');
      setRightSidebarOpen(true);
    }
  }

  function parseCardContent(content: string): { kind: string; meta: Record<string, string> } | null {
    if (!content.startsWith('__CARD__')) return null;
    try {
      const parsed = JSON.parse(content.slice('__CARD__'.length));
      const { kind, ...meta } = parsed;
      if (typeof kind !== 'string') return null;
      return { kind, meta: meta as Record<string, string> };
    } catch {
      return null;
    }
  }

  useEffect(() => {
    listChats()
      .then((chats) => {
        setConversations(chats);
        if (chats.length > 0) {
          const current = chats.find((c) => c.id === activeConversationId);
          if (!current) {
            setActiveConversation(chats[0].id);
          }
        } else if (!createdRef.current) {
          createdRef.current = true;
          createChat('New chat', 'openai', 'gpt-4o-mini').then((id) => {
            const chat = {
              id,
              title: 'New chat',
              provider: 'openai',
              model: 'gpt-4o-mini',
              updated_at: new Date().toISOString(),
            };
            setConversations([chat]);
            setActiveConversation(id);
          });
        }
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!activeConversationId) return;
    chatMessages(activeConversationId)
      .then((msgs) => setMessages(activeConversationId, msgs))
      .catch(() => {});
  }, [activeConversationId, setMessages]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    const unsubs: (() => void)[] = [];
    (async () => {
      unsubs.push(
        await onChatUpdated((event) => {
          const payload = event.payload as { chat_id?: string };
          if (payload.chat_id) {
            chatMessages(payload.chat_id)
              .then((msgs) => setMessages(payload.chat_id!, msgs))
              .catch(() => {});
          }
        }),
      );
      unsubs.push(
        await onHarnessEvent((event) => {
          const { chat_id, event: ev } = event.payload as HarnessEventPayload;
          if (!chat_id) return;
          if (ev.type === 'AssistantDelta') {
            const delta = ((ev.payload as { delta?: string }) || {}).delta || '';
            const streamId = streamingIdsRef.current[chat_id];
            if (streamId) {
              updateMessage(chat_id, streamId, (prev) => prev + delta);
            } else {
              const id = `${chat_id}-stream-${Date.now()}`;
              streamingIdsRef.current[chat_id] = id;
              addMessage(chat_id, {
                id,
                role: 'assistant',
                content: delta,
                created_at: new Date().toISOString(),
              });
            }
          } else if (ev.type === 'Done' || ev.type === 'Error') {
            delete streamingIdsRef.current[chat_id];
            setRunning(false);
          }
        }),
      );
    })();
    return () => unsubs.forEach((u) => u());
  }, [setMessages, addMessage, updateMessage]);

  function ApiKeyCard({ messageId, provider }: { messageId: string; provider: string }) {
    const [value, setValue] = useState('');
    const [saving, setSaving] = useState(false);
    const providerInfo = LLM_PROVIDERS.find((p) => p.id === provider);
    const defaultModel = providerInfo?.defaultModel || '';

    async function save() {
      if (!value.trim() || !activeConversationId) return;
      setSaving(true);
      try {
        await setLlmSetting(provider, value.trim(), defaultModel);
        updateConversation(activeConversationId, { provider, model: defaultModel });
        await chatMessages(activeConversationId);
        updateMessage(activeConversationId, messageId, `API key saved for ${providerInfo?.name || provider}.`);
      } catch {
        updateMessage(activeConversationId, messageId, `Failed to save API key for ${providerInfo?.name || provider}.`);
      } finally {
        setSaving(false);
      }
    }

    function cancel() {
      if (!activeConversationId) return;
      removeMessage(activeConversationId, messageId);
    }

    return (
      <div className="normal-message-card api-key-card">
        <div className="card-title">Configure {providerInfo?.name || provider} API key</div>
        <input
          type="password"
          placeholder="sk-..."
          value={value}
          onChange={(e) => setValue(e.target.value)}
          disabled={saving}
        />
        <div className="card-actions">
          <button onClick={save} disabled={saving || !value.trim()}>
            {saving ? 'Saving...' : 'Save'}
          </button>
          <button onClick={cancel} disabled={saving}>Cancel</button>
        </div>
      </div>
    );
  }

  if (!activeConversationId || !activeConversation) {
    return (
      <div className="normal-chat empty">
        <div className="normal-chat-welcome">
          <div className="normal-chat-logo">G</div>
          <h2>Welcome to Goble</h2>
          <p>Start a new conversation from the sidebar.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="normal-chat">
      <div className="normal-chat-header">
        <span className="normal-chat-title">{activeConversation.title || 'Chat'}</span>
        <button
          type="button"
          className={`chat-header-observability-btn ${rightSidebarOpen && rightSidebarTab === 'history' ? 'active' : ''}`}
          title="Observability"
          aria-label="Observability"
          onClick={toggleObservability}
        >
          <Activity size={16} />
        </button>
      </div>

      <div className="normal-chat-messages">
        {messages.length === 0 && (
          <div className="normal-chat-empty">Send a message to start the conversation.</div>
        )}
        {messages.map((m) => {
          const card = m.role === 'system' ? parseCardContent(m.content) : null;
          if (card?.kind === 'api-key' && activeConversationId) {
            return (
              <div key={m.id} className="normal-message system">
                <div className="normal-message-body">
                  <ApiKeyCard messageId={m.id} provider={card.meta.provider} />
                </div>
              </div>
            );
          }
          return (
            <div key={m.id} className={`normal-message ${m.role}`}>
              <div className="normal-message-body">
                {m.role === 'system' ? (
                  <div className="normal-message-system">
                    <span className="normal-message-content">{m.content}</span>
                    {m.content.includes('No model configured') && (
                      <button
                        className="normal-message-configure-btn"
                        onClick={() => navigate('/settings')}
                      >
                        Configure model
                      </button>
                    )}
                  </div>
                ) : (
                  <div className="normal-message-content">{m.content}</div>
                )}
              </div>
            </div>
          );
        })}
        <div ref={messagesEndRef} />
      </div>

      <div className={`normal-chat-status-bar ${running ? 'active' : ''}`}>
        <div className="running-label">
          <span className="running-dot" />
          Running…
        </div>
      </div>
      <div className="normal-chat-composer-wrap">
        <ChatComposer chatId={activeConversationId} onRunningChange={setRunning} />
      </div>
    </div>
  );
}
