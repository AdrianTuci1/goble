import { useEffect, useState } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { useStore } from './stores/appStore';
import Sidebar from './components/Sidebar';
import ChatArea from './components/ChatArea';
import ConnectorsPage from './pages/ConnectorsPage';
import WorkflowsPage from './pages/WorkflowsPage';
import KnowledgePage from './pages/KnowledgePage';
import SearchPage from './pages/SearchPage';
import AgentsPage from './pages/AgentsPage';
import TeamsPage from './pages/TeamsPage';
import ExecutionsPage from './pages/ExecutionsPage';
import VaultPage from './pages/VaultPage';
import SettingsModal from './components/SettingsModal';
import {
  listWorkers,
  workerLogs,
  listAgents,
  listWorkflows,
  listTeams,
  listExecutions,
  listVaultSecrets,
  listChats,
  onWorkersUpdated,
  onLogsUpdated,
  onAgentLog,
  onAgentStarted,
  onAgentFinished,
  onChatUpdated,
  onChatsUpdated,
  onAgentsUpdated,
  onWorkflowsUpdated,
  onTeamsUpdated,
  onExecutionsUpdated,
  onVaultUpdated,
} from './tauri/api';

function AppShell() {
  const setWorkers = useStore((s) => s.setWorkers);
  const setLogs = useStore((s) => s.setLogs);
  const addLog = useStore((s) => s.addLog);
  const setAgents = useStore((s) => s.setAgents);
  const setWorkflows = useStore((s) => s.setWorkflows);
  const setTeams = useStore((s) => s.setTeams);
  const setExecutions = useStore((s) => s.setExecutions);
  const setVaultSecrets = useStore((s) => s.setVaultSecrets);
  const setConversations = useStore((s) => s.setConversations);
  const addMessage = useStore((s) => s.addMessage);
  const chatMessages = useStore((s) => s.messages);

  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let unsubs: (() => void)[] = [];

    async function init() {
      setWorkers(await listWorkers());
      setLogs(await workerLogs());
      setAgents(await listAgents());
      setWorkflows(await listWorkflows());
      setTeams(await listTeams());
      setExecutions(await listExecutions());
      setVaultSecrets(await listVaultSecrets());
      setConversations(await listChats());
      setLoaded(true);
    }

    init();

    (async () => {
      unsubs.push(await onWorkersUpdated(() => listWorkers().then(setWorkers)));
      unsubs.push(await onLogsUpdated(() => workerLogs().then(setLogs)));
      unsubs.push(await onAgentsUpdated(() => listAgents().then(setAgents)));
      unsubs.push(await onWorkflowsUpdated(() => listWorkflows().then(setWorkflows)));
      unsubs.push(await onTeamsUpdated(() => listTeams().then(setTeams)));
      unsubs.push(await onExecutionsUpdated(() => listExecutions().then(setExecutions)));
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
      unsubs.push(await onChatUpdated((event) => {
        const payload = event.payload as { chat_id?: string };
        if (payload.chat_id) {
          const msgs = chatMessages[payload.chat_id] || [];
          addMessage(payload.chat_id, msgs[msgs.length - 1]);
        }
      }));
    })();

    return () => {
      unsubs.forEach((u) => u());
    };
  }, [
    setWorkers,
    setLogs,
    addLog,
    setAgents,
    setWorkflows,
    setTeams,
    setExecutions,
    setVaultSecrets,
    setConversations,
    addMessage,
    chatMessages,
  ]);

  if (!loaded) return <div className="loading">Loading...</div>;

  return (
    <div className="app-shell">
      <Sidebar />
      <main className="main-area">
        <Routes>
          <Route path="/" element={<Navigate to="/chat" />} />
          <Route path="/chat" element={<ChatArea />} />
          <Route path="/agents" element={<AgentsPage />} />
          <Route path="/workflows" element={<WorkflowsPage />} />
          <Route path="/teams" element={<TeamsPage />} />
          <Route path="/knowledge" element={<KnowledgePage />} />
          <Route path="/connectors" element={<ConnectorsPage />} />
          <Route path="/executions" element={<ExecutionsPage />} />
          <Route path="/vault" element={<VaultPage />} />
          <Route path="/search" element={<SearchPage />} />
        </Routes>
      </main>
      <SettingsModal />
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
