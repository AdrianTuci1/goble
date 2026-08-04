import type { DesignSystem, ThemeName, FontName, RadiusName, DensityName } from '../types/common';

export const designSystem = {
  colors: {
    bg: '#0a0a0b',
    surface: '#131315',
    surfaceRaised: '#1c1c1f',
    border: '#27272a',
    text: '#f4f4f5',
    muted: '#a1a1aa',
    accent: '#52525b',
    hover: '#1c1c1f',
    selected: '#27272a',
  },
  themes: {
    dark: {
      bg: '#0a0a0b',
      surface: '#131315',
      surfaceRaised: '#1c1c1f',
      border: '#27272a',
      text: '#f4f4f5',
      muted: '#a1a1aa',
      accent: '#52525b',
      hover: '#1c1c1f',
      selected: '#27272a',
    },
    light: {
      bg: '#f6f7f9',
      surface: '#ffffff',
      surfaceRaised: '#f3f4f6',
      border: '#e2e4e9',
      text: '#1f2937',
      muted: '#6b7280',
      accent: '#2563eb',
      hover: '#f3f4f6',
      selected: '#e5e7eb',
    },
    midnight: {
      bg: '#050507',
      surface: '#0e0e10',
      surfaceRaised: '#161618',
      border: '#202023',
      text: '#f4f4f5',
      muted: '#71717a',
      accent: '#52525b',
      hover: '#161618',
      selected: '#202023',
    },
  },
  fonts: {
    system: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
    mono: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
    serif: 'Georgia, Cambria, "Times New Roman", Times, serif',
  },
  radius: {
    sharp: '0px',
    default: '8px',
    rounded: '14px',
  },
  density: {
    compact: 0.85,
    default: 1,
    spacious: 1.15,
  },
  spacing: {
    xs: '4px',
    sm: '8px',
    md: '12px',
    lg: '16px',
    xl: '24px',
  },
  shadows: {
    sm: '0 1px 2px rgba(0,0,0,0.15)',
    md: '0 4px 12px rgba(0,0,0,0.25)',
  },
} as const;

export function applyThemeClass(
  root: HTMLElement | null,
  theme: ThemeName,
  font: FontName,
  radius: RadiusName,
  density: DensityName,
) {
  if (!root) return;
  root.classList.remove('theme-light', 'theme-midnight', 'theme-dark');
  root.classList.remove('font-system', 'font-mono', 'font-serif');
  root.classList.remove('radius-sharp', 'radius-default', 'radius-rounded');
  root.classList.remove('density-compact', 'density-default', 'density-spacious');
  root.classList.add(`theme-${theme}`, `font-${font}`, `radius-${radius}`, `density-${density}`);
}

export function useDesignClasses(design: DesignSystem) {
  return `theme-${design.theme} font-${design.font} radius-${design.radius} density-${design.density}`;
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

export function uid() {
  return Math.random().toString(36).slice(2, 11) + Date.now().toString(36).slice(-4);
}

export function getInitials(name: string) {
  return name
    .split(' ')
    .map((n) => n[0])
    .join('')
    .slice(0, 2)
    .toUpperCase();
}

export function hslHash(str: string) {
  let hash = 0;
  for (let i = 0; i < str.length; i++) hash = str.charCodeAt(i) + ((hash << 5) - hash);
  return `hsl(${Math.abs(hash) % 360}, 60%, 45%)`;
}

export function debounce<T extends (...args: unknown[]) => void>(fn: T, ms: number) {
  let t: ReturnType<typeof setTimeout> | null = null;
  return (...args: Parameters<T>) => {
    if (t) clearTimeout(t);
    t = setTimeout(() => fn(...args), ms);
  };
}

export function isDefined<T>(value: T | undefined | null): value is T {
  return value !== undefined && value !== null;
}

export function redact(str: string) {
  return str.replace(/(?:api[_-]?key|token|password|secret|credential)[\s=:]+[^\s]+/gi, '[REDACTED]');
}
