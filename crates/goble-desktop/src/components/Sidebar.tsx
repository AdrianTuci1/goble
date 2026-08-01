import { useNavigate } from 'react-router-dom';
import { Plus, Bot } from 'lucide-react';
import './Sidebar.css';
import { useStore, type Conversation } from '../stores/appStore';

interface SidebarProps {
  collapsed: boolean;
  onNewChat: () => void;
}

export default function Sidebar({ collapsed, onNewChat }: SidebarProps) {
  const navigate = useNavigate();
  const conversations = useStore((s) => s.conversations);
  const activeId = useStore((s) => s.activeConversationId);
  const setActive = useStore((s) => s.setActiveConversation);

  // Split active/past by updated_at within last 24h
  const now = Date.now();
  const activeConversations: Conversation[] = [];
  const pastConversations: Conversation[] = [];
  conversations.forEach((c) => {
    const updated = new Date(c.updated_at || 0).getTime();
    if (now - updated < 24 * 60 * 60 * 1000) {
      activeConversations.push(c);
    } else {
      pastConversations.push(c);
    }
  });

  return (
    <aside className={`sidebar ${collapsed ? 'collapsed' : ''}`}>
      <div className="sidebar-expanded-content">
        <div className="sidebar-header">
          <div className="sidebar-brand">Goble</div>
        </div>
        <button className="new-chat-btn" onClick={onNewChat}>
          <span className="btn-icon">
            <Plus size={18} />
          </span>
          <span>New chat</span>
        </button>
        <button className="agents-btn" onClick={() => navigate('/agents')}>
          <span className="btn-icon">
            <Bot size={18} />
          </span>
          <span>Agents</span>
        </button>
        <div className="sidebar-section active-section">
          <h3>Active</h3>
          <div className="conversation-list">
            {activeConversations.length === 0 ? (
              <div className="conversation-empty">None</div>
            ) : (
              activeConversations.map((c) => (
                <div
                  key={c.id}
                  className={`conversation-item ${activeId === c.id ? 'selected' : ''}`}
                  onClick={() => setActive(c.id)}
                  title={c.title}
                >
                  {c.title}
                </div>
              ))
            )}
          </div>
        </div>
        <div className="sidebar-section past-section">
          <h3>Past</h3>
          <div className="conversation-list">
            {pastConversations.length === 0 ? (
              <div className="conversation-empty">None</div>
            ) : (
              pastConversations.map((c) => (
                <div
                  key={c.id}
                  className={`conversation-item ${activeId === c.id ? 'selected' : ''}`}
                  onClick={() => setActive(c.id)}
                  title={c.title}
                >
                  {c.title}
                </div>
              ))
            )}
          </div>
        </div>
      </div>

      <div className="sidebar-collapsed-content">
        <button className="collapsed-btn new-chat-collapsed" title="New chat" onClick={onNewChat}>
          <Plus size={18} />
        </button>
        <button className="collapsed-btn agents-collapsed" title="Agents" onClick={() => navigate('/agents')}>
          <Bot size={18} />
        </button>
      </div>
    </aside>
  );
}
