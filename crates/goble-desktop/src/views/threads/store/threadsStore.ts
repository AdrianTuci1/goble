import { create } from 'zustand';
import { initialWorkspaces, type Workspace, type ThreadMessage } from '../data/threadsData';

export type ThreadsNav = 'inbox' | 'threads' | 'projects';

interface ThreadsState {
  workspaces: Workspace[];
  activeWorkspaceId: string;
  nav: ThreadsNav;
  activeChannelId: string | null;
  activeDmId: string | null;
  setActiveWorkspace: (id: string) => void;
  setNav: (nav: ThreadsNav) => void;
  selectChannel: (id: string) => void;
  selectDm: (id: string) => void;
  addMessage: (channelId: string, message: ThreadMessage) => void;
}

export const useThreadsStore = create<ThreadsState>((set) => ({
  workspaces: initialWorkspaces,
  activeWorkspaceId: initialWorkspaces[0]?.id || '',
  nav: 'threads',
  activeChannelId: initialWorkspaces[0]?.channels[0]?.id || null,
  activeDmId: null,
  setActiveWorkspace: (id) =>
    set((s) => {
      const ws = s.workspaces.find((w) => w.id === id);
      return {
        activeWorkspaceId: id,
        activeChannelId: ws?.channels[0]?.id || null,
        activeDmId: null,
        nav: 'threads',
      };
    }),
  setNav: (nav) => set({ nav }),
  selectChannel: (id) => set({ activeChannelId: id, activeDmId: null, nav: 'threads' }),
  selectDm: (id) => set({ activeDmId: id, activeChannelId: null, nav: 'threads' }),
  addMessage: (channelId, message) =>
    set((s) => {
      const workspaces = s.workspaces.map((w) => {
        const messagesByChannel = { ...w.messagesByChannel };
        if (w.messagesByChannel[channelId]) {
          messagesByChannel[channelId] = [...w.messagesByChannel[channelId], message];
        } else if (w.directMessagesById[channelId]) {
          return {
            ...w,
            directMessagesById: {
              ...w.directMessagesById,
              [channelId]: [...(w.directMessagesById[channelId] || []), message],
            },
          };
        }
        return { ...w, messagesByChannel };
      });
      return { workspaces };
    }),
}));

export function activeWorkspace(store: ThreadsState): Workspace | undefined {
  return store.workspaces.find((w) => w.id === store.activeWorkspaceId);
}

export function activeMessages(store: ThreadsState): ThreadMessage[] {
  const ws = activeWorkspace(store);
  if (!ws) return [];
  if (store.activeChannelId) return ws.messagesByChannel[store.activeChannelId] || [];
  if (store.activeDmId) return ws.directMessagesById[store.activeDmId] || [];
  return [];
}
