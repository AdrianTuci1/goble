import { create } from 'zustand';

export interface WorkerInfo {
  id: string;
  name: string;
  url: string;
  paired: boolean;
}

export interface Conversation {
  id: string;
  title: string;
  updated_at: string;
}

export interface LogEntry {
  id: string;
  timestamp: string;
  message: string;
}

interface AppState {
  workers: WorkerInfo[];
  logs: LogEntry[];
  conversations: Conversation[];
  activeConversationId: string | null;
  isWorkflowDrawerOpen: boolean;
  isSettingsOpen: boolean;
  setWorkers: (workers: WorkerInfo[]) => void;
  setLogs: (logs: string[]) => void;
  addLog: (message: string) => void;
  setConversations: (conversations: Conversation[]) => void;
  addConversation: (conversation: Conversation) => void;
  setActiveConversation: (id: string | null) => void;
  toggleWorkflowDrawer: () => void;
  setSettingsOpen: (open: boolean) => void;
}

export const useStore = create<AppState>((set) => ({
  workers: [],
  logs: [],
  conversations: [],
  activeConversationId: null,
  isWorkflowDrawerOpen: false,
  isSettingsOpen: false,
  setWorkers: (workers) => set({ workers }),
  setLogs: (logLines) => set({
    logs: logLines.map((message, i) => ({
      id: `${i}`,
      timestamp: new Date().toISOString(),
      message,
    })),
  }),
  addLog: (message) => set((state) => ({
    logs: [
      ...state.logs,
      { id: `${Date.now()}`, timestamp: new Date().toISOString(), message },
    ],
  })),
  setConversations: (conversations) => set({ conversations }),
  addConversation: (conversation) =>
    set((state) => ({ conversations: [conversation, ...state.conversations] })),
  setActiveConversation: (id) => set({ activeConversationId: id }),
  toggleWorkflowDrawer: () =>
    set((state) => ({ isWorkflowDrawerOpen: !state.isWorkflowDrawerOpen })),
  setSettingsOpen: (open) => set({ isSettingsOpen: open }),
}));
