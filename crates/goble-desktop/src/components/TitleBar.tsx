import { useNavigate } from 'react-router-dom';
import { PanelLeft, Settings } from 'lucide-react';
import './TitleBar.css';

interface TitleBarProps {
  collapsed: boolean;
  onToggleCollapse: () => void;
}

export default function TitleBar({ collapsed, onToggleCollapse }: TitleBarProps) {
  const navigate = useNavigate();

  return (
    <div className="title-bar">
      <div className="title-bar-traffic-spacer" />
      <div className="title-bar-spacer" />
      <button
        className="title-bar-button"
        onClick={onToggleCollapse}
        aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      >
        <PanelLeft size={16} />
      </button>
      <button
        className="title-bar-button"
        onClick={() => navigate('/settings')}
        aria-label="Settings"
      >
        <Settings size={16} />
      </button>
    </div>
  );
}
