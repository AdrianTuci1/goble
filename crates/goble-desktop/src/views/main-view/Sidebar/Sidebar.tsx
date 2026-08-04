import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useMainViewStore } from '../store/mainViewStore';
import { MessageSquarePlus, Sparkles, Trash2 } from 'lucide-react';
import { useChatStore, type AppChatMessage } from '../Content/chat-window/store/chatStore';
import { deleteChat } from '../../../shared';
import './Sidebar.css';

function hasPendingActions(messages: AppChatMessage[] | undefined): boolean {
  if (!messages) return false;
  return messages.some((m) => {
    if (['confirmationCard', 'formCard', 'variantCard', 'secretCard'].includes(m.kind || '')) return true;
    if (m.kind === 'actionList' && m.items?.some((item) => (item as any).status === 'pending')) return true;
    return false;
  });
}

export default function Sidebar() {
  const navigate = useNavigate();
  const { sidebarCollapsed, historyConversations, setPage } = useMainViewStore();
  const { activeConversationId, setActiveConversationId, deleteConversation, messagesByChat } = useChatStore();

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

  const pendingConversations = historyConversations.filter((c) => hasPendingActions(messagesByChat[c.id]));
  const historyList = historyConversations.filter((c) => {
    if (pendingConversations.some((p) => p.id === c.id)) return false;
    const msgs = messagesByChat[c.id];
    return msgs && msgs.length > 0;
  });

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

        {pendingConversations.length > 0 && (
          <div className="sidebar-section pending-section">
            <h3>Pending</h3>
            <div className="conversation-list pending-list">
              {pendingConversations.map((c) => (
                <ConversationItem
                  key={c.id}
                  conversation={c}
                  isActive={activeConversationId === c.id}
                  isHistory={false}
                  onSelect={() => selectConversation(c.id)}
                  onDelete={(e) => handleDeleteConversation(e, c.id)}
                />
              ))}
            </div>
          </div>
        )}

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
                  isHistory={true}
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
  isHistory: boolean;
  onSelect: () => void;
  onDelete: (e: React.MouseEvent) => void;
}

function ConversationItem({ conversation, isActive, isHistory, onSelect, onDelete }: ConversationItemProps) {
  const [hovered, setHovered] = useState(false);
  return (
    <button
      className={`conversation-item ${isHistory ? 'history' : 'pending'} ${isActive ? 'selected' : ''}`}
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
