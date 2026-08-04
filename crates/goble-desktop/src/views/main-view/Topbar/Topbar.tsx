import { useNavigate, useLocation } from 'react-router-dom';
import { Menu, Hash, Settings, Plus, X, Minus, Square } from 'lucide-react';
import { useMainViewStore } from '../store/mainViewStore';
import { IconButton, Button } from '../../../ui';
import './Topbar.css';

export default function Topbar() {
  const navigate = useNavigate();
  const location = useLocation();
  const threadsActive = location.pathname.startsWith('/threads');
  const settingsActive = location.pathname.startsWith('/settings');
  const { sidebarCollapsed, toggleSidebar } = useMainViewStore();

  function onThreads() {
    navigate(threadsActive ? '/main/chat' : '/threads');
  }

  function onSettings() {
    navigate('/settings/appearance');
  }

  function onNewChat() {
    window.dispatchEvent(new CustomEvent('goble:new-chat'));
  }

  return (
    <header className="topbar">
      <div className="topbar-left">
        <IconButton label={threadsActive ? 'Back' : sidebarCollapsed ? 'Expand' : 'Collapse'} onClick={threadsActive ? () => navigate('/main/chat') : toggleSidebar}>
          {threadsActive ? <span>←</span> : <Menu size={18} />}
        </IconButton>
        <IconButton label="Threads" onClick={onThreads} className={threadsActive ? 'active' : ''}>
          <Hash size={18} />
        </IconButton>
        <span className="topbar-title">Goble</span>
      </div>
      <div className="topbar-right">
        <Button variant="secondary" size="sm" onClick={onNewChat} className="topbar-new-chat">
          <Plus size={14} /> New chat
        </Button>
        <IconButton label="Settings" onClick={onSettings} className={settingsActive ? 'active' : ''}>
          <Settings size={18} />
        </IconButton>
        <div className="topbar-window-controls" data-tauri-drag-region>
          <button className="topbar-window-btn" aria-label="Minimize"><Minus size={14} /></button>
          <button className="topbar-window-btn" aria-label="Maximize"><Square size={12} /></button>
          <button className="topbar-window-btn close" aria-label="Close"><X size={14} /></button>
        </div>
      </div>
    </header>
  );
}
