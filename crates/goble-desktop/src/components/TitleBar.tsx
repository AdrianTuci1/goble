import { Plus } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import './TitleBar.css';

interface TitleBarProps {
  collapsed: boolean;
  onToggleCollapse: () => void;
}

export default function TitleBar({ collapsed, onToggleCollapse }: TitleBarProps) {
  const navigate = useNavigate();
  return (
    <div className="title-bar">
      <div className="title-bar-left">
        <button className="title-bar-menu-btn" onClick={onToggleCollapse} aria-label={collapsed ? 'Expand' : 'Collapse'}>
          <span className="hamburger" />
        </button>
        <span className="title-bar-title">Goble</span>
      </div>
      <div className="title-bar-actions">
        <button
          className="title-bar-action"
          onClick={() => navigate('/threads')}
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
