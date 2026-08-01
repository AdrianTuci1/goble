import { useEffect, useMemo } from 'react';
import type { DesignSystem } from '../stores/appStore';

const ACCENT_MAP: Record<DesignSystem['accent'], string> = {
  blue: '#2563eb',
  green: '#22c55e',
  purple: '#8b5cf6',
  orange: '#f97316',
};

export function useDesignClasses(design: DesignSystem) {
  return useMemo(() => {
    return `theme-${design.theme} accent-${design.accent} font-${design.font} density-${design.density} radius-${design.radius}`;
  }, [design]);
}

export function applyDesignSystem(design: DesignSystem) {
  const root = document.getElementById('app-root');
  if (!root) return;
  root.classList.remove(
    'theme-dark', 'theme-light', 'theme-midnight',
    'accent-blue', 'accent-green', 'accent-purple', 'accent-orange',
    'font-system', 'font-mono', 'font-serif',
    'density-compact', 'density-default', 'density-spacious',
    'radius-sharp', 'radius-default', 'radius-rounded'
  );
  root.classList.add(
    `theme-${design.theme}`,
    `accent-${design.accent}`,
    `font-${design.font}`,
    `density-${design.density}`,
    `radius-${design.radius}`
  );
  root.style.setProperty('--ds-accent', ACCENT_MAP[design.accent] || ACCENT_MAP.blue);
}

export function useDesignSystemEffect(design: DesignSystem) {
  useEffect(() => {
    applyDesignSystem(design);
  }, [design]);
}

export function getDefaultDesign(): DesignSystem {
  return {
    theme: 'dark',
    accent: 'blue',
    font: 'system',
    density: 'default',
    radius: 'default',
  };
}
