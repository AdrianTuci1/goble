import type { ReactNode } from 'react';
import './ComposerDropdown.css';

interface ComposerDropdownItem {
  id: string;
  label: ReactNode;
  meta?: ReactNode;
}

interface ComposerDropdownProps {
  items: ComposerDropdownItem[];
  selectedId?: string;
  align?: 'left' | 'right';
  onSelect: (id: string) => void;
}

export default function ComposerDropdown({
  items,
  selectedId,
  align = 'left',
  onSelect,
}: ComposerDropdownProps) {
  return (
    <div className={`composer-dropdown ${align === 'right' ? 'right' : ''}`}>
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          className={`composer-dropdown-item ${selectedId === item.id ? 'selected' : ''}`}
          onClick={() => onSelect(item.id)}
        >
          <span className="composer-dropdown-label">{item.label}</span>
          {item.meta && <span className="composer-dropdown-meta">{item.meta}</span>}
        </button>
      ))}
    </div>
  );
}
