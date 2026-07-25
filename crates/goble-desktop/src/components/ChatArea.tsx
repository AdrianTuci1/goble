import { useEffect, useRef, useState } from 'react';
import { useStore } from '../stores/appStore';
import {
  cancelHarness,
  createChat,
  chatMessages,
  addChatMessage,
  addLog,
  runAgent,
  runHarness,
  setChatModel,
  onHarnessEvent,
  LLM_PROVIDERS,
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
  const updateConversation = useStore((s) => s.updateConversation);
  const messages = useStore((s) => (activeChatId ? s.messages[activeChatId] || [] : []));
  const setMessages = useStore((s) => s.setMessages);
  const addMessage = useStore((s) => s.addMessage);
  const updateMessage = useStore((s) => s.updateMessage);
  const [input, setInput] = useState('');
  const [isRunning, setIsRunning] = useState(false);
  const [workerId, setWorkerId] = useState('');
  const [agentId, setAgentId] = useState('');
  const [provider, setProvider] = useState('openai');
  const [model, setModel] = useState('gpt-4o-mini');
  const workers = useStore((s) => s.workers);
  const agents = useStore((s) => s.agents);
  const scrollRef = useRef<HTMLDivElement>(null);
  const pendingIds = useRef<Set<string>>(new Set());

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
    if (activeConversation?.provider) {
      setProvider(activeConversation.provider);
    }
    if (activeConversation?.model) {
      setModel(activeConversation.model);
    }
  }, [activeConversation?.provider, activeConversation?.model]);

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

  async function handleSend() {
    if (!input.trim()) return;
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
      setIsRunning(true);
      await runHarness(chatId, sentInput, usedProvider, usedModel);
      setIsRunning(false);
    } else if (workerId && agentId) {
      await runAgent(workerId, agentId, sentInput);
    }
  }

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

  function renderMessageContent(content: string, role: string) {
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
          <select value={provider} onChange={(e) => onProviderChange(e.target.value)}>
            {LLM_PROVIDERS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <input
            type="text"
            value={model}
            onChange={(e) => onModelChange(e.target.value)}
            placeholder="model"
            className="model-input"
          />
          <select value={workerId} onChange={(e) => setWorkerId(e.target.value)}>
            <option value="">Select worker</option>
            {workers.filter((w) => w.paired).map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
              </option>
            ))}
          </select>
          <select value={agentId} onChange={(e) => setAgentId(e.target.value)}>
            <option value="">Select agent</option>
            {agents.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
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
        {isRunning && (
          <button
            className="cancel-button"
            onClick={() => activeChatId && cancelHarness(activeChatId)}
          >
            Cancel
          </button>
        )}
      </div>
    </div>
  );
}
