import { useState, useRef } from 'react';
import { AtSign, Paperclip, Smile, Hash, Type } from 'lucide-react';
import { Send } from 'lucide-react';
import { useChatStore } from '../store/chatStore';
import './Composer.css';

export default function Composer() {
  const [input, setInput] = useState('');
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const { addMessage, activeConversationId, setTyping } = useChatStore();

  async function handleSend() {
    if (!input.trim() || !activeConversationId) return;
    const text = input.trim();
    setInput('');
    addMessage(activeConversationId, { id: uid(), role: 'user', content: text, created_at: new Date().toISOString() });
    setTyping(true);
    setTimeout(() => {
      setTyping(false);
      addMessage(activeConversationId, { id: uid(), role: 'assistant', content: 'Received: ' + text, created_at: new Date().toISOString() });
    }, 600);
    inputRef.current?.focus();
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  return (
    <div className="composer">
      <div className="composer-input-row">
        <textarea
          ref={inputRef}
          className="composer-input"
          placeholder="Message..."
          rows={1}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
        />
      </div>
      <div className="composer-toolbar">
        <div className="composer-toolbar-left">
          <button className="composer-toolbar-btn" title="Mention"><AtSign size={16} /></button>
          <button className="composer-toolbar-btn" title="Attach"><Paperclip size={16} /></button>
          <button className="composer-toolbar-btn" title="Emoji"><Smile size={16} /></button>
          <button className="composer-toolbar-btn" title="Tag"><Hash size={16} /></button>
          <button className="composer-toolbar-btn" title="Format"><Type size={16} /></button>
        </div>
        <button className="composer-send" onClick={handleSend} disabled={!input.trim() || !activeConversationId}>
          <Send size={16} />
        </button>
      </div>
    </div>
  );
}

function uid() {
  return Math.random().toString(36).slice(2, 11) + Date.now().toString(36).slice(-4);
}
