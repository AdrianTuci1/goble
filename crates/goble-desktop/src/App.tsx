import { useEffect, useState } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import './pages/Pages.css';
import { useStore } from './stores/appStore';
import Topbar from './components/Topbar';
import ChatPage from './components/ChatPage';
import MainView from './views/MainView';
import ThreadsView from './views/ThreadsView';
import ConnectorsPage from './pages/ConnectorsPage';
import AgentsPage from './pages/AgentsPage';
import SettingsPage from './pages/SettingsPage';
import AgentTracePage from './pages/AgentTracePage';
import WorkflowsPage from './pages/WorkflowsPage';
import TeamsPage from './pages/TeamsPage';
import VaultPage from './pages/VaultPage';
import ExecutionsPage from './pages/ExecutionsPage';
import LogsPage from './pages/LogsPage';
import SearchPage from './pages/SearchPage';
import {
  listWorkers,
  workerLogs,
  listAgents,
  listVaultSecrets,
  listChats,
  listMcpServers,
  chatMessages,
  createChat,
  onAgentStateUpdate,
  onAgentToolResult,
  onChatsUpdated,
  onLogsUpdated,
  onWorkersUpdated,
  onAgentLog,
  onAgentStarted,
  onAgentFinished,
  onChatUpdated,
  onAgentsUpdated,
  onVaultUpdated,
} from './tauri/api';
import type { StateUpdateEvent, ToolResultEvent } from './tauri/api';
import { useDesignClasses, loadDesign, saveDesign } from './utils/designSystem';

function AppShell() {
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

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
  const setAgentState = useStore((s) => s.setAgentState);
  const addAgentToolResult = useStore((s) => s.addAgentToolResult);

  const design = useStore((s) => s.design);
  const setDesign = useStore((s) => s.setDesign);
  const designClasses = useDesignClasses(design);

  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    saveDesign(design);
  }, [design]);

  useEffect(() => {
    const unsubs: (() => void)[] = [];

    async function init() {
      const savedDesign = loadDesign();
      if (savedDesign) setDesign(savedDesign);
      setWorkers(await listWorkers());
      setLogs(await workerLogs());
      setAgents(await listAgents());
      setVaultSecrets(await listVaultSecrets());
      setConversations(await listChats());
      setMcpServers(await listMcpServers());
      setLoaded(true);
    }

    init();

    (async () => {
      unsubs.push(await onWorkersUpdated(() => listWorkers().then(setWorkers)));
      unsubs.push(await onLogsUpdated(() => workerLogs().then(setLogs)));
      unsubs.push(await onAgentsUpdated(() => listAgents().then(setAgents)));
      unsubs.push(await onVaultUpdated(() => listVaultSecrets().then(setVaultSecrets)));
      unsubs.push(await onChatsUpdated(() => listChats().then(setConversations)));
      unsubs.push(await onAgentLog((event) => {
        const payload = event.payload as { message?: string; worker_id?: string };
        addLog(`[${payload.worker_id || 'worker'}] ${payload.message || 'log'}`);
      }));
      unsubs.push(await onAgentStarted((event) => {
        const payload = event.payload as { agent_id?: string; trace_id?: string };
        addLog(`agent started ${payload.agent_id} trace ${payload.trace_id}`);
      }));
      unsubs.push(await onAgentFinished((event) => {
        const payload = event.payload as { status?: string; trace_id?: string };
        addLog(`agent finished ${payload.trace_id} status ${payload.status}`);
      }));
      unsubs.push(await onAgentStateUpdate((event) => {
        const payload = event.payload as StateUpdateEvent;
        setAgentState(payload.trace_id, payload.state);
      }));
      unsubs.push(await onAgentToolResult((event) => {
        const payload = event.payload as ToolResultEvent;
        addAgentToolResult(payload.trace_id, payload);
      }));
      unsubs.push(await onChatUpdated((event) => {
        const payload = event.payload as { chat_id?: string };
        if (payload.chat_id) {
          chatMessages(payload.chat_id)
            .then((msgs) => setMessages(payload.chat_id!, msgs))
            .catch(console.error);
        }
      }));
    })();

    return () => {
      unsubs.forEach((u) => u());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function onNewChat() {
    const chatId = await createChat('New chat', 'openai', 'gpt-4o-mini');
    addConversation({ id: chatId, title: 'New chat', provider: 'openai', model: 'gpt-4o-mini', updated_at: new Date().toISOString() });
    setActiveChatId(chatId);
  }

  if (!loaded) return <div className="loading">Loading...</div>;

  return (
    <div id="app" className={`app-shell ${designClasses}`}>
      <Topbar
        collapsed={sidebarCollapsed}
        onToggleSidebar={() => setSidebarCollapsed((c) => !c)}
      />
      <div className="app-body">
        <Routes>
          <Route path="/" element={<Navigate to="/chat" replace />} />
          <Route element={<MainView collapsed={sidebarCollapsed} onNewChat={onNewChat} />}>
            <Route path="/chat" element={<ChatPage />} />
            <Route path="/agents" element={<AgentsPage />} />
            <Route path="/connectors" element={<ConnectorsPage />} />
            <Route path="/traces" element={<AgentTracePage />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="/workflows" element={<WorkflowsPage />} />
            <Route path="/teams" element={<TeamsPage />} />
            <Route path="/vault" element={<VaultPage />} />
            <Route path="/executions" element={<ExecutionsPage />} />
            <Route path="/logs" element={<LogsPage />} />
            <Route path="/search" element={<SearchPage />} />
          </Route>
          <Route path="/threads" element={<ThreadsView />} />
          <Route path="/threads/*" element={<ThreadsView />} />
          <Route path="*" element={<Navigate to="/chat" replace />} />
        </Routes>
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
