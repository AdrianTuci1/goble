import { useNavigate } from 'react-router-dom';
import { useMainViewStore, type MainPage } from '../store/mainViewStore';
import { agentsData, type Agent } from '../data/agentsData';
import { Plus, MessageSquare, Bot, Plug, Workflow, ListTodo, BookOpen, Search, Users, KeyRound, PanelLeft, PanelRight } from 'lucide-react';
import { IconButton } from '../../../ui';
import './Sidebar.css';

const pages: { id: MainPage; label: string; icon: React.ReactNode }[] = [
  { id: 'chat', label: 'Chat', icon: <MessageSquare size={18} /> },
  { id: 'agents', label: 'Agents', icon: <Bot size={18} /> },
  { id: 'connectors', label: 'Connectors', icon: <Plug size={18} /> },
  { id: 'workflows', label: 'Workflows', icon: <Workflow size={18} /> },
  { id: 'executions', label: 'Executions', icon: <ListTodo size={18} /> },
  { id: 'knowledge', label: 'Knowledge', icon: <BookOpen size={18} /> },
  { id: 'search', label: 'Search', icon: <Search size={18} /> },
  { id: 'teams', label: 'Teams', icon: <Users size={18} /> },
  { id: 'vault', label: 'Vault', icon: <KeyRound size={18} /> },
];

interface SidebarProps {
  activeConversationId?: string | null;
  onSelectConversation?: (id: string) => void;
  onNewChat?: () => void;
}

export default function Sidebar({ activeConversationId, onSelectConversation, onNewChat }: SidebarProps) {
  const navigate = useNavigate();
  const { page, setPage, sidebarCollapsed, toggleSidebar, activeConversations, pastConversations } = useMainViewStore();

  function navigateTo(p: MainPage) {
    setPage(p);
    navigate(`/main/${p}`);
  }

  if (sidebarCollapsed) {
    return (
      <aside className="main-sidebar collapsed" aria-label="Sidebar">
        <div className="main-sidebar-rail">
          <IconButton label="New chat" onClick={onNewChat} className="new-chat-collapsed">
            <Plus size={18} />
          </IconButton>
          {pages.map((p) => (
            <IconButton key={p.id} label={p.label} className={page === p.id ? 'active' : ''} onClick={() => navigateTo(p.id)}>
              {p.icon}
            </IconButton>
          ))}
        </div>
        <div className="main-sidebar-rail-bottom">
          <IconButton label="Expand" onClick={toggleSidebar}>
            <PanelLeft size={18} />
          </IconButton>
        </div>
      </aside>
    );
  }

  return (
    <aside className="main-sidebar" aria-label="Sidebar">
      <div className="main-sidebar-header">
        <button className="new-chat-btn" onClick={onNewChat}>
          <Plus size={18} />
          New chat
        </button>
      </div>

      <div className="main-sidebar-section">
        <h4 className="main-sidebar-section-title">Agents</h4>
        <div className="main-sidebar-list">
          {agentsData.map((agent: Agent) => (
            <button
              key={agent.id}
              className="main-sidebar-item"
              onClick={() => navigate(`/main/chat?agent=${agent.id}`)}
              title={agent.description}
            >
              <span className="main-sidebar-item-dot" style={{ background: agent.color }} />
              <span className="main-sidebar-item-label">{agent.name}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="main-sidebar-section">
        <div className="main-sidebar-section-header">
          <h4 className="main-sidebar-section-title">Active</h4>
        </div>
        <div className="main-sidebar-list">
          {activeConversations.length === 0 && <div className="main-sidebar-empty">No active chats.</div>}
          {activeConversations.map((c) => (
            <button
              key={c.id}
              className={`main-sidebar-item ${activeConversationId === c.id ? 'selected' : ''}`}
              onClick={() => onSelectConversation?.(c.id)}
            >
              <span className="main-sidebar-item-label">{c.title}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="main-sidebar-section">
        <div className="main-sidebar-section-header">
          <h4 className="main-sidebar-section-title">Past</h4>
        </div>
        <div className="main-sidebar-list">
          {pastConversations.length === 0 && <div className="main-sidebar-empty">No past chats.</div>}
          {pastConversations.map((c) => (
            <button
              key={c.id}
              className={`main-sidebar-item past ${activeConversationId === c.id ? 'selected' : ''}`}
              onClick={() => onSelectConversation?.(c.id)}
            >
              <span className="main-sidebar-item-label">{c.title}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="main-sidebar-spacer" />

      <div className="main-sidebar-section">
        <div className="main-sidebar-list">
          {pages.map((p) => (
            <button
              key={p.id}
              className={`main-sidebar-item ${page === p.id ? 'selected' : ''}`}
              onClick={() => navigateTo(p.id)}
            >
              <span className="main-sidebar-item-icon">{p.icon}</span>
              <span className="main-sidebar-item-label">{p.label}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="main-sidebar-footer">
        <button className="main-sidebar-collapse" onClick={toggleSidebar}>
          <PanelRight size={18} />
          <span>Collapse</span>
        </button>
      </div>
    </aside>
  );
}
