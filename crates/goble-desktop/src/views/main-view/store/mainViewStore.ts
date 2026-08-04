import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { Conversation } from '../../../shared';
import { flowsData } from '../data/flowsData';

export type MainPage = 'chat' | 'agents' | 'connectors' | 'workflows' | 'executions' | 'knowledge' | 'search' | 'teams' | 'vault';

export interface MessageStep {
  id: string;
  time: string;
  label: string;
}

export interface ExecutionRecord {
  id: number;
  time: string;
  title: string;
  steps: MessageStep[];
}

interface MainViewState {
  page: MainPage;
  sidebarCollapsed: boolean;
  rightOpen: boolean;
  rightTab: 'info' | 'history';
  selectedFlowId: string | null;
  selectedAgentId: string | null;
  activeConversations: Conversation[];
  pastConversations: Conversation[];
  executions: ExecutionRecord[];
  setPage: (page: MainPage) => void;
  toggleSidebar: () => void;
  openRight: (tab: 'info' | 'history') => void;
  setRightTab: (tab: 'info' | 'history') => void;
  closeRight: () => void;
  toggleRight: () => void;
  selectFlow: (id: string | null) => void;
  selectAgent: (id: string | null) => void;
  addActiveConversation: (c: Conversation) => void;
  archiveConversation: (id: string) => void;
  ensureActive: (id: string) => void;
  ensurePast: (id: string) => void;
  setConversations: (active: Conversation[], past: Conversation[]) => void;
  addExecution: (title: string, steps: MessageStep[]) => void;
  clearExecutions: () => void;
}

const demoConversations: Conversation[] = flowsData.map((f) => ({
  id: f.id,
  title: f.title,
  updated_at: new Date().toISOString(),
}));

export const useMainViewStore = create<MainViewState>()(
  persist(
    (set, get) => ({
      page: 'chat',
      sidebarCollapsed: false,
      rightOpen: true,
      rightTab: 'info',
      selectedFlowId: null,
      selectedAgentId: null,
      activeConversations: [],
      pastConversations: demoConversations,
      executions: [],
      setPage: (page) => set({ page, selectedAgentId: null }),
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      openRight: (tab) => set({ rightOpen: true, rightTab: tab }),
      setRightTab: (tab) => set({ rightTab: tab }),
      closeRight: () => set({ rightOpen: false }),
      toggleRight: () => set((s) => ({ rightOpen: !s.rightOpen })),
      selectFlow: (id) => set({ selectedFlowId: id, selectedAgentId: null }),
      selectAgent: (id) => set({ selectedAgentId: id, selectedFlowId: null }),
      addActiveConversation: (c) =>
        set((s) => ({
          activeConversations: s.activeConversations.find((x) => x.id === c.id)
            ? s.activeConversations
            : [c, ...s.activeConversations],
        })),
      archiveConversation: (id) =>
        set((s) => {
          const active = s.activeConversations.filter((c) => c.id !== id);
          const past = [...s.pastConversations];
          const removed = s.activeConversations.find((c) => c.id === id);
          if (removed && !past.find((c) => c.id === removed.id)) past.unshift(removed);
          return { activeConversations: active, pastConversations: past };
        }),
      ensureActive: (id) => {
        const { activeConversations, pastConversations } = get();
        if (activeConversations.find((c) => c.id === id)) return;
        const pastIndex = pastConversations.findIndex((c) => c.id === id);
        if (pastIndex !== -1) {
          const conv = pastConversations[pastIndex];
          set({
            activeConversations: [conv, ...activeConversations],
            pastConversations: pastConversations.filter((_, i) => i !== pastIndex),
          });
        }
      },
      ensurePast: (id) => {
        const { activeConversations, pastConversations } = get();
        const activeIndex = activeConversations.findIndex((c) => c.id === id);
        if (activeIndex === -1) return;
        const conv = activeConversations[activeIndex];
        set({
          activeConversations: activeConversations.filter((_, i) => i !== activeIndex),
          pastConversations: [conv, ...pastConversations],
        });
      },
      setConversations: (active, past) => set({ activeConversations: active, pastConversations: past }),
      addExecution: (title, steps) =>
        set((s) => ({
          executions: [{ id: Date.now(), time: new Date().toLocaleTimeString(), title, steps }, ...s.executions],
        })),
      clearExecutions: () => set({ executions: [] }),
    }),
    {
      name: 'goble-main-view',
      partialize: (s) => ({ sidebarCollapsed: s.sidebarCollapsed, rightOpen: s.rightOpen, rightTab: s.rightTab }),
    },
  ),
);
