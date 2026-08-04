import { useEffect } from 'react';
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import './App.css';
import './ui/index.css';
import Topbar from './views/main-view/Topbar/Topbar';
import MainView from './views/main-view/MainView';
import ThreadsView from './views/threads/ThreadsView';
import UserSettingsView from './views/user-settings/UserSettingsView';
import { useDesignStore, initializeDesignRoot } from './shared';

function AppShell() {
  const design = useDesignStore((s) => s.design);

  useEffect(() => {
    initializeDesignRoot();
  }, []);

  useEffect(() => {
    const root = document.getElementById('root');
    if (root) {
      root.classList.remove('theme-dark', 'theme-light', 'theme-midnight');
      root.classList.add(`theme-${design.theme}`);
    }
  }, [design.theme]);

  return (
    <div
      id="app"
      className={`app-shell theme-${design.theme} font-${design.font} radius-${design.radius} density-${design.density}`}
    >
      <Topbar />
      <div className="app-body">
        <Routes>
          <Route path="/" element={<Navigate to="/main/chat" replace />} />
          <Route path="/main" element={<Navigate to="/main/chat" replace />} />
          <Route path="/main/:page" element={<MainView />} />
          <Route path="/threads" element={<ThreadsView />} />
          <Route path="/threads/*" element={<ThreadsView />} />
          <Route path="/settings" element={<Navigate to="/settings/appearance" replace />} />
          <Route path="/settings/:section" element={<UserSettingsView />} />
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
