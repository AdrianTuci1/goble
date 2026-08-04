import { useEffect } from 'react';
import { useParams } from 'react-router-dom';
import Topbar from './Topbar/Topbar';
import Sidebar from './Sidebar/Sidebar';
import RightSidebar from './right-sidebar/RightSidebar';
import Content from './Content/Content';
import { useMainViewStore, type MainPage } from './store/mainViewStore';
import type { Conversation } from '../../shared';
import { listChats, listAgents, listWorkers, listMcpServers, listVaultSecrets, workerLogs, onWorkersUpdated, onLogsUpdated, onAgentsUpdated, onVaultUpdated, onChatsUpdated, onChatUpdated, onAgentLog, onAgentStarted, onAgentFinished, chatMessages } from '../../shared';
import { setChatStoreConversations, setChatStoreMessages } from './Content/chat-window/store/chatStoreBridge';
import './MainView.css';

export default function MainView() {
  const { setConversations, setPage, page } = useMainViewStore();
  const { page: routePage } = useParams<{ page: string }>();

  useEffect(() => {
    if (routePage && routePage !== page) {
      setPage(routePage as MainPage);
    }
  }, [routePage]);

  useEffect(() => {
    let cancelled = false;
    const unlistenPromises: Promise<(() => void) | null>[] = [];

    async function init() {
      try {
        const [workers, logs, agents, vaultSecrets, conversations, mcpServers] = await Promise.all([
          listWorkers(),
          workerLogs(),
          listAgents(),
          listVaultSecrets(),
          listChats(),
          listMcpServers(),
        ]);
        if (cancelled) return;
        void workers;
        void logs;
        void agents;
        void vaultSecrets;
        void mcpServers;
        setChatStoreConversations(conversations as Conversation[]);
        setConversations(conversations as Conversation[], []);
      } catch (e) {
        console.error('Failed to initialize app data', e);
      }
    }
    init();

    async function setupListeners() {
      const promises = [
        onWorkersUpdated(() => listWorkers().then(() => undefined).catch(console.error)),
        onLogsUpdated(() => workerLogs().then(() => undefined).catch(console.error)),
        onAgentsUpdated(() => listAgents().then(() => undefined).catch(console.error)),
        onVaultUpdated(() => listVaultSecrets().then(() => undefined).catch(console.error)),
        onChatsUpdated(() => listChats().then(setChatStoreConversations).catch(console.error)),
        onAgentLog((event) => {
          void event;
        }),
        onAgentStarted((event) => {
          void event;
        }),
        onAgentFinished((event) => {
          void event;
        }),
        onChatUpdated((event) => {
          const payload = event.payload as { chat_id?: string };
          if (payload.chat_id) {
            chatMessages(payload.chat_id)
              .then((msgs) => setChatStoreMessages(payload.chat_id!, msgs))
              .catch(console.error);
          }
        }),
      ];
      for (const promise of promises) {
        unlistenPromises.push(
          promise.catch((err) => {
            console.error('Failed to register listener', err);
            return null;
          })
        );
      }
    }
    setupListeners();

    return () => {
      cancelled = true;
      unlistenPromises.forEach(async (p) => (await p)?.());
    };
  }, []);

  return (
    <div className="main-view-shell">
      <Topbar />
      <div className="main-view">
        <Sidebar />
        <main className="main-area">
          <Content />
        </main>
        <RightSidebar />
      </div>
    </div>
  );
}
