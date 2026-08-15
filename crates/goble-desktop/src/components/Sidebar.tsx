import { useNavigate, useLocation } from 'react-router-dom';
import { useStore } from '../stores/appStore';
import type { AgentInfo } from '../tauri/api';
import './Sidebar.css';

interface SidebarProps {
  collapsed: boolean;
  onNewChat: () => void;
  activeConversationId?: string | null;
  onSelectConversation?: (id: string) => void;
}

export default function Sidebar({ collapsed, onNewChat, activeConversationId, onSelectConversation }: SidebarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const conversations = useStore((s) => s.conversations);
  const agents = useStore((s) => s.agents);
  const threads = useStore((s) => s.threads);
  const threadMessages = useStore((s) => s.threadMessages);

  const unreadThreadCount = threads.reduce((sum, t) => {
    const messages = threadMessages[t.id] || [];
    if (!messages.length) return sum;
    const lastRead = t.last_read_at ? new Date(t.last_read_at) : new Date(0);
    return sum + messages.filter((m) => new Date(m.created_at) > lastRead).length;
  }, 0);

  if (collapsed) {
    return (
      <aside className="sidebar collapsed" aria-label="Sidebar">
        <div className="sidebar-workspaces">
          <button className="workspace active" title="Demo">D</button>
        </div>
        <div className="sidebar-nav">
          <button className={`sidebar-icon ${location.pathname === '/chat' ? 'active' : ''}`} onClick={() => navigate('/chat')} title="Chat">💬</button>
          <button className={`sidebar-icon ${location.pathname.startsWith('/threads') ? 'active' : ''}`} onClick={() => navigate('/threads')} title="Threads">
            📥
            {unreadThreadCount > 0 && <span className="sidebar-badge">{unreadThreadCount}</span>}
          </button>
          <button className={`sidebar-icon ${location.pathname === '/agents' ? 'active' : ''}`} onClick={() => navigate('/agents')} title="Agents">🤖</button>
          <button className={`sidebar-icon ${location.pathname === '/connectors' ? 'active' : ''}`} onClick={() => navigate('/connectors')} title="Connectors">🔌</button>
          <button className={`sidebar-icon ${location.pathname === '/workflows' ? 'active' : ''}`} onClick={() => navigate('/workflows')} title="Workflows">⚡</button>
          <button className={`sidebar-icon ${location.pathname === '/teams' ? 'active' : ''}`} onClick={() => navigate('/teams')} title="Teams">👥</button>
          <button className={`sidebar-icon ${location.pathname === '/vault' ? 'active' : ''}`} onClick={() => navigate('/vault')} title="Vault">🔒</button>
          <button className={`sidebar-icon ${location.pathname === '/executions' ? 'active' : ''}`} onClick={() => navigate('/executions')} title="Executions">▶️</button>
          <button className={`sidebar-icon ${location.pathname === '/knowledge' ? 'active' : ''}`} onClick={() => navigate('/knowledge')} title="Knowledge">📚</button>
          <button className={`sidebar-icon ${location.pathname === '/search' ? 'active' : ''}`} onClick={() => navigate('/search')} title="Search">🔎</button>
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
          {agents.length === 0 && <div className="sidebar-empty">No agents yet.</div>}
          {agents.map((agent: AgentInfo) => (
            <button
              key={agent.id}
              className="sidebar-item"
              onClick={() => navigate(`/chat?agent=${agent.id}`)}
              title={agent.spec.description || agent.spec.prompt}
            >
              <span className="sidebar-item-dot" style={{ background: '#2563eb' }} />
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
          <button className={`sidebar-item ${location.pathname.startsWith('/threads') ? 'active' : ''}`} onClick={() => navigate('/threads')}>
            <span className="sidebar-item-icon">📥</span>
            <span className="sidebar-item-label">Threads</span>
            {unreadThreadCount > 0 && <span className="sidebar-item-badge">{unreadThreadCount}</span>}
          </button>
          <button className={`sidebar-item ${location.pathname === '/agents' ? 'active' : ''}`} onClick={() => navigate('/agents')}>
            <span className="sidebar-item-icon">🤖</span>
            <span className="sidebar-item-label">Agents</span>
          </button>
          <button className={`sidebar-item ${location.pathname === '/connectors' ? 'active' : ''}`} onClick={() => navigate('/connectors')}>
            <span className="sidebar-item-icon">🔌</span>
            <span className="sidebar-item-label">Connectors</span>
          </button>
          <button className={`sidebar-item ${location.pathname === '/workflows' ? 'active' : ''}`} onClick={() => navigate('/workflows')}>
            <span className="sidebar-item-icon">⚡</span>
            <span className="sidebar-item-label">Workflows</span>
          </button>
          <button className={`sidebar-item ${location.pathname === '/teams' ? 'active' : ''}`} onClick={() => navigate('/teams')}>
            <span className="sidebar-item-icon">👥</span>
            <span className="sidebar-item-label">Teams</span>
          </button>
          <button className={`sidebar-item ${location.pathname === '/vault' ? 'active' : ''}`} onClick={() => navigate('/vault')}>
            <span className="sidebar-item-icon">🔒</span>
            <span className="sidebar-item-label">Vault</span>
          </button>
          <button className={`sidebar-item ${location.pathname === '/executions' ? 'active' : ''}`} onClick={() => navigate('/executions')}>
            <span className="sidebar-item-icon">▶️</span>
            <span className="sidebar-item-label">Executions</span>
          </button>
          <button className={`sidebar-item ${location.pathname === '/knowledge' ? 'active' : ''}`} onClick={() => navigate('/knowledge')}>
            <span className="sidebar-item-icon">📚</span>
            <span className="sidebar-item-label">Knowledge</span>
          </button>
          <button className={`sidebar-item ${location.pathname === '/search' ? 'active' : ''}`} onClick={() => navigate('/search')}>
            <span className="sidebar-item-icon">🔎</span>
            <span className="sidebar-item-label">Search</span>
          </button>
          <button className={`sidebar-item ${location.pathname === '/settings' ? 'active' : ''}`} onClick={() => navigate('/settings')}>
            <span className="sidebar-item-icon">⚙️</span>
            <span className="sidebar-item-label">Settings</span>
          </button>
        </div>
      </div>
    </aside>
  );
}
