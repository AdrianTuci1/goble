import { create } from 'zustand';

export type SettingsSection =
  | 'appearance'
  | 'general'
  | 'profile'
  | 'providers'
  | 'workers'
  | 'cluster'
  | 'vault'
  | 'connectors'
  | 'about';

interface UserSettingsState {
  section: SettingsSection;
  setSection: (section: SettingsSection) => void;
}

export const useUserSettingsStore = create<UserSettingsState>((set) => ({
  section: 'appearance',
  setSection: (section) => set({ section }),
}));
