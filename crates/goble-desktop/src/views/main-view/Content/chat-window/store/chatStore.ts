import { create } from 'zustand';
import type { ChatMessage, Conversation } from '../../../../../shared';

interface ChatState {
  messagesByChat: Record<string, ChatMessage[]>;
  activeConversationId: string | null;
  conversations: Conversation[];
  typing: boolean;
  activeTrace: string | null;
  setActiveConversationId: (id: string | null) => void;
  setConversations: (conversations: Conversation[]) => void;
  addConversation: (conversation: Conversation) => void;
  setMessages: (chatId: string, messages: ChatMessage[]) => void;
  addMessage: (chatId: string, message: ChatMessage) => void;
  updateMessage: (chatId: string, messageId: string, updater: (content: string) => string) => void;
  setTyping: (typing: boolean) => void;
  setActiveTrace: (id: string | null) => void;
}

export const useChatStore = create<ChatState>((set) => ({
  messagesByChat: {},
  activeConversationId: null,
  conversations: [],
  typing: false,
  activeTrace: null,
  setActiveConversationId: (id) => set({ activeConversationId: id }),
  setConversations: (conversations) => set({ conversations }),
  addConversation: (conversation) =>
    set((s) => ({
      conversations: s.conversations.find((c) => c.id === conversation.id) ? s.conversations : [conversation, ...s.conversations],
    })),
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
  setTyping: (typing) => set({ typing }),
  setActiveTrace: (id) => set({ activeTrace: id }),
}));

export function getActiveMessages(store: ChatState): ChatMessage[] {
  return store.activeConversationId ? store.messagesByChat[store.activeConversationId] || [] : [];
}
