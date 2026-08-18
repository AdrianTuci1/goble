import { useState, useRef, useEffect } from 'react';
import { User } from 'lucide-react';
import { addChatMessage, runHarness } from '../tauri/api';
import { useStore } from '../stores/appStore';
import './ChatComposer.css';

interface ChatComposerProps {
  chatId: string;
  onRunningChange?: (running: boolean) => void;
}

export default function ChatComposer({ chatId, onRunningChange }: ChatComposerProps) {
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [input]);
  const addMessage = useStore((s) => s.addMessage);
  const activeConversation = useStore((s) =>
    s.conversations.find((c) => c.id === chatId)
  );
  const modelName = activeConversation?.model || null;

  async function handleSend() {
    const text = input.trim();
    if (!text || !chatId) return;
    setInput('');
    setLoading(true);
    onRunningChange?.(true);
    try {
      await addChatMessage(chatId, 'user', text);
      addMessage(chatId, {
        id: `${Date.now()}-user`,
        role: 'user',
        content: text,
        created_at: new Date().toISOString(),
      });
      await runHarness(chatId, text);
    } catch {
      // Backend events will populate the assistant reply; ignore local errors.
    } finally {
      setLoading(false);
      onRunningChange?.(false);
      textareaRef.current?.focus();
    }
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  return (
    <div className="chat-composer-normal">
      <div className="composer-content">
        <div className="composer-input-wrap">
          <div className="composer-chips">
            <span className="composer-chip" title="Profile">
              <User size={12} />
              Default
            </span>
            <span
              className={`composer-chip model-chip ${modelName ? '' : 'missing'}`}
              title="Current model"
            >
              {modelName || 'No model'}
            </span>
          </div>
          <textarea
            ref={textareaRef}
            className="composer-input"
            rows={1}
            placeholder="Message..."
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            disabled={loading}
          />
        </div>
      </div>
    </div>
  );
}
