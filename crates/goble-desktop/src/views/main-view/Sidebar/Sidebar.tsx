import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useMainViewStore } from '../store/mainViewStore';
import { MessageSquarePlus, Sparkles, Trash2 } from 'lucide-react';
import { useChatStore } from '../Content/chat-window/store/chatStore';
import { deleteChat } from '../../../shared';
import './Sidebar.css';

export default function Sidebar() {
  const navigate = useNavigate();
  const { sidebarCollapsed, activeConversations, pastConversations, setPage } = useMainViewStore();
  const { activeConversationId, setActiveConversationId, deleteConversation } = useChatStore();

  function onNewChat() {
    setPage('chat');
    navigate('/main/chat');
    window.dispatchEvent(new CustomEvent('goble:new-chat'));
  }

  function onAgents() {
    setPage('agents');
    navigate('/main/agents');
  }

  function selectConversation(id: string) {
    setPage('chat');
    navigate('/main/chat');
    setActiveConversationId(id);
  }

  async function handleDeleteConversation(e: React.MouseEvent, id: string) {
    e.stopPropagation();
    try {
      await deleteChat(id);
      deleteConversation(id);
    } catch (err) {
      console.error('Failed to delete conversation', err);
    }
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
                <ConversationItem
                  key={c.id}
                  conversation={c}
                  isActive={activeConversationId === c.id}
                  isPast={false}
                  onSelect={() => selectConversation(c.id)}
                  onDelete={(e) => handleDeleteConversation(e, c.id)}
                />
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
                <ConversationItem
                  key={c.id}
                  conversation={c}
                  isActive={activeConversationId === c.id}
                  isPast={true}
                  onSelect={() => selectConversation(c.id)}
                  onDelete={(e) => handleDeleteConversation(e, c.id)}
                />
              ))
            )}
          </div>
        </div>
      </div>
    </aside>
  );
}

interface ConversationItemProps {
  conversation: { id: string; title: string };
  isActive: boolean;
  isPast: boolean;
  onSelect: () => void;
  onDelete: (e: React.MouseEvent) => void;
}

function ConversationItem({ conversation, isActive, isPast, onSelect, onDelete }: ConversationItemProps) {
  const [hovered, setHovered] = useState(false);
  return (
    <button
      className={`conversation-item ${isPast ? 'past' : ''} ${isActive ? 'selected' : ''}`}
      onClick={onSelect}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <span className="conversation-title">{conversation.title}</span>
      {hovered && (
        <span
          className="conversation-delete"
          onClick={onDelete}
          title="Delete conversation"
          aria-label="Delete conversation"
        >
          <Trash2 size={14} />
        </span>
      )}
    </button>
  );
}
