import { useNavigate, useLocation } from 'react-router-dom';
import { useEffect, useState } from 'react';
import { Menu, Hash, Settings, X, Minus, Square } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useMainViewStore } from '../store/mainViewStore';
import { IconButton } from '../../../ui';
import './Topbar.css';

function isMacOS() {
  if (typeof navigator === 'undefined') return false;
  return /Macintosh|Mac OS X|MacIntel/.test(navigator.userAgent) || navigator.platform === 'MacIntel';
}

export default function Topbar() {
  const navigate = useNavigate();
  const location = useLocation();
  const threadsActive = location.pathname.startsWith('/threads');
  const settingsActive = location.pathname.startsWith('/settings');
  const { sidebarCollapsed, toggleSidebar } = useMainViewStore();
  const [mac, setMac] = useState(false);

  useEffect(() => {
    setMac(isMacOS());
  }, []);

  const appWindow = getCurrentWindow();

  function onThreads() {
    navigate(threadsActive ? '/main/chat' : '/threads');
  }

  function onSettings() {
    navigate('/settings/appearance');
  }

  function onMinimize() {
    appWindow.minimize();
  }

  function onToggleMaximize() {
    appWindow.toggleMaximize();
  }

  function onClose() {
    appWindow.close();
  }

  function handleMouseDown(e: React.MouseEvent<HTMLElement>) {
    const target = e.target as HTMLElement;
    if (target.closest('button, a, input, textarea, select')) return;
    if (e.detail === 2) {
      appWindow.toggleMaximize();
    } else if (e.buttons === 1) {
      appWindow.startDragging();
    }
  }

  return (
    <header className={`topbar ${mac ? 'platform-macos' : ''}`} onMouseDown={handleMouseDown}>
      <div className="topbar-left">
        <IconButton label={threadsActive || settingsActive ? 'Back' : sidebarCollapsed ? 'Expand' : 'Collapse'} onClick={threadsActive || settingsActive ? () => navigate('/main/chat') : toggleSidebar}>
          {threadsActive || settingsActive ? <span>←</span> : <Menu size={18} />}
        </IconButton>
        <IconButton label="Threads" onClick={onThreads} className={threadsActive ? 'active' : ''}>
          <Hash size={18} />
        </IconButton>
      </div>
      <div className="topbar-right">
        <IconButton label="Settings" onClick={onSettings} className={settingsActive ? 'active' : ''}>
          <Settings size={18} />
        </IconButton>
        {!mac && (
          <div className="topbar-window-controls">
            <button className="topbar-window-btn" aria-label="Minimize" onClick={onMinimize}><Minus size={14} /></button>
            <button className="topbar-window-btn" aria-label="Maximize" onClick={onToggleMaximize}><Square size={12} /></button>
            <button className="topbar-window-btn close" aria-label="Close" onClick={onClose}><X size={14} /></button>
          </div>
        )}
      </div>
    </header>
  );
}
