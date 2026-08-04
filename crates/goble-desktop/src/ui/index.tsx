import type { ButtonHTMLAttributes, ReactNode } from 'react';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
  size?: 'sm' | 'md';
  children: ReactNode;
}

export function Button({ variant = 'primary', size = 'md', children, className = '', ...rest }: ButtonProps) {
  const classes = ['ui-btn', `ui-btn-${variant}`, `ui-btn-${size}`, className].join(' ');
  return (
    <button className={classes} {...rest}>
      {children}
    </button>
  );
}

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  children: ReactNode;
}

export function IconButton({ label, children, className = '', ...rest }: IconButtonProps) {
  return (
    <button className={`ui-icon-btn ${className}`} title={label} aria-label={label} {...rest}>
      {children}
    </button>
  );
}

interface AvatarProps {
  name: string;
  color?: string;
  size?: 'sm' | 'md' | 'lg';
}

export function Avatar({ name, color, size = 'md' }: AvatarProps) {
  const initials = name
    .split(' ')
    .map((n) => n[0])
    .join('')
    .slice(0, 2)
    .toUpperCase();
  const bg = color || `hsl(${Math.abs(name.split('').reduce((a, b) => a + b.charCodeAt(0), 0)) % 360}, 60%, 45%)`;
  return <span className={`ui-avatar ui-avatar-${size}`} style={{ background: bg }}>{initials}</span>;
}

interface BadgeProps {
  children: ReactNode;
}

export function Badge({ children }: BadgeProps) {
  return <span className="ui-badge">{children}</span>;
}

export function Spinner() {
  return <span className="ui-spinner" aria-label="loading" />;
}
