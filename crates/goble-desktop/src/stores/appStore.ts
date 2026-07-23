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

export interface ChatMessage {
  id: string;
  role: string;
  content: string;
  created_at: string;
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
  messages: Record<string, ChatMessage[]>;
  isWorkflowDrawerOpen: boolean;
  isSettingsOpen: boolean;
  setWorkers: (workers: WorkerInfo[]) => void;
  setLogs: (logs: LogEntry[]) => void;
  addLog: (message: string) => void;
  setConversations: (conversations: Conversation[]) => void;
  addConversation: (conversation: Conversation) => void;
  setMessages: (chatId: string, messages: ChatMessage[]) => void;
  addMessage: (chatId: string, message: ChatMessage) => void;
  setActiveConversation: (id: string | null) => void;
  toggleWorkflowDrawer: () => void;
  setSettingsOpen: (open: boolean) => void;
}

export const useStore = create<AppState>((set) => ({
  workers: [],
  logs: [],
  conversations: [],
  activeConversationId: null,
  messages: {},
  isWorkflowDrawerOpen: false,
  isSettingsOpen: false,
  setWorkers: (workers) => set({ workers }),
  setLogs: (logs) => set({ logs }),
  addLog: (message) =>
    set((state) => ({
      logs: [
        ...state.logs,
        { id: `${Date.now()}`, timestamp: new Date().toISOString(), message },
      ],
    })),
  setConversations: (conversations) => set({ conversations }),
  addConversation: (conversation) =>
    set((state) => ({ conversations: [conversation, ...state.conversations] })),
  setMessages: (chatId, messages) =>
    set((state) => ({
      messages: { ...state.messages, [chatId]: messages },
    })),
  addMessage: (chatId, message) =>
    set((state) => ({
      messages: {
        ...state.messages,
        [chatId]: [...(state.messages[chatId] || []), message],
      },
    })),
  setActiveConversation: (id) => set({ activeConversationId: id }),
  toggleWorkflowDrawer: () =>
    set((state) => ({ isWorkflowDrawerOpen: !state.isWorkflowDrawerOpen })),
  setSettingsOpen: (open) => set({ isSettingsOpen: open }),
}));
