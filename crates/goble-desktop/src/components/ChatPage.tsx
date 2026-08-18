import { useEffect, useRef, useState } from 'react';
import { useStore } from '../stores/appStore';
import {
  listChats,
  createChat,
  chatMessages,
  onChatUpdated,
  onHarnessEvent,
  type HarnessEventPayload,
  type ChatMessage,
} from '../tauri/api';
import { getInitials } from '../utils/designSystem';
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

  const messages = activeConversationId
    ? messagesMap[activeConversationId] || EMPTY_MESSAGES
    : EMPTY_MESSAGES;
  const activeConversation = conversations.find((c) => c.id === activeConversationId);

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
        <span className="normal-chat-meta">
          {activeConversation.provider} / {activeConversation.model}
        </span>
      </div>

      <div className="normal-chat-messages">
        {messages.length === 0 && (
          <div className="normal-chat-empty">Send a message to start the conversation.</div>
        )}
        {messages.map((m) => (
          <div key={m.id} className={`normal-message ${m.role}`}>
            <div className="normal-message-avatar" title={m.role}>
              {getInitials(m.role)}
            </div>
            <div className="normal-message-body">
              <div className="normal-message-content">{m.content}</div>
            </div>
          </div>
        ))}
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
