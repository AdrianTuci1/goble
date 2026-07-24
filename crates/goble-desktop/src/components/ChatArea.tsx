import { useEffect, useRef, useState } from 'react';
import { useStore } from '../stores/appStore';
import {
  createChat,
  chatMessages,
  addChatMessage,
  addLog,
  runAgent,
  runHarness,
  onHarnessEvent,
} from '../tauri/api';
import type { HarnessEventPayload } from '../tauri/api';

interface ToolCallPayload {
  id: string;
  name?: string;
  arguments?: Record<string, unknown>;
  status?: 'finished' | 'error';
  result?: string;
  message?: string;
}

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

export default function ChatArea() {
  const activeChatId = useStore((s) => s.activeConversationId);
  const setActiveChatId = useStore((s) => s.setActiveConversation);
  const conversations = useStore((s) => s.conversations);
  const addConversation = useStore((s) => s.addConversation);
  const messages = useStore((s) => (activeChatId ? s.messages[activeChatId] || [] : []));
  const setMessages = useStore((s) => s.setMessages);
  const addMessage = useStore((s) => s.addMessage);
  const updateMessage = useStore((s) => s.updateMessage);
  const [input, setInput] = useState('');
  const [workerId, setWorkerId] = useState('');
  const [agentId, setAgentId] = useState('');
  const workers = useStore((s) => s.workers);
  const agents = useStore((s) => s.agents);
  const scrollRef = useRef<HTMLDivElement>(null);
  const pendingIds = useRef<Set<string>>(new Set());

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
    let unsub: (() => void) | undefined;
    onHarnessEvent((e) => {
      const { chat_id, event } = e.payload as HarnessEventPayload;
      if (chat_id !== activeChatId) return;
      switch (event.type) {
        case 'AssistantDelta': {
          const text = String(event.payload ?? '');
          const key = `streaming-${chat_id}`;
          const existing = messages.find((m) => m.id === key);
          if (existing) {
            updateMessage(chat_id, key, existing.content + text);
          } else {
            addMessage(chat_id, {
              id: key,
              role: 'assistant',
              content: text,
              created_at: new Date().toISOString(),
            });
            pendingIds.current.add(key);
          }
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
          updateMessage(chat_id, id, JSON.stringify({
            id: tool.id,
            name: tool.name,
            status: 'finished',
            result: tool.result,
          }));
          break;
        }
        case 'ToolCallError': {
          const tool = event as unknown as ToolCallPayload;
          const id = `tool-${tool.id}`;
          updateMessage(chat_id, id, JSON.stringify({
            id: tool.id,
            name: tool.name,
            status: 'error',
            message: tool.message,
          }));
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
  }, [activeChatId, messages, addMessage, updateMessage, setMessages]);

  async function handleSend() {
    if (!input.trim()) return;
    let chatId = activeChatId;
    if (!chatId) {
      chatId = await createChat('New chat');
      addConversation({ id: chatId, title: 'New chat', updated_at: new Date().toISOString() });
      setActiveChatId(chatId);
    }
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

    if (sentInput.startsWith('/')) {
      await runHarness(chatId, sentInput);
    } else if (workerId && agentId) {
      await runAgent(workerId, agentId, sentInput);
    }
  }

  function startNewChat() {
    createChat('New chat').then((id) => {
      addConversation({ id, title: 'New chat', updated_at: new Date().toISOString() });
      setActiveChatId(id);
    });
  }

  function renderMessageContent(content: string, role: string) {
    if (role === 'tool') {
      const tool = tryParseTool(content);
      if (tool) {
        return (
          <div className="tool-call">
            <div className="tool-call-header">tool: {tool.name || tool.id}</div>
            {tool.arguments && (
              <pre className="tool-call-args">{String(JSON.stringify(tool.arguments, null, 2))}</pre>
            )}
            {tool.status === 'finished' && <div className="tool-result">✅ {tool.result}</div>}
            {tool.status === 'error' && <div className="tool-error">❌ {tool.message}</div>}
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
            : 'No chat selected'}
        </div>
        <div className="chat-controls">
          <select value={workerId} onChange={(e) => setWorkerId(e.target.value)}>
            <option value="">Select worker</option>
            {workers.filter((w) => w.paired).map((w) => (
              <option key={w.id} value={w.id}>{w.name}</option>
            ))}
          </select>
          <select value={agentId} onChange={(e) => setAgentId(e.target.value)}>
            <option value="">Select agent</option>
            {agents.map((a) => (
              <option key={a.id} value={a.id}>{a.name}</option>
            ))}
          </select>
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
            {renderMessageContent(m.content, m.role)}
          </div>
        ))}
      </div>
      <div className="chat-input-area">
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
        <button onClick={handleSend}>Send</button>
      </div>
    </div>
  );
}
