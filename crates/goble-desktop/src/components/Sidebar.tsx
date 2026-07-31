import { Link, useLocation } from 'react-router-dom';
import {
  MessageSquare,
  Bot,
  Plug,
} from 'lucide-react';
import './Sidebar.css';
import { useStore } from '../stores/appStore';

const GRADIENTS = [
  'linear-gradient(135deg, #667eea 0%, #764ba2 100%)',
  'linear-gradient(135deg, #ff9a9e 0%, #fecfef 99%, #fecfef 100%)',
  'linear-gradient(120deg, #84fab0 0%, #8fd3f4 100%)',
  'linear-gradient(120deg, #fccb90 0%, #d57eeb 100%)',
  'linear-gradient(120deg, #e0c3fc 0%, #8ec5fc 100%)',
  'linear-gradient(135deg, #f093fb 0%, #f5576c 100%)',
  'linear-gradient(135deg, #4facfe 0%, #00f2fe 100%)',
  'linear-gradient(135deg, #43e97b 0%, #38f9d7 100%)',
];

function gradientFor(id: string) {
  let hash = 0;
  for (let i = 0; i < id.length; i++) {
    hash = id.charCodeAt(i) + ((hash << 5) - hash);
  }
  return GRADIENTS[Math.abs(hash) % GRADIENTS.length];
}

function initials(name: string) {
  return name
    .split(/\s+/)
    .map((w) => w[0])
    .slice(0, 2)
    .join('')
    .toUpperCase();
}

interface SidebarProps {
  collapsed: boolean;
}

export default function Sidebar({ collapsed }: SidebarProps) {
  const location = useLocation();
  const conversations = useStore((s) => s.conversations);
  const agents = useStore((s) => s.agents);
  const activeChatId = useStore((s) => s.activeConversationId);
  const setActiveChatId = useStore((s) => s.setActiveConversation);

  const navItems = [
    { path: '/chat', label: 'Chat', icon: MessageSquare },
    { path: '/agents', label: 'Agents', icon: Bot },
    { path: '/connectors', label: 'Connectors', icon: Plug },
  ];

  return (
    <aside className={`sidebar ${collapsed ? 'collapsed' : ''}`}>
      <nav className="nav">
        {navItems.map((item) => {
          const Icon = item.icon;
          return (
            <Link
              key={item.path}
              to={item.path}
              className={`nav-item ${location.pathname === item.path ? 'active' : ''}`}
              title={item.label}
            >
              <Icon size={18} className="nav-icon" />
              {!collapsed && <span className="nav-label">{item.label}</span>}
            </Link>
          );
        })}
      </nav>

      <div className="sidebar-section">
        <div className="section-title">{collapsed ? 'Chats' : 'Conversations'}</div>
        <div className="conversation-list">
          {conversations.map((c) => (
            <button
              key={c.id}
              className={`conversation-item ${activeChatId === c.id ? 'active' : ''}`}
              onClick={() => setActiveChatId(c.id)}
              title={c.title}
            >
              {!collapsed && <span className="conversation-title">{c.title}</span>}
              {collapsed && <span className="conversation-dot" />}
            </button>
          ))}
        </div>
      </div>

      <div className="sidebar-section compact">
        <div className="section-title">{collapsed ? 'Bots' : 'Agents'}</div>
        <div className="agent-list">
          {agents.map((a) => (
            <Link
              key={a.id}
              to="/agents"
              className="agent-item"
              title={a.name}
            >
              <span
                className="agent-avatar"
                style={{ background: gradientFor(a.id) }}
              >
                {initials(a.name)}
              </span>
              {!collapsed && <span className="agent-name">{a.name}</span>}
            </Link>
          ))}
        </div>
      </div>
    </aside>
  );
}
