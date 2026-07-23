import { useEffect } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { useStore } from './stores/appStore';
import Sidebar from './components/Sidebar';
import ChatArea from './components/ChatArea';
import ConnectorsPage from './pages/ConnectorsPage';
import WorkflowsPage from './pages/WorkflowsPage';
import SearchPage from './pages/SearchPage';
import KnowledgePage from './pages/KnowledgePage';
import SettingsModal from './components/SettingsModal';
import { listWorkers, workerLogs } from './tauri/api';

function AppLayout() {
  const setWorkers = useStore((s) => s.setWorkers);
  const setLogs = useStore((s) => s.setLogs);

  useEffect(() => {
    listWorkers().then(setWorkers).catch(() => {});
    workerLogs().then(setLogs).catch(() => {});
  }, [setWorkers, setLogs]);

  return (
    <div className="app-layout">
      <Sidebar />
      <div className="main-content">
        <Routes>
          <Route path="/chat" element={<ChatArea />} />
          <Route path="/connectors" element={<ConnectorsPage />} />
          <Route path="/workflows" element={<WorkflowsPage />} />
          <Route path="/knowledge" element={<KnowledgePage />} />
          <Route path="/search" element={<SearchPage />} />
          <Route path="*" element={<Navigate to="/chat" />} />
        </Routes>
      </div>
      <SettingsModal />
    </div>
  );
}

function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/*" element={<AppLayout />} />
      </Routes>
    </BrowserRouter>
  );
}

export default App;
