import { useState, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { Settings, User, GitBranch, Mic } from 'lucide-react';
import {
  addChatMessage,
  runHarness,
  getLlmSetting,
  setChatModel,
} from '../tauri/api';
import { useStore } from '../stores/appStore';
import ComposerIconButton from './composer/ComposerIconButton';
import ComposerChip from './composer/ComposerChip';
import ComposerModelSelector from './composer/ComposerModelSelector';
import ComposerVariantSelector, { type VariantOption } from './composer/ComposerVariantSelector';
import './ChatComposer.css';

interface ChatComposerProps {
  chatId: string;
  onRunningChange?: (running: boolean) => void;
}

const VARIANTS: VariantOption[] = [
  { id: 'default', label: 'Default', placeholder: '/remote-control' },
  { id: 'agent', label: 'Agent', placeholder: 'Ask an agent...' },
  { id: 'code', label: 'Code', placeholder: 'Generate code...', prefix: '/code' },
  { id: 'voice', label: 'Voice', placeholder: 'Voice input (not yet available)' },
];

function buildCardContent(kind: string, meta: Record<string, string>) {
  return `__CARD__${JSON.stringify({ kind, ...meta })}`;
}

export default function ChatComposer({ chatId, onRunningChange }: ChatComposerProps) {
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [modelOpen, setModelOpen] = useState(false);
  const [variantOpen, setVariantOpen] = useState(false);
  const [variant, setVariant] = useState<VariantOption['id']>('default');
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const navigate = useNavigate();

  const addMessage = useStore((s) => s.addMessage);
  const updateConversation = useStore((s) => s.updateConversation);
  const activeConversation = useStore((s) => s.conversations.find((c) => c.id === chatId));
  const provider = activeConversation?.provider || null;
  const model = activeConversation?.model || null;

  const currentVariant = VARIANTS.find((v) => v.id === variant) || VARIANTS[0];

  async function handleSend() {
    const raw = input.trim();
    if (!raw || !chatId) return;

    const apiKeyMatch = raw.match(/^\/set-api-key\s+(\S+)$/);
    if (apiKeyMatch) {
      const targetProvider = apiKeyMatch[1];
      setInput('');
      const content = buildCardContent('api-key', { provider: targetProvider });
      addMessage(chatId, {
        id: `${Date.now()}-card`,
        role: 'system',
        content,
        created_at: new Date().toISOString(),
      });
      try {
        await addChatMessage(chatId, 'system', content);
      } catch {
        // ignore persistence errors
      }
      return;
    }

    const text = currentVariant.prefix && !raw.startsWith(currentVariant.prefix)
      ? `${currentVariant.prefix} ${raw}`
      : raw;

    const currentProvider = provider;
    let configured = false;
    if (currentProvider) {
      try {
        const setting = await getLlmSetting(currentProvider);
        configured = !!setting && !!setting.api_key && !!setting.model;
      } catch {
        configured = false;
      }
    }

    if (!configured) {
      setInput('');
      addMessage(chatId, {
        id: `${Date.now()}-system`,
        role: 'system',
        content: 'No model configured. Open settings to configure a model before sending messages.',
        created_at: new Date().toISOString(),
      });
      try {
        await addChatMessage(chatId, 'system', 'No model configured. Open settings to configure a model before sending messages.');
      } catch {
        // ignore
      }
      return;
    }

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

  function selectModel(providerId: string, modelId: string) {
    updateConversation(chatId, { provider: providerId, model: modelId });
    setChatModel(chatId, providerId, modelId).catch(() => {});
  }

  function selectVariant(id: VariantOption['id']) {
    setVariant(id);
    setVariantOpen(false);
    textareaRef.current?.focus();
  }

  return (
    <div className="chat-composer-normal">
      <textarea
        ref={textareaRef}
        className="composer-textarea"
        rows={1}
        placeholder={currentVariant.placeholder}
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKeyDown}
        disabled={loading}
      />

      <div className="composer-left">
        <ComposerIconButton title="Settings" onClick={() => navigate('/settings')}>
          <Settings size={16} />
        </ComposerIconButton>

        <ComposerChip title="Profile">
          <User size={12} />
          Default
        </ComposerChip>

        <ComposerModelSelector
          provider={provider}
          model={model}
          open={modelOpen}
          onToggle={() => setModelOpen((v) => !v)}
          onClose={() => setModelOpen(false)}
          onSelect={selectModel}
        />
      </div>

      <div className="composer-right">
        <ComposerIconButton title="Branches" disabled={loading}>
          <GitBranch size={16} />
        </ComposerIconButton>
        <ComposerIconButton title="Voice" disabled={loading}>
          <Mic size={16} />
        </ComposerIconButton>
        <ComposerVariantSelector
          variants={VARIANTS}
          selected={variant}
          open={variantOpen}
          onToggle={() => setVariantOpen((v) => !v)}
          onClose={() => setVariantOpen(false)}
          onSelect={selectVariant}
        />
      </div>
    </div>
  );
}
