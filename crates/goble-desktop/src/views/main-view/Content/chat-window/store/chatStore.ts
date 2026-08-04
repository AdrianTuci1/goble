import { create } from 'zustand';
import type { ChatMessage as BaseChatMessage, Conversation } from '../../../../../shared';
import { useMainViewStore } from '../../../store/mainViewStore';

export interface CodeChangeFile {
  path: string;
  mode?: 'created' | 'modified' | 'deleted';
  content: string;
}

export interface ActionListItem {
  id: string;
  label: string;
  status?: 'pending' | 'done' | 'failed' | 'canceled';
  statusText?: string;
}

export interface FormField {
  name: string;
  label: string;
  type?: string;
  options?: string[];
}

export type AppChatMessage = BaseChatMessage & {
  kind?: 'text' | 'codeBlock' | 'codeChangeCard' | 'toolCall' | 'actionList' | 'variantCard' | 'secretCard' | 'confirmationCard' | 'formCard' | 'configureLink';
  language?: string;
  code?: string;
  description?: string;
  files?: CodeChangeFile[];
  tool?: string;
  args?: Record<string, unknown>;
  title?: string;
  options?: string[];
  scope?: string;
  message?: string;
  actions?: string[];
  fields?: FormField[];
  items?: ActionListItem[];
  streaming?: boolean;
};

interface ChatState {
  messagesByChat: Record<string, AppChatMessage[]>;
  activeConversationId: string | null;
  conversations: Conversation[];
  typing: boolean;
  activeTrace: string | null;
  transientChatId: string | null;
  setActiveConversationId: (id: string | null) => void;
  setConversations: (conversations: Conversation[]) => void;
  addConversation: (conversation: Conversation) => void;
  updateConversation: (id: string, updates: Partial<Conversation>) => void;
  setMessages: (chatId: string, messages: AppChatMessage[]) => void;
  addMessage: (chatId: string, message: AppChatMessage) => void;
  updateMessage: (chatId: string, messageId: string, updater: (content: string) => string) => void;
  updateMessageMeta: (chatId: string, messageId: string, meta: Partial<AppChatMessage>) => void;
  setTyping: (typing: boolean) => void;
  setActiveTrace: (id: string | null) => void;
  setTransientChatId: (id: string | null) => void;
  commitConversation: (conversation: Conversation) => void;
  clearTransientChat: () => void;
  deleteConversation: (id: string) => void;
}

export const useChatStore = create<ChatState>((set) => ({
  messagesByChat: {},
  activeConversationId: null,
  conversations: [],
  typing: false,
  activeTrace: null,
  transientChatId: null,
  setActiveConversationId: (id) => set({ activeConversationId: id }),
  setConversations: (conversations) => set({ conversations }),
  addConversation: (conversation) =>
    set((s) => ({
      conversations: s.conversations.find((c) => c.id === conversation.id) ? s.conversations : [conversation, ...s.conversations],
    })),
  updateConversation: (id, updates) =>
    set((s) => {
      const main = useMainViewStore.getState();
      const historyIndex = main.historyConversations.findIndex((c) => c.id === id);
      const pendingIndex = main.pendingConversations.findIndex((c) => c.id === id);
      if (historyIndex !== -1) {
        main.setConversations(
          main.historyConversations.map((c) => (c.id === id ? { ...c, ...updates } : c)),
          main.pendingConversations,
        );
      } else if (pendingIndex !== -1) {
        main.setConversations(
          main.historyConversations,
          main.pendingConversations.map((c) => (c.id === id ? { ...c, ...updates } : c)),
        );
      }
      return {
        conversations: s.conversations.map((c) => (c.id === id ? { ...c, ...updates } : c)),
      };
    }),
  setMessages: (chatId, messages) => set((s) => ({ messagesByChat: { ...s.messagesByChat, [chatId]: messages } })),
  addMessage: (chatId, message) =>
    set((s) => ({
      messagesByChat: {
        ...s.messagesByChat,
        [chatId]: [...(s.messagesByChat[chatId] || []), message],
      },
    })),
  updateMessage: (chatId, messageId, updater) =>
    set((s) => {
      const msgs = s.messagesByChat[chatId] || [];
      return {
        messagesByChat: {
          ...s.messagesByChat,
          [chatId]: msgs.map((m) => (m.id === messageId ? { ...m, content: updater(m.content) } : m)),
        },
      };
    }),
  updateMessageMeta: (chatId, messageId, meta) =>
    set((s) => {
      const msgs = s.messagesByChat[chatId] || [];
      return {
        messagesByChat: {
          ...s.messagesByChat,
          [chatId]: msgs.map((m) => (m.id === messageId ? { ...m, ...meta } : m)),
        },
      };
    }),
  setTyping: (typing) => set({ typing }),
  setActiveTrace: (id) => set({ activeTrace: id }),
  setTransientChatId: (id) => set({ transientChatId: id }),
  commitConversation: (conversation) =>
    set((s) => {
      if (s.conversations.find((c) => c.id === conversation.id)) {
        return { transientChatId: s.transientChatId === conversation.id ? null : s.transientChatId };
      }
      useMainViewStore.getState().addHistoryConversation(conversation);
      return {
        conversations: [conversation, ...s.conversations],
        transientChatId: s.transientChatId === conversation.id ? null : s.transientChatId,
      };
    }),
  clearTransientChat: () =>
    set((s) => {
      const id = s.transientChatId;
      if (!id) return {};
      const messagesByChat = { ...s.messagesByChat };
      delete messagesByChat[id];
      return {
        activeConversationId: s.activeConversationId === id ? null : s.activeConversationId,
        messagesByChat,
        transientChatId: null,
      };
    }),
  deleteConversation: (id) =>
    set((s) => {
      useMainViewStore.getState().removeConversation(id);
      const messagesByChat = { ...s.messagesByChat };
      delete messagesByChat[id];
      return {
        conversations: s.conversations.filter((c) => c.id !== id),
        messagesByChat,
        activeConversationId: s.activeConversationId === id ? null : s.activeConversationId,
      };
    }),
}));

export function getActiveMessages(store: ChatState): AppChatMessage[] {
  return store.activeConversationId ? store.messagesByChat[store.activeConversationId] || [] : [];
}
