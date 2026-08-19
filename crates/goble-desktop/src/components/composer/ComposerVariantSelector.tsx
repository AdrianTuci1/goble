import { useRef, useEffect } from 'react';
import { Plus } from 'lucide-react';
import ComposerIconButton from './ComposerIconButton';
import ComposerDropdown from './ComposerDropdown';
import './ComposerVariantSelector.css';

export type ComposerVariant = 'default' | 'agent' | 'code' | 'voice';

export interface VariantOption {
  id: ComposerVariant;
  label: string;
  placeholder: string;
  prefix?: string;
}

interface ComposerVariantSelectorProps {
  variants: VariantOption[];
  selected: ComposerVariant;
  open: boolean;
  onToggle: () => void;
  onClose: () => void;
  onSelect: (variant: ComposerVariant) => void;
}

export default function ComposerVariantSelector({
  variants,
  selected,
  open,
  onToggle,
  onClose,
  onSelect,
}: ComposerVariantSelectorProps) {
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

  const items = variants.map((v) => ({ id: v.id, label: v.label }));

  return (
    <div className="composer-variant-selector" ref={ref}>
      <ComposerIconButton title="Variants" active={open} onClick={onToggle}>
        <Plus size={16} />
      </ComposerIconButton>
      {open && (
        <ComposerDropdown
          items={items}
          selectedId={selected}
          align="right"
          onSelect={(id) => onSelect(id as ComposerVariant)}
        />
      )}
    </div>
  );
}
