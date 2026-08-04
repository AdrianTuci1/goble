import { Plus } from 'lucide-react';
import { useNavigate, useLocation } from 'react-router-dom';
import './TitleBar.css';

interface TitleBarProps {
  collapsed: boolean;
  onToggleCollapse: () => void;
}

export default function TitleBar({ collapsed, onToggleCollapse }: TitleBarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const threadsActive = location.pathname.startsWith('/threads');

  function toggleThreads() {
    navigate(threadsActive ? '/chat' : '/threads');
  }

  return (
    <div className="title-bar">
      <div className="title-bar-left">
        {threadsActive ? (
          <button className="title-bar-menu-btn" onClick={() => navigate('/chat')} aria-label="Back to chat">
            ←
          </button>
        ) : (
          <button className="title-bar-menu-btn" onClick={onToggleCollapse} aria-label={collapsed ? 'Expand' : 'Collapse'}>
            <span className="hamburger" />
          </button>
        )}
        <span className="title-bar-title">Goble</span>
      </div>
      <div className="title-bar-actions">
        <button
          className={`title-bar-action ${threadsActive ? 'active' : ''}`}
          onClick={toggleThreads}
          title="Threads"
        >
          Threads
        </button>
        <button className="title-bar-new-chat" onClick={() => window.dispatchEvent(new CustomEvent('goble:new-chat'))}>
          <Plus size={14} />
          New chat
        </button>
      </div>
    </div>
  );
}
