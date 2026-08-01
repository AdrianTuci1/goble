import { useNavigate, useLocation } from 'react-router-dom';
import { Hash, Settings, PanelLeft } from 'lucide-react';
import './TitleBar.css';

interface TitleBarProps {
  collapsed: boolean;
  onToggleCollapse: () => void;
  threadsActive?: boolean;
  onToggleThreads?: () => void;
}

export default function TitleBar({ collapsed, onToggleCollapse, threadsActive, onToggleThreads }: TitleBarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  void collapsed; // reserved for collapsed state indicator

  const showThreads = location.pathname === '/chat' || location.pathname === '/threads';

  return (
    <div className="title-bar">
      <div className="title-bar-traffic-spacer" />
      <div className="topbar-left">
        <button
          className="topbar-btn sidebar-toggle"
          title="Toggle sidebar"
          onClick={onToggleCollapse}
        >
          <PanelLeft size={16} />
        </button>
        {showThreads && onToggleThreads && (
          <button
            className={`topbar-btn threads-btn ${threadsActive ? 'active' : ''}`}
            title="Threads"
            onClick={onToggleThreads}
          >
            <Hash size={16} />
          </button>
        )}
      </div>
      <div className="topbar-right">
        <button
          className={`topbar-btn settings-btn ${location.pathname === '/settings' ? 'active' : ''}`}
          title="Settings"
          onClick={() => navigate('/settings')}
        >
          <Settings size={16} />
        </button>
      </div>
    </div>
  );
}
