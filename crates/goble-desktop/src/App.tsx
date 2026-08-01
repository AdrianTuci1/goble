import { useEffect, useState } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import './pages/Pages.css';
import { useStore } from './stores/appStore';
import TitleBar from './components/TitleBar';
import Sidebar from './components/Sidebar';
import ChatArea from './components/ChatArea';
import RightSidebar from './components/RightSidebar';
import ConnectorsPage from './pages/ConnectorsPage';
import AgentsPage from './pages/AgentsPage';
import ThreadsPage from './pages/ThreadsPage';
import SettingsPage from './pages/SettingsPage';
import {
  listWorkers,
  workerLogs,
  listAgents,
  listVaultSecrets,
  listChats,
  listMcpServers,
  chatMessages,
  createChat,
  onWorkersUpdated,
  onLogsUpdated,
  onAgentLog,
  onAgentStarted,
  onAgentFinished,
  onChatUpdated,
  onChatsUpdated,
  onAgentsUpdated,
  onVaultUpdated,
} from './tauri/api';
import { useDesignClasses, applyThemeClass } from './utils/designSystem';

function AppShell() {
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [threadsActive, setThreadsActive] = useState(false);

  const setWorkers = useStore((s) => s.setWorkers);
  const setLogs = useStore((s) => s.setLogs);
  const addLog = useStore((s) => s.addLog);
  const setAgents = useStore((s) => s.setAgents);
  const setVaultSecrets = useStore((s) => s.setVaultSecrets);
  const setConversations = useStore((s) => s.setConversations);
  const setMcpServers = useStore((s) => s.setMcpServers);
  const addConversation = useStore((s) => s.addConversation);
  const setActiveChatId = useStore((s) => s.setActiveConversation);
  const setMessages = useStore((s) => s.setMessages);

  const activeConversationId = useStore((s) => s.activeConversationId);

  const design = useStore((s) => s.design);
  const designClasses = useDesignClasses(design);

  useEffect(() => {
    const root = document.getElementById('root');
    if (root) applyThemeClass(root, design.theme, design.font, design.radius, design.density);
  }, [design]);

  const [loaded, setLoaded] = useState(false);

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
        setWorkers(workers);
        setLogs(logs);
        setAgents(agents);
        setVaultSecrets(vaultSecrets);
        setConversations(conversations);
        setMcpServers(mcpServers);
      } catch (e) {
        console.error('Failed to initialize app data', e);
      } finally {
        setLoaded(true);
      }
    }

    init();

    async function setupListeners() {
      const promises = [
        onWorkersUpdated(() => listWorkers().then(setWorkers).catch(console.error)),
        onLogsUpdated(() => workerLogs().then(setLogs).catch(console.error)),
        onAgentsUpdated(() => listAgents().then(setAgents).catch(console.error)),
        onVaultUpdated(() => listVaultSecrets().then(setVaultSecrets).catch(console.error)),
        onChatsUpdated(() => listChats().then(setConversations).catch(console.error)),
        onAgentLog((event) => {
          const payload = event.payload as { message?: string; worker_id?: string };
          addLog(`[${payload.worker_id || 'worker'}] ${payload.message || 'log'}`);
        }),
        onAgentStarted((event) => {
          const payload = event.payload as { agent_id?: string; trace_id?: string };
          addLog(`agent started ${payload.agent_id} trace ${payload.trace_id}`);
        }),
        onAgentFinished((event) => {
          const payload = event.payload as { status?: string; trace_id?: string };
          addLog(`agent finished ${payload.trace_id} status ${payload.status}`);
        }),
        onChatUpdated((event) => {
          const payload = event.payload as { chat_id?: string };
          if (payload.chat_id) {
            chatMessages(payload.chat_id)
              .then((msgs) => setMessages(payload.chat_id!, msgs))
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
      unlistenPromises.forEach(async (promise) => {
        const unlisten = await promise;
        unlisten?.();
      });
    };
  }, []);

  async function onNewChat() {
    const chatId = await createChat('New chat', 'openai', 'gpt-4o-mini');
    addConversation({ id: chatId, title: 'New chat', provider: 'openai', model: 'gpt-4o-mini', updated_at: new Date().toISOString() });
    setActiveChatId(chatId);
    setThreadsActive(false);
  }

  if (!loaded) return <div className="loading">Loading...</div>;

  return (
    <div id="app" className={`app-shell ${designClasses}`}>
      <TitleBar
        collapsed={sidebarCollapsed}
        onToggleCollapse={() => setSidebarCollapsed((c) => !c)}
        threadsActive={threadsActive}
        onToggleThreads={() => setThreadsActive((t) => !t)}
      />
      <div className="app-body">
        <Sidebar
          collapsed={sidebarCollapsed}
          onNewChat={onNewChat}
          activeConversationId={activeConversationId}
          onSelectConversation={setActiveChatId}
        />
        <main className="main-area">
          <Routes>
            <Route path="/" element={<Navigate to="/chat" />} />
            <Route path="/chat" element={<ChatArea threadsActive={threadsActive} />} />
            <Route path="/threads" element={<ThreadsPage />} />
            <Route path="/agents" element={<AgentsPage />} />
            <Route path="/connectors" element={<ConnectorsPage />} />
            <Route path="/settings" element={<SettingsPage />} />
          </Routes>
        </main>
        <RightSidebar />
      </div>
    </div>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <AppShell />
    </BrowserRouter>
  );
}
