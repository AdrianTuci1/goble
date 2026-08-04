import { useChatStore } from './chatStore';
import type { Conversation, ChatMessage } from '../../../../../shared';

export function setChatStoreConversations(conversations: Conversation[]) {
  useChatStore.getState().setConversations(conversations);
}

export function setChatStoreMessages(chatId: string, messages: ChatMessage[]) {
  useChatStore.getState().setMessages(chatId, messages);
}
