import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { DesignSystem, ThemeName, FontName, RadiusName, DensityName } from '../types/common';
import { designSystem, applyThemeClass } from '../utils/designSystem';

const defaultDesign: DesignSystem = {
  theme: 'dark',
  accent: 'blue',
  font: 'system',
  radius: 'default',
  density: 'default',
};

interface DesignState {
  design: DesignSystem;
  setTheme: (theme: ThemeName) => void;
  setFont: (font: FontName) => void;
  setRadius: (radius: RadiusName) => void;
  setDensity: (density: DensityName) => void;
  setAccent: (accent: DesignSystem['accent']) => void;
  applyToRoot: (root: HTMLElement | null) => void;
  tokens: typeof designSystem;
}

export const useDesignStore = create<DesignState>()(
  persist(
    (set, get) => ({
      design: defaultDesign,
      tokens: designSystem,
      setTheme: (theme) => {
        set((state) => ({ design: { ...state.design, theme } }));
        get().applyToRoot(document.getElementById('root'));
      },
      setFont: (font) => {
        set((state) => ({ design: { ...state.design, font } }));
        get().applyToRoot(document.getElementById('root'));
      },
      setRadius: (radius) => {
        set((state) => ({ design: { ...state.design, radius } }));
        get().applyToRoot(document.getElementById('root'));
      },
      setDensity: (density) => {
        set((state) => ({ design: { ...state.design, density } }));
        get().applyToRoot(document.getElementById('root'));
      },
      setAccent: (accent) => {
        set((state) => ({ design: { ...state.design, accent } }));
      },
      applyToRoot: (root) => {
        const { design } = get();
        applyThemeClass(root, design.theme, design.font, design.radius, design.density);
      },
    }),
    {
      name: 'goble-design',
      partialize: (state) => ({ design: state.design }),
    },
  ),
);

export function initializeDesignRoot() {
  useDesignStore.getState().applyToRoot(document.getElementById('root'));
}
