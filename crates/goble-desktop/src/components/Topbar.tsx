import { useEffect, useState } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import { Menu, Hash, Settings, X, Minus, Square } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import './Topbar.css';

function isMacOS() {
  if (typeof navigator === 'undefined') return false;
  return /Macintosh|Mac OS X|MacIntel/.test(navigator.userAgent) || navigator.platform === 'MacIntel';
}

interface TopbarProps {
  collapsed: boolean;
  onToggleSidebar: () => void;
}

export default function Topbar({ collapsed, onToggleSidebar }: TopbarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const threadsActive = location.pathname.startsWith('/threads');
  const settingsActive = location.pathname.startsWith('/settings');
  const [mac, setMac] = useState(false);

  useEffect(() => {
    setMac(isMacOS());
  }, []);

  const appWindow = getCurrentWindow();

  function onThreads() {
    navigate(threadsActive ? '/chat' : '/threads');
  }

  function onSettings() {
    navigate('/settings');
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
        <button
          className="topbar-btn"
          aria-label={threadsActive || settingsActive ? 'Back' : collapsed ? 'Expand' : 'Collapse'}
          title={threadsActive || settingsActive ? 'Back' : collapsed ? 'Expand' : 'Collapse'}
          onClick={threadsActive || settingsActive ? () => navigate('/chat') : onToggleSidebar}
        >
          {threadsActive || settingsActive ? <span>←</span> : <Menu size={18} />}
        </button>
        <button
          className={`topbar-btn ${threadsActive ? 'active' : ''}`}
          aria-label="Threads"
          title="Threads"
          onClick={onThreads}
        >
          <Hash size={18} />
        </button>
      </div>
      <div className="topbar-right">
        <button
          className={`topbar-btn ${settingsActive ? 'active' : ''}`}
          aria-label="Settings"
          title="Settings"
          onClick={onSettings}
        >
          <Settings size={18} />
        </button>
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
