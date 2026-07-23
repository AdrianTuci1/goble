import { useState } from 'react';
import { useLocation } from 'react-router-dom';
import { useStore } from '../stores/appStore';

export default function Sidebar() {
  const location = useLocation();
  const currentPath = location.pathname;
  const [isCollapsed, setIsCollapsed] = useState(false);
  const conversations = useStore((s) => s.conversations);
  const activeId = useStore((s) => s.activeConversationId);
  const setActiveConversation = useStore((s) => s.setActiveConversation);
  const setSettingsOpen = useStore((s) => s.setSettingsOpen);

  return (
    <div className={`gemini-sidebar ${isCollapsed ? 'collapsed' : ''}`}>
      <div className="gemini-sb-header">
        {!isCollapsed && (
          <div className="gemini-logo-container">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
              <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="2"/>
              <circle cx="12" cy="12" r="4" fill="currentColor"/>
            </svg>
            <span>Goble</span>
          </div>
        )}
        <button className="gemini-toggle-btn" onClick={() => setIsCollapsed(!isCollapsed)}>
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="3" y="3" width="18" height="18" rx="2"/>
            <line x1="9" y1="3" x2="9" y2="21"/>
          </svg>
        </button>
      </div>

      <div className="gemini-sb-nav">
        <div
          className={`gemini-nav-item ${currentPath === '/chat' ? 'active' : ''}`}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
          </svg>
          {!isCollapsed && <span>Chat</span>}
        </div>

        <div
          className={`gemini-nav-item ${currentPath === '/workflows' ? 'active' : ''}`}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="3" y="4" width="6" height="6" rx="1"/>
            <rect x="15" y="4" width="6" height="6" rx="1"/>
            <rect x="9" y="14" width="6" height="6" rx="1"/>
            <path d="M6 10v2a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2v-2"/>
          </svg>
          {!isCollapsed && <span>Workflows</span>}
        </div>

        <div
          className={`gemini-nav-item ${currentPath === '/knowledge' ? 'active' : ''}`}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20"/>
            <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z"/>
          </svg>
          {!isCollapsed && <span>Knowledge</span>}
        </div>

        <div
          className={`gemini-nav-item ${currentPath === '/connectors' ? 'active' : ''}`}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="5" r="3"/>
            <circle cx="5" cy="19" r="3"/>
            <circle cx="19" cy="19" r="3"/>
            <line x1="8.5" y1="7.5" x2="12" y2="12"/>
            <line x1="15.5" y1="7.5" x2="12" y2="12"/>
            <line x1="12" y1="12" x2="5" y2="16.5"/>
            <line x1="12" y1="12" x2="19" y2="16.5"/>
          </svg>
          {!isCollapsed && <span>Connectors</span>}
        </div>
      </div>

      {!isCollapsed && <div className="gemini-sb-section-label">Recent chats</div>}
      <div className="gemini-sb-recents">
        {conversations.map((conv) => (
          <div
            key={conv.id}
            className={`gemini-recent-item ${conv.id === activeId ? 'active' : ''}`}
            onClick={() => setActiveConversation(conv.id)}
          >
            {conv.title}
          </div>
        ))}
      </div>

      <div className="gemini-sb-footer">
        <div className="gemini-profile-block">
          <div className="gemini-avatar">
            <svg viewBox="0 0 32 32" fill="none">
              <circle cx="16" cy="16" r="16" fill="#525252"/>
              <circle cx="16" cy="12" r="5" fill="#0a0a0a"/>
              <path d="M16 19c-5 0-9 3.5-9 8 0 1.5 1 2 2.5 2h13c1.5 0 2.5-.5 2.5-2 0-4.5-4-8-9-8z" fill="#0a0a0a"/>
            </svg>
          </div>
          {!isCollapsed && (
            <>
              <div className="gemini-profile-info">
                <div className="gemini-profile-name">Local User</div>
                <div className="gemini-profile-tier">Desktop</div>
              </div>
              <button className="gemini-settings-btn" onClick={() => setSettingsOpen(true)}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <circle cx="12" cy="12" r="3"/>
                  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
                </svg>
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
