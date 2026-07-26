import { useLocation, useNavigate } from 'react-router-dom';
import { useStore } from '../stores/appStore';
import { pingWorker } from '../tauri/api';

export default function Sidebar() {
  const location = useLocation();
  const navigate = useNavigate();
  const workers = useStore((s) => s.workers);
  const conversations = useStore((s) => s.conversations);
  const activeChatId = useStore((s) => s.activeConversationId);
  const setActiveChatId = useStore((s) => s.setActiveConversation);

  const navItems = [
    { path: '/chat', label: 'Chat', icon: '💬' },
    { path: '/agents', label: 'Agents', icon: '🤖' },
    { path: '/workflows', label: 'Workflows', icon: '⚡' },
    { path: '/teams', label: 'Teams', icon: '👥' },
    { path: '/knowledge', label: 'Knowledge', icon: '📚' },
    { path: '/connectors', label: 'Connectors', icon: '🔌' },
    { path: '/executions', label: 'Executions', icon: '▶️' },
    { path: '/vault', label: 'Vault', icon: '🔐' },
    { path: '/search', label: 'Search', icon: '🔍' },
  ];

  return (
    <aside className="sidebar">
      <div className="logo">Goble</div>
      <nav className="nav">
        {navItems.map((item) => (
          <a
            key={item.path}
            href={item.path}
            className={`nav-item ${location.pathname === item.path ? 'active' : ''}`}
          >
            <span className="nav-icon">{item.icon}</span>
            <span className="nav-label">{item.label}</span>
          </a>
        ))}
      </nav>

      <div className="sidebar-section">
        <div className="section-title">Conversations</div>
        <div className="conversation-list">
          {conversations.map((c) => (
            <button
              key={c.id}
              className={`conversation-item ${activeChatId === c.id ? 'active' : ''}`}
              onClick={() => setActiveChatId(c.id)}
              title={c.model ? `${c.provider || 'provider'} / ${c.model}` : undefined}
            >
              {c.title}
              {c.model && <span className="conversation-model">{c.provider} / {c.model}</span>}
            </button>
          ))}
        </div>
      </div>

      <div className="sidebar-section compact">
        <div className="section-title">Workers</div>
        <div className="worker-list">
          {workers.slice(0, 3).map((w) => (
            <div key={w.id} className={`worker-item ${w.paired ? 'paired' : ''}`}>
              <div className="worker-name">{w.name}</div>
              <div className="worker-meta">{w.paired ? 'paired' : 'unpaired'}</div>
              {w.paired && <button onClick={() => pingWorker(w.id)}>Ping</button>}
            </div>
          ))}
          {workers.length > 3 && (
            <div className="worker-meta">+{workers.length - 3} more</div>
          )}
        </div>
      </div>

      <div className="sidebar-footer">
        <button className="settings-button" onClick={() => navigate('/settings')}>
          Settings
        </button>
      </div>
    </aside>
  );
}
