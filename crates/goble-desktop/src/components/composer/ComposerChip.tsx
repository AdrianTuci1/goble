import type { ReactNode } from 'react';
import './ComposerChip.css';

interface ComposerChipProps {
  children: ReactNode;
  title?: string;
  interactive?: boolean;
  missing?: boolean;
  onClick?: () => void;
}

export default function ComposerChip({
  children,
  title,
  interactive,
  missing,
  onClick,
}: ComposerChipProps) {
  return (
    <span
      className={`composer-chip ${interactive ? 'interactive' : ''} ${missing ? 'missing' : ''}`}
      title={title}
      onClick={onClick}
      role={interactive ? 'button' : undefined}
      tabIndex={interactive ? 0 : undefined}
    >
      {children}
    </span>
  );
}
