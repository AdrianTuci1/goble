import { useNavigate } from 'react-router-dom';
import { useMainViewStore } from '../store/mainViewStore';
import { MessageSquarePlus, Sparkles } from 'lucide-react';
import { useChatStore } from '../Content/chat-window/store/chatStore';
import './Sidebar.css';

export default function Sidebar() {
  const navigate = useNavigate();
  const { sidebarCollapsed, activeConversations, pastConversations, setPage } = useMainViewStore();
  const { activeConversationId, setActiveConversationId } = useChatStore();

  function onNewChat() {
    window.dispatchEvent(new CustomEvent('goble:new-chat'));
  }

  function onAgents() {
    setPage('agents');
    navigate('/main/agents');
  }

  function selectConversation(id: string) {
    setActiveConversationId(id);
  }

  if (sidebarCollapsed) {
    return null;
  }

  return (
    <aside className="main-sidebar" aria-label="Sidebar">
      <div className="sidebar-expanded-content">
        <div className="sidebar-header" />
        <button className="new-chat-btn" onClick={onNewChat}>
          <MessageSquarePlus size={18} />
          New chat
        </button>
        <button className="agents-btn" onClick={onAgents}>
          <Sparkles size={18} />
          Agents
        </button>

        <div className="sidebar-section active-section">
          <h3>Active</h3>
          <div className="conversation-list active-list">
            {activeConversations.length === 0 ? (
              <div className="conversation-empty">None</div>
            ) : (
              activeConversations.map((c) => (
                <button
                  key={c.id}
                  className={`conversation-item ${activeConversationId === c.id ? 'selected' : ''}`}
                  onClick={() => selectConversation(c.id)}
                >
                  {c.title}
                </button>
              ))
            )}
          </div>
        </div>

        <div className="sidebar-section past-section">
          <h3>Past</h3>
          <div className="conversation-list past-list">
            {pastConversations.length === 0 ? (
              <div className="conversation-empty">None</div>
            ) : (
              pastConversations.map((c) => (
                <button
                  key={c.id}
                  className={`conversation-item past ${activeConversationId === c.id ? 'selected' : ''}`}
                  onClick={() => selectConversation(c.id)}
                >
                  {c.title}
                </button>
              ))
            )}
          </div>
        </div>
      </div>
    </aside>
  );
}
