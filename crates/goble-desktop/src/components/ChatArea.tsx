import { useEffect, useRef, useState } from 'react';
import { useStore } from '../stores/appStore';
import {
  createChat,
  chatMessages,
  addChatMessage,
  addLog,
  runAgent,
  runHarness,
} from '../tauri/api';

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
  const [input, setInput] = useState('');
  const [workerId, setWorkerId] = useState('');
  const [agentId, setAgentId] = useState('');
  const workers = useStore((s) => s.workers);
  const agents = useStore((s) => s.agents);
  const scrollRef = useRef<HTMLDivElement>(null);

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
