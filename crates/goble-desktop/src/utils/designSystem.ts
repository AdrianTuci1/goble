export const designSystem = {
  colors: {
    bg: '#0f1115',
    surface: '#181b21',
    surfaceRaised: '#21252e',
    border: '#2a2e36',
    text: '#e4e6eb',
    muted: '#8b949e',
    accent: '#2563eb',
    hover: '#21252e',
    selected: '#2a2e36',
  },
  themes: {
    dark: {
      bg: '#0f1115',
      surface: '#181b21',
      surfaceRaised: '#21252e',
      border: '#2a2e36',
      text: '#e4e6eb',
      muted: '#8b949e',
      accent: '#2563eb',
      hover: '#21252e',
      selected: '#2a2e36',
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
      bg: '#0a0c10',
      surface: '#11131a',
      surfaceRaised: '#181b23',
      border: '#1f222b',
      text: '#e8eaed',
      muted: '#6b7280',
      accent: '#2563eb',
      hover: '#181b23',
      selected: '#1e212b',
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

export type ThemeName = 'dark' | 'light' | 'midnight';
export type FontName = 'system' | 'mono' | 'serif';
export type RadiusName = 'sharp' | 'default' | 'rounded';
export type DensityName = 'compact' | 'default' | 'spacious';

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
  if (theme === 'dark') root.classList.add('theme-dark');
}

export function useDesignClasses(design: { theme: ThemeName; font: FontName; radius: RadiusName; density: DensityName }) {
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
