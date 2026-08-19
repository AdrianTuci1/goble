import { useRef, useEffect } from 'react';
import { ChevronDown } from 'lucide-react';
import ComposerChip from './ComposerChip';
import ComposerDropdown from './ComposerDropdown';
import { LLM_PROVIDERS } from '../../tauri/api';
import './ComposerModelSelector.css';

interface ComposerModelSelectorProps {
  provider?: string | null;
  model?: string | null;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
  onSelect: (providerId: string, modelId: string) => void;
}

function formatModelLabel(provider?: string | null, model?: string | null) {
  if (!provider || !model) return 'Select model';
  const providerName = LLM_PROVIDERS.find((p) => p.id === provider)?.name || provider;
  return `${model} (${providerName})`;
}

export default function ComposerModelSelector({
  provider,
  model,
  open,
  onToggle,
  onClose,
  onSelect,
}: ComposerModelSelectorProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function onClickOutside(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    }
    if (open) {
      document.addEventListener('mousedown', onClickOutside);
      return () => document.removeEventListener('mousedown', onClickOutside);
    }
  }, [open, onClose]);

  const items = LLM_PROVIDERS.map((p) => ({
    id: p.id,
    label: p.name,
    meta: p.defaultModel,
  }));

  return (
    <div className="composer-model-selector" ref={ref}>
      <ComposerChip
        interactive
        missing={!provider || !model}
        title="Current model"
        onClick={onToggle}
      >
        {formatModelLabel(provider, model)}
        <ChevronDown size={12} />
      </ComposerChip>
      {open && (
        <ComposerDropdown
          items={items}
          selectedId={provider || undefined}
          onSelect={(id) => {
            const p = LLM_PROVIDERS.find((x) => x.id === id);
            if (p) onSelect(p.id, p.defaultModel);
          }}
        />
      )}
    </div>
  );
}
