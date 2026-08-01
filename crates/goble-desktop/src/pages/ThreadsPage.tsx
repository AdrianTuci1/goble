import { useEffect, useRef, useState } from 'react';
import { useStore, type ChatMessage } from '../stores/appStore';
import './ThreadsPage.css';

interface ThreadChannel {
  id: string;
  name: string;
  private: boolean;
  unread: number;
}

interface ThreadWorkspace {
  id: string;
  name: string;
  color: string;
  channels: ThreadChannel[];
  directMessages: { id: string; name: string; unread: number }[];
  projects: { id: string; name: string; groups: { id: string; name: string; channels: ThreadChannel[] }[] }[];
}

const AVATAR_COLORS: Record<string, string> = {
  'You': '#22c55e',
  'Assistant': '#2563eb',
  'System': '#9ca3af',
};

function avatarColor(author: string) {
  if (AVATAR_COLORS[author]) return AVATAR_COLORS[author];
  let hash = 0;
  for (let i = 0; i < author.length; i++) {
    hash = author.charCodeAt(i) + ((hash << 5) - hash);
  }
  const hue = Math.abs(hash) % 360;
  return `hsl(${hue}, 60%, 45%)`;
}

function avatarInitials(name: string) {
  return name.split(' ').map((n) => n[0]).join('').slice(0, 2).toUpperCase();
}

function formatTime(iso: string) {
  try {
    return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  } catch {
    return iso;
  }
}

export default function ThreadsPage() {
  const messagesByChat = useStore((s) => s.messages);
  const agents = useStore((s) => s.agents);
  const activeConversationId = useStore((s) => s.activeConversationId);
  const setActiveConversation = useStore((s) => s.setActiveConversation);
  const addMessage = useStore((s) => s.addMessage);

  const [workspaces] = useState<ThreadWorkspace[]>(() => {
    const ws: ThreadWorkspace = {
      id: 'default',
      name: 'Main',
      color: '#2563eb',
      channels: [
        { id: 'general', name: 'general', private: false, unread: 0 },
        { id: 'agents', name: 'agents', private: false, unread: 0 },
        { id: 'random', name: 'random', private: false, unread: 0 },
      ],
      directMessages: agents.map((a) => ({ id: a.id, name: a.name, unread: 0 })),
      projects: [
        {
          id: 'p1',
          name: 'Active project',
          groups: [
            {
              id: 'g1',
              name: 'Development',
              channels: [
                { id: 'frontend', name: 'frontend', private: false, unread: 0 },
                { id: 'backend', name: 'backend', private: false, unread: 0 },
              ],
            },
          ],
        },
      ],
    };
    return [ws];
  });

  const [activeWorkspace] = useState('default');
  void activeWorkspace; // reserved for multi-workspace support
  const [currentChannel, setCurrentChannel] = useState('general');
  const [currentView, setCurrentView] = useState<'channel' | 'inbox' | 'projects'>('channel');
  const [replyTo, setReplyTo] = useState<string | null>(null);
  const [input, setInput] = useState('');
  const [tags] = useState(['#bug', '#feature', '#question', '#release']);
  const [currentTag, setCurrentTag] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const workspace = workspaces[0];

  // Map conversations to a channel view by selecting active conversation messages for the active channel
  const messages: ChatMessage[] = activeConversationId
    ? (messagesByChat[activeConversationId] || []).map((m) => ({
        ...m,
        author: m.role === 'user' ? 'You' : m.role === 'assistant' ? 'Assistant' : 'System',
      }))
    : [];

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'auto' });
  }, [messages]);

  function send() {
    if (!input.trim() || !activeConversationId) return;
    addMessage(activeConversationId, {
      id: `${Date.now()}`,
      role: 'user',
      content: input,
      created_at: new Date().toISOString(),
    });
    setInput('');
    setReplyTo(null);
    setCurrentTag('');
  }

  function cycleTag() {
    const idx = tags.indexOf(currentTag);
    const next = idx === -1 ? 0 : (idx + 1) % tags.length;
    setCurrentTag(tags[next] === currentTag ? '' : tags[next]);
  }

  return (
    <div className="threads-page">
      <div className="threads-sidebar">
        <div className="threads-workspace-header">
          <div className="workspace-dot" style={{ background: workspace.color }} />
          <span className="workspace-name">{workspace.name}</span>
        </div>

        <div className="threads-section">
          <div className="threads-section-title">Channels</div>
          {workspace.channels.map((c) => (
            <div
              key={c.id}
              className={`threads-item ${currentView === 'channel' && currentChannel === c.id ? 'selected' : ''}`}
              onClick={() => { setCurrentView('channel'); setCurrentChannel(c.id); }}
            >
              <span>{c.private ? '🔒' : '#'}</span>
              <span>{c.name}</span>
              {c.unread > 0 && <span className="threads-badge">{c.unread}</span>}
            </div>
          ))}
        </div>

        <div className="threads-section">
          <div className="threads-section-title">Direct messages</div>
          {workspace.directMessages.map((d) => (
            <div
              key={d.id}
              className={`threads-item ${currentView === 'channel' && currentChannel === d.id ? 'selected' : ''}`}
              onClick={() => { setCurrentView('channel'); setCurrentChannel(d.id); setActiveConversation(d.id); }}
            >
              <div className="threads-avatar-small" style={{ background: avatarColor(d.name) }}>{avatarInitials(d.name)}</div>
              <span>{d.name}</span>
            </div>
          ))}
        </div>

        <div className="threads-section">
          <div className="threads-section-title">Projects</div>
          {workspace.projects.map((p) => (
            <div key={p.id}>
              <div className="threads-project-name">{p.name}</div>
              {p.groups.map((g) => (
                <div key={g.id}>
                  <div className="threads-group-name">{g.name}</div>
                  {g.channels.map((c) => (
                    <div
                      key={c.id}
                      className={`threads-item nested ${currentView === 'channel' && currentChannel === c.id ? 'selected' : ''}`}
                      onClick={() => { setCurrentView('channel'); setCurrentChannel(c.id); }}
                    >
                      <span>{c.private ? '🔒' : '#'}</span>
                      <span>{c.name}</span>
                    </div>
                  ))}
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>

      <div className="threads-main">
        <div className="threads-topbar">
          <div className="threads-topbar-left">
            <span className="threads-channel-name">
              {currentView === 'channel' ? `#${currentChannel}` : currentView === 'inbox' ? 'Inbox' : 'Projects'}
            </span>
          </div>
          <div className="threads-topbar-right">
            <button className={`threads-topbar-btn ${currentView === 'inbox' ? 'active' : ''}`} onClick={() => setCurrentView('inbox')}>Inbox</button>
            <button className={`threads-topbar-btn ${currentView === 'projects' ? 'active' : ''}`} onClick={() => setCurrentView('projects')}>Projects</button>
          </div>
        </div>

        <div className="threads-messages">
          {currentView === 'channel' && (
            messages.length === 0 ? (
              <div className="threads-empty">No messages yet in #{currentChannel}.</div>
            ) : (
              messages.map((m) => (
                <div key={m.id} className="threads-message">
                  <div className="threads-message-avatar" style={{ background: avatarColor((m as ChatMessage & { author?: string }).author || 'System') }}>
                    {avatarInitials((m as ChatMessage & { author?: string }).author || 'System')}
                  </div>
                  <div className="threads-message-body">
                    <div className="threads-message-meta">
                      <span className="threads-message-author">{(m as ChatMessage & { author?: string }).author || 'System'}</span>
                      <span className="threads-message-time">{formatTime(m.created_at)}</span>
                    </div>
                    <div className="threads-message-text">{m.content}</div>
                    <div className="threads-message-actions">
                      <button className="threads-action-btn" onClick={() => setReplyTo(m.id)}>Reply</button>
                    </div>
                  </div>
                </div>
              ))
            )
          )}
          {currentView === 'inbox' && <div className="threads-empty">No mentions or replies yet.</div>}
          {currentView === 'projects' && (
            <div className="projects-view">
              {workspace.projects.map((p) => (
                <div key={p.id} className="project-card">
                  <div className="project-header">
                    <span className="project-icon">📁</span>
                    {p.name}
                  </div>
                  {p.groups.map((g) => (
                    <div key={g.id}>
                      <div className="project-group-name">{g.name}</div>
                      <div className="project-group-channels">
                        {g.channels.map((c) => (
                          <div key={c.id} className="project-channel-item" onClick={() => { setCurrentView('channel'); setCurrentChannel(c.id); }}>
                            <span>{c.private ? '🔒' : '#'}</span>
                            {c.name}
                          </div>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              ))}
            </div>
          )}
          <div ref={messagesEndRef} />
        </div>

        <div className="threads-composer">
          {replyTo && (
            <div className="composer-context">
              <span>↳ Replying to message</span>
              <button className="cancel-reply" onClick={() => setReplyTo(null)}>×</button>
            </div>
          )}
          <input
            className="threads-composer-input"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } }}
            placeholder="Message..."
          />
          <div className="composer-toolbar">
            <div className="toolbar-left">
              <button className={`toolbar-btn ${currentTag ? 'active' : ''}`} onClick={cycleTag}>
                {currentTag || '#'}
              </button>
              <button className="toolbar-btn">😊</button>
            </div>
            <button className="composer-send" onClick={send} disabled={!input.trim()}>Send</button>
          </div>
        </div>
      </div>
    </div>
  );
}
