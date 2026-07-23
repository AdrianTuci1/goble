import { useState, useRef, useEffect } from 'react';
import { useStore } from '../stores/appStore';
import { createChat, chatMessages, addChatMessage, addLog, runAgent } from '../tauri/api';

export default function ChatArea() {
  const activeChatId = useStore((s) => s.activeConversationId);
  const setActiveChatId = useStore((s) => s.setActiveConversation);
  const conversations = useStore((s) => s.conversations);
  const addConversation = useStore((s) => s.addConversation);
  const messages = useStore((s) => (activeChatId ? s.messages[activeChatId] || [] : []));
  const addMessage = useStore((s) => s.addMessage);
  const [input, setInput] = useState('');
  const [workerId, setWorkerId] = useState('');
  const [agentId, setAgentId] = useState('');
  const workers = useStore((s) => s.workers);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  useEffect(() => {
    if (activeChatId) {
      chatMessages(activeChatId).then((msgs) => {
        for (const msg of msgs) {
          addMessage(activeChatId, msg);
        }
      });
    }
  }, [activeChatId, addMessage]);

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
    setInput('');
    addLog(`user sent message in chat ${chatId}`);

    if (workerId && agentId) {
      await runAgent(workerId, agentId, input);
    }
  }

  function startNewChat() {
    createChat('New chat').then((id) => {
      addConversation({ id, title: 'New chat', updated_at: new Date().toISOString() });
      setActiveChatId(id);
    });
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
          <input
            value={agentId}
            onChange={(e) => setAgentId(e.target.value)}
            placeholder="Agent ID"
          />
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
            <div className="message-content">{m.content}</div>
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
          placeholder="Type a message..."
        />
        <button onClick={handleSend}>Send</button>
      </div>
    </div>
  );
}
