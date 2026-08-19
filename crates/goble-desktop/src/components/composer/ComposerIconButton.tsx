import type { ReactNode } from 'react';
import './ComposerIconButton.css';

interface ComposerIconButtonProps {
  children: ReactNode;
  title?: string;
  disabled?: boolean;
  active?: boolean;
  onClick?: () => void;
}

export default function ComposerIconButton({
  children,
  title,
  disabled,
  active,
  onClick,
}: ComposerIconButtonProps) {
  return (
    <button
      type="button"
      className={`composer-icon-btn ${active ? 'active' : ''}`}
      title={title}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
