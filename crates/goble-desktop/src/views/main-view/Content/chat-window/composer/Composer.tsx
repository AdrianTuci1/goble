import { useState, useRef, forwardRef, useImperativeHandle } from 'react';
import { User } from 'lucide-react';
import { useChatStore } from '../store/chatStore';
import { useChatApi } from '../ChatWindow';
import { uid, runHarness, setChatModel, getFirstConfiguredModel, useProviderStore } from '../../../../../shared';
import './Composer.css';

export interface ComposerHandle {
  focus: () => void;
}

interface ComposerProps {
  mode: 'default' | 'inline';
}

const Composer = forwardRef<ComposerHandle, ComposerProps>(function Composer({ mode }, ref) {
  const [input, setInput] = useState('');
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const { addMessage, activeConversationId, setTyping, setActiveConversationId, setMessages, conversations, updateConversation, setActiveTrace, updateMessageMeta, commitConversation, setTransientChatId } = useChatStore();
  const chatApi = useChatApi();
  const { endpoints } = useProviderStore();

  useImperativeHandle(ref, () => ({
    focus: () => inputRef.current?.focus(),
  }));

  if (mode === 'inline') {
    return null;
  }

  const activeConversation = conversations.find((c) => c.id === activeConversationId);
  const modelName = activeConversation?.model || null;
  function resolveModelLabel(name: string | null): string {
    if (!name) return 'No model';
    for (const ep of endpoints) {
      for (const m of ep.models) {
        if (m.name === name) return m.alias || m.name;
      }
    }
    return name;
  }
  const modelLabel = resolveModelLabel(modelName);

  function ensureChat() {
    let chatId = activeConversationId;
    if (!chatId) {
      chatId = uid();
      setActiveConversationId(chatId);
      setTransientChatId(chatId);
      setMessages(chatId, []);
    }
    return chatId;
  }

  async function findConfiguredModel(): Promise<{ provider: string; model: string } | null> {
    const configured = getFirstConfiguredModel();
    return configured ? { provider: configured.provider, model: configured.model } : null;
  }

  function titleFromText(text: string): string {
    const firstLine = text.split('\n')[0].trim();
    if (!firstLine) return 'New chat';
    return firstLine.length > 40 ? `${firstLine.slice(0, 40)}…` : firstLine;
  }

  async function handleSend() {
    const text = input.trim();
    if (!text) return;
    const chatId = ensureChat();
    let conversation = useChatStore.getState().conversations.find((c) => c.id === chatId);

    if (!conversation?.model) {
      const configured = await findConfiguredModel();
      if (!configured) {
        setInput('');
        addMessage(chatId, {
          id: uid(),
          role: 'system',
          content: "You don't have a model configured, please click here to configure a model.",
          kind: 'configureLink',
          created_at: new Date().toISOString(),
        });
        inputRef.current?.focus();
        return;
      }
      if (!conversation) {
        commitConversation({ id: chatId, title: 'New chat', provider: configured.provider, model: configured.model, updated_at: new Date().toISOString() });
      } else {
        updateConversation(chatId, { provider: configured.provider, model: configured.model });
      }
      try {
        await setChatModel(chatId, configured.provider, configured.model);
      } catch {
        // best-effort persistence; local state already updated
      }
      conversation = useChatStore.getState().conversations.find((c) => c.id === chatId);
    }

    setInput('');
    addMessage(chatId, { id: uid(), role: 'user', content: text, created_at: new Date().toISOString() });
    if (conversation && (!conversation.title || conversation.title === 'New chat')) {
      updateConversation(chatId, { title: titleFromText(text), updated_at: new Date().toISOString() });
    }
    const traceId = uid();
    setActiveTrace(traceId);
    addMessage(chatId, { id: traceId, role: 'assistant', content: '', created_at: new Date().toISOString(), streaming: true });
    setTyping(true);
    chatApi?.setRenderMode('thinking');

    try {
      await runHarness(chatId, text, conversation?.provider || '', conversation?.model || '');
    } catch (err) {
      setTyping(false);
      chatApi?.setRenderMode(null);
      setActiveTrace(null);
      updateMessageMeta(chatId, traceId, { streaming: false });
      addMessage(chatId, {
        id: uid(),
        role: 'system',
        content: err instanceof Error ? err.message : 'Failed to send message.',
        created_at: new Date().toISOString(),
      });
    }
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
      <div className="composer-footer">
        <div className="composer-chips">
          <span className="composer-chip" title="Profile">
            <User size={12} />
            Default
          </span>
          <span className={`composer-chip model-chip ${modelName ? '' : 'missing'}`} title="Current model">
            {modelLabel}
          </span>
        </div>
      </div>
    </div>
  );
});

export default Composer;
