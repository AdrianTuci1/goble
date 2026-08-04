import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { Conversation } from '../../../shared';

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
  pendingConversations: Conversation[];
  historyConversations: Conversation[];
  executions: ExecutionRecord[];
  setPage: (page: MainPage) => void;
  toggleSidebar: () => void;
  openRight: (tab: 'info' | 'history') => void;
  setRightTab: (tab: 'info' | 'history') => void;
  closeRight: () => void;
  toggleRight: () => void;
  selectFlow: (id: string | null) => void;
  selectAgent: (id: string | null) => void;
  addHistoryConversation: (c: Conversation) => void;
  removeConversation: (id: string) => void;
  setConversations: (history: Conversation[], pending: Conversation[]) => void;
  addExecution: (title: string, steps: MessageStep[]) => void;
  clearExecutions: () => void;
}

export const useMainViewStore = create<MainViewState>()(
  persist(
    (set) => ({
      page: 'chat',
      sidebarCollapsed: false,
      rightOpen: true,
      rightTab: 'info',
      selectedFlowId: null,
      selectedAgentId: null,
      pendingConversations: [],
      historyConversations: [],
      executions: [],
      setPage: (page) => set({ page, selectedAgentId: null }),
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      openRight: (tab) => set({ rightOpen: true, rightTab: tab }),
      setRightTab: (tab) => set({ rightTab: tab }),
      closeRight: () => set({ rightOpen: false }),
      toggleRight: () => set((s) => ({ rightOpen: !s.rightOpen })),
      selectFlow: (id) => set({ selectedFlowId: id, selectedAgentId: null }),
      selectAgent: (id) => set({ selectedAgentId: id, selectedFlowId: null }),
      addHistoryConversation: (c) =>
        set((s) => ({
          historyConversations: s.historyConversations.find((x) => x.id === c.id)
            ? s.historyConversations
            : [c, ...s.historyConversations],
        })),
      removeConversation: (id) =>
        set((s) => ({
          pendingConversations: s.pendingConversations.filter((c) => c.id !== id),
          historyConversations: s.historyConversations.filter((c) => c.id !== id),
        })),
      setConversations: (history, pending) => set({ historyConversations: history, pendingConversations: pending }),
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
