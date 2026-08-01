import { useNavigate } from 'react-router-dom';
import { useStore } from '../stores/appStore';
import { agentsData, type Agent } from '../mocks/agentsData';
import './Sidebar.css';

interface SidebarProps {
  collapsed: boolean;
  onNewChat: () => void;
  activeConversationId?: string | null;
  onSelectConversation?: (id: string) => void;
}

export default function Sidebar({ collapsed, onNewChat, activeConversationId, onSelectConversation }: SidebarProps) {
  const navigate = useNavigate();
  const conversations = useStore((s) => s.conversations);

  if (collapsed) {
    return (
      <aside className="sidebar collapsed" aria-label="Sidebar">
        <div className="sidebar-workspaces">
          <button className="workspace active" title="Demo">D</button>
        </div>
        <div className="sidebar-nav">
          <button className="sidebar-icon active" onClick={() => navigate('/chat')} title="Chat">💬</button>
          <button className="sidebar-icon" onClick={() => navigate('/threads')} title="Threads">📥</button>
          <button className="sidebar-icon" onClick={() => navigate('/agents')} title="Agents">🤖</button>
          <button className="sidebar-icon" onClick={() => navigate('/connectors')} title="Connectors">🔌</button>
        </div>
        <div className="sidebar-footer">
          <button className="sidebar-icon" onClick={() => navigate('/settings')} title="Settings">⚙️</button>
        </div>
      </aside>
    );
  }

  return (
    <aside className="sidebar" aria-label="Sidebar">
      <div className="sidebar-header">
        <button className="new-chat-btn" onClick={onNewChat}>
          <span className="new-chat-icon">+</span>
          New chat
        </button>
      </div>

      <div className="sidebar-section">
        <h4 className="sidebar-section-title">Agents</h4>
        <div className="sidebar-list">
          {agentsData.map((agent: Agent) => (
            <button
              key={agent.id}
              className="sidebar-item"
              onClick={() => navigate(`/chat?agent=${agent.id}`)}
              title={agent.description}
            >
              <span className="sidebar-item-dot" style={{ background: agent.color }} />
              <span className="sidebar-item-label">{agent.name}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="sidebar-section">
        <div className="sidebar-section-header">
          <h4 className="sidebar-section-title">Conversations</h4>
          <span className="sidebar-count">{conversations.length}</span>
        </div>
        <div className="sidebar-list">
          {conversations.length === 0 && (
            <div className="sidebar-empty">No conversations yet.</div>
          )}
          {conversations.map((c) => (
            <button
              key={c.id}
              className={`sidebar-item ${activeConversationId === c.id ? 'active' : ''}`}
              onClick={() => onSelectConversation?.(c.id)}
            >
              <span className="sidebar-item-label">{c.title}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="sidebar-spacer" />

      <div className="sidebar-section">
        <div className="sidebar-list">
          <button className="sidebar-item" onClick={() => navigate('/threads')}>
            <span className="sidebar-item-icon">📥</span>
            <span className="sidebar-item-label">Threads</span>
          </button>
          <button className="sidebar-item" onClick={() => navigate('/agents')}>
            <span className="sidebar-item-icon">🤖</span>
            <span className="sidebar-item-label">Agents</span>
          </button>
          <button className="sidebar-item" onClick={() => navigate('/connectors')}>
            <span className="sidebar-item-icon">🔌</span>
            <span className="sidebar-item-label">Connectors</span>
          </button>
          <button className="sidebar-item" onClick={() => navigate('/settings')}>
            <span className="sidebar-item-icon">⚙️</span>
            <span className="sidebar-item-label">Settings</span>
          </button>
        </div>
      </div>
    </aside>
  );
}
