import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { MessageSquarePlus, Sparkles, Plug, Trash2 } from 'lucide-react';
import { useStore } from '../stores/appStore';
import './Sidebar.css';

export default function Sidebar({ collapsed, onNewChat }: { collapsed: boolean; onNewChat: () => void }) {
  const navigate = useNavigate();
  const conversations = useStore((s) => s.conversations);
  const activeConversationId = useStore((s) => s.activeConversationId);
  const setActiveConversationId = useStore((s) => s.setActiveConversation);

  if (collapsed) {
    return null;
  }

  function onAgents() {
    navigate('/agents');
  }

  function onConnectors() {
    navigate('/connectors');
  }

  function selectConversation(id: string) {
    navigate('/chat');
    setActiveConversationId(id);
  }

  const historyList = conversations.filter((c) => c.title && c.title !== 'New chat');

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
        <button className="agents-btn" onClick={onConnectors}>
          <Plug size={18} />
          Connectors
        </button>

        <div className="sidebar-section history-section">
          <h3>History</h3>
          <div className="conversation-list history-list">
            {historyList.length === 0 ? (
              <div className="conversation-empty">None</div>
            ) : (
              historyList.map((c) => (
                <ConversationItem
                  key={c.id}
                  conversation={c}
                  isActive={activeConversationId === c.id}
                  onSelect={() => selectConversation(c.id)}
                />
              ))
            )}
          </div>
        </div>
      </div>
    </aside>
  );
}

function ConversationItem({
  conversation,
  isActive,
  onSelect,
}: {
  conversation: { id: string; title: string };
  isActive: boolean;
  onSelect: () => void;
}) {
  const [hovered, setHovered] = useState(false);
  return (
    <button
      className={`conversation-item history ${isActive ? 'selected' : ''}`}
      onClick={onSelect}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <span className="conversation-title">{conversation.title}</span>
      {hovered && (
        <span
          className="conversation-delete"
          title="Delete conversation"
          aria-label="Delete conversation"
          onClick={(e) => {
            e.stopPropagation();
            // Delete not wired; backend has no deleteChat yet.
          }}
        >
          <Trash2 size={14} />
        </span>
      )}
    </button>
  );
}
