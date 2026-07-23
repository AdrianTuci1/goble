import { useState, useRef, useEffect } from 'react';
import { useStore } from '../stores/appStore';
import { addLog, runAgent } from '../tauri/api';

interface Message {
  id: string;
  role: 'user' | 'agent';
  content: string;
  timestamp: string;
}

export default function ChatArea() {
  const storeActiveConversationId = useStore((s) => s.activeConversationId);
  const addStoreLog = useStore((s) => s.addLog);
  const [messages, setMessages] = useState<Message[]>([]);
  const [inputValue, setInputValue] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, isLoading]);

  const handleSend = async () => {
    if (!inputValue.trim()) return;
    const text = inputValue.trim();
    setInputValue('');

    const userMsg: Message = {
      id: `${Date.now()}`,
      role: 'user',
      content: text,
      timestamp: new Date().toLocaleTimeString(),
    };
    setMessages((m) => [...m, userMsg]);
    setIsLoading(true);

    await addLog(`user: ${text}`);

    try {
      await runAgent('default', text);
      await new Promise((r) => setTimeout(r, 800));
      const agentMsg: Message = {
        id: `${Date.now() + 1}`,
        role: 'agent',
        content: `Răspuns generat pentru: "${text}".`,
        timestamp: new Date().toLocaleTimeString(),
      };
      setMessages((m) => [...m, agentMsg]);
      addStoreLog(`agent: ${agentMsg.content}`);
    } catch (e) {
      addStoreLog(`error: ${e}`);
    } finally {
      setIsLoading(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const title = storeActiveConversationId ? 'Conversație' : 'Te ascult!';

  return (
    <div className="gemini-main">
      <div className="gemini-chat-content-container">
        <div className="gemini-main-header">
          <span style={{ fontSize: 14, color: '#a3a3a3' }}>{title}</span>
        </div>

        {messages.length === 0 ? (
          <div className="gemini-start-screen">
            <h1 className="gemini-start-title">Ce vrei să automatizezi azi?</h1>
          </div>
        ) : (
          <div className="gemini-chat-log">
            {messages.map((msg) => (
              <div
                key={msg.id}
                className={`gemini-bubble ${msg.role}`}
              >
                <div className="gemini-bubble-content">{msg.content}</div>
              </div>
            ))}
            {isLoading && (
              <div className="gemini-bubble model">
                <div className="gemini-bubble-content">Se gândește…</div>
              </div>
            )}
            <div ref={messagesEndRef} />
          </div>
        )}

        <div className="gemini-prompt-box-container">
          <div className="composer">
            <textarea
              rows={1}
              placeholder="Scrie un task sau o întrebare…"
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              onKeyDown={handleKeyDown}
            />
            <button onClick={handleSend} disabled={isLoading}>Trimite</button>
          </div>
        </div>
      </div>
    </div>
  );
}
