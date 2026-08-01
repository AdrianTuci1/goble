import { useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import './ThreadsPage.css';
import { useStore } from '../stores/appStore';
import { initialWorkspaces, currentUser as mockCurrentUser } from '../mocks/threadsData';
import { Lock } from 'lucide-react';

interface ThreadChannel {
  id: string;
  name: string;
  private: boolean;
  unread: number;
  total?: number;
  members?: string[];
}

interface DirectMessage {
  id: string;
  name: string;
  online: boolean;
  unread: number;
}

interface ThreadGroup {
  name: string;
  channels: ThreadChannel[];
}

interface ThreadProject {
  id: string;
  name: string;
  groups: ThreadGroup[];
}

interface ThreadWorkspace {
  id: string;
  name: string;
  color: string;
  channels: ThreadChannel[];
  directMessages: DirectMessage[];
  projects: ThreadProject[];
  messagesByChannel: Record<string, ThreadMessage[]>;
  directMessagesById: Record<string, ThreadMessage[]>;
  tags: string[];
  authorizedKeys: Array<{ userId: string; name: string; publicKeyPem: string; role: string; privateChannels?: string[] }>;
}

interface ThreadMessage {
  id: number | string;
  text: string;
  author: string;
  time: string;
  tag?: string;
  replyTo?: number | string;
  reactions?: Array<{ emoji: string; count: number }>;
}

const AVATAR_COLORS: Record<string, string> = {
  Adrian: '#2563eb',
  You: '#22c55e',
  'Release bot': '#9ca3af',
  'Maya Chen': '#ec4899',
  'Jordan Brooks': '#8b5cf6',
  'Camille Dubois': '#f97316',
  Fizz: '#10b981',
  Honey: '#ef4444',
};

function avatarColor(author: string) {
  if (AVATAR_COLORS[author]) return AVATAR_COLORS[author];
  let hash = 0;
  for (let i = 0; i < author.length; i++) hash = author.charCodeAt(i) + ((hash << 5) - hash);
  const hue = Math.abs(hash) % 360;
  return `hsl(${hue}, 60%, 45%)`;
}

function avatarInitials(name: string) {
  return name.split(' ').map((n) => n[0]).join('').slice(0, 2).toUpperCase();
}

function canAccessChannel(workspace: ThreadWorkspace, channel: ThreadChannel, userId: string) {
  if (!channel.private) return true;
  const member = workspace.authorizedKeys.find((m) => m.userId === userId);
  if (!member) return false;
  if (member.role === 'owner' || member.role === 'admin') return true;
  if (channel.members?.includes(userId)) return true;
  if (member.privateChannels?.includes(channel.id)) return true;
  return false;
}

export default function ThreadsPage() {
  const navigate = useNavigate();
  const setSelectedFlowId = useStore((s) => s.setSelectedFlowId);
  void setSelectedFlowId; // used when opening flows from threads later
  const [workspaces] = useState<ThreadWorkspace[]>(() => initialWorkspaces as unknown as ThreadWorkspace[]);
  const [currentWorkspaceId, setCurrentWorkspaceId] = useState(workspaces[0]?.id || null);
  const [currentView, setCurrentView] = useState<'channel' | 'directMessage' | 'inbox' | 'projects'>('channel');
  const [currentChannelId, setCurrentChannelId] = useState<string | null>(null);
  const [currentDmId, setCurrentDmId] = useState<string | null>(null);
  const [replyTo, setReplyTo] = useState<number | string | null>(null);
  const [currentTag, setCurrentTag] = useState('');
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const workspace = useMemo(() => workspaces.find((w) => w.id === currentWorkspaceId) || workspaces[0], [workspaces, currentWorkspaceId]);

  const channels = useMemo(() => {
    return workspace.channels.filter((c) => canAccessChannel(workspace, c, mockCurrentUser.id));
  }, [workspace]);

  useEffect(() => {
    if (currentView === 'channel' && !currentChannelId) {
      setCurrentChannelId(channels[0]?.id || null);
    }
  }, [channels, currentChannelId, currentView]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'auto', block: 'end' });
  }, [currentWorkspaceId, currentChannelId, currentDmId, currentView, workspace]);

  function selectWorkspace(id: string) {
    setCurrentWorkspaceId(id);
    setCurrentView('channel');
    setCurrentChannelId(null);
    setCurrentDmId(null);
    setReplyTo(null);
    setCurrentTag('');
  }

  function selectItem(item: { type: 'channel' | 'directMessage' | 'inbox' | 'projects'; id?: string }) {
    if (item.type === 'inbox') {
      setCurrentView('inbox');
      setCurrentChannelId(null);
      setCurrentDmId(null);
      return;
    }
    if (item.type === 'projects') {
      setCurrentView('projects');
      setCurrentChannelId(null);
      setCurrentDmId(null);
      return;
    }
    if (item.type === 'channel') {
      setCurrentView('channel');
      setCurrentChannelId(item.id || null);
      setCurrentDmId(null);
      const channel = workspace.channels.find((c) => c.id === item.id);
      if (channel) channel.unread = 0;
      return;
    }
    if (item.type === 'directMessage') {
      setCurrentView('directMessage');
      setCurrentDmId(item.id || null);
      setCurrentChannelId(null);
      const dm = workspace.directMessages.find((d) => d.id === item.id);
      if (dm) dm.unread = 0;
      return;
    }
  }

  function currentMessages(): ThreadMessage[] {
    if (currentView === 'channel' && currentChannelId) return workspace.messagesByChannel[currentChannelId] || [];
    if (currentView === 'directMessage' && currentDmId) return workspace.directMessagesById[currentDmId] || [];
    return [];
  }

  function headerIcon() {
    if (currentView === 'inbox') return '📥';
    if (currentView === 'projects') return '📁';
    if (currentView === 'channel') {
      const channel = workspace.channels.find((c) => c.id === currentChannelId);
      return channel?.private ? (
        <span className="header-lock">
          <Lock size={16} />
        </span>
      ) : '#';
    }
    if (currentView === 'directMessage') {
      const dm = workspace.directMessages.find((d) => d.id === currentDmId);
      return dm ? (
        <div className="threads-message-avatar avatar-small" style={{ background: avatarColor(dm.name) }}>
          {avatarInitials(dm.name)}
        </div>
      ) : null;
    }
    return null;
  }

  function headerTitle() {
    if (currentView === 'inbox') return 'Inbox';
    if (currentView === 'projects') return 'Projects';
    if (currentView === 'channel') return workspace.channels.find((c) => c.id === currentChannelId)?.name || '';
    if (currentView === 'directMessage') return workspace.directMessages.find((d) => d.id === currentDmId)?.name || '';
    return '';
  }

  function hasComposer() {
    return currentView === 'channel' || currentView === 'directMessage';
  }

  function replyLabel(replyToId: number | string) {
    const all = Object.values(workspace.messagesByChannel).flat().concat(Object.values(workspace.directMessagesById).flat());
    const msg = all.find((m) => m.id === replyToId);
    return msg ? `${msg.author}: ${msg.text.slice(0, 40)}` : 'message';
  }

  function send() {
    if (!input.trim() || !hasComposer()) return;
    const list = currentView === 'channel' ? workspace.messagesByChannel[currentChannelId || ''] : workspace.directMessagesById[currentDmId || ''];
    if (!list) return;
    list.push({
      id: Date.now(),
      text: input,
      author: 'You',
      time: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
      tag: currentTag || undefined,
      replyTo: replyTo || undefined,
    });
    setInput('');
    setReplyTo(null);
    setCurrentTag('');
  }

  function cycleTag() {
    const tags = workspace.tags;
    const idx = tags.indexOf(currentTag);
    const next = idx === -1 ? 0 : (idx + 1) % tags.length;
    setCurrentTag(tags[next] === currentTag ? '' : tags[next]);
  }

  function renderProjects() {
    return (
      <div className="projects-view">
        {workspace.projects.map((p) => (
          <div key={p.id} className="project-card">
            <div className="project-header">
              <span className="project-icon">📁</span>
              {p.name}
            </div>
            {p.groups.map((g) => (
              <div key={g.name}>
                <div className="project-group-name">{g.name}</div>
                <div className="project-group-channels">
                  {g.channels.map((c) => (
                    <div
                      key={c.id}
                      className="project-channel-item"
                      onClick={() => selectItem({ type: 'channel', id: c.id })}
                    >
                      {c.private ? (
                        <span className="channel-lock">
                          <Lock size={12} />
                        </span>
                      ) : '#'}
                      {c.name}
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        ))}
      </div>
    );
  }

  return (
    <div className="threads-page">
      <div className="threads-view">
        <div className="threads-workspace-sidebar">
          <div className="workspace-list">
            {workspaces.map((w) => {
              const selected = w.id === currentWorkspaceId;
              const initials = avatarInitials(w.name);
              return (
                <div
                  key={w.id}
                  className={`workspace-item ${selected ? 'selected' : ''}`}
                  style={{ background: w.color }}
                  title={w.name}
                  onClick={() => selectWorkspace(w.id)}
                >
                  <span className="workspace-initials">{initials}</span>
                </div>
              );
            })}
          </div>
          <div className="workspace-divider" />
          <button
            className="workspace-add"
            title="Add workspace"
            onClick={() => {
              const id = 'workspace-' + Date.now();
              const newWorkspace: ThreadWorkspace = {
                id,
                name: 'New',
                color: '#2563eb',
                channels: [],
                directMessages: [],
                projects: [],
                messagesByChannel: {},
                directMessagesById: {},
                tags: ['#bug', '#feature', '#question', '#release'],
                authorizedKeys: [],
              };
              workspaces.push(newWorkspace);
              selectWorkspace(id);
            }}
          >
            +
          </button>
        </div>

        <div className="threads-sidebar">
          <div className="threads-sidebar-nav">
            <div className={`nav-item inbox-item ${currentView === 'inbox' ? 'selected' : ''}`} onClick={() => selectItem({ type: 'inbox' })}>
              <span className="nav-icon">📥</span>
              <span className="nav-label">Inbox</span>
            </div>
            <div className={`nav-item projects-item ${currentView === 'projects' ? 'selected' : ''}`} onClick={() => selectItem({ type: 'projects' })}>
              <span className="nav-icon">📁</span>
              <span className="nav-label">Projects</span>
            </div>
            <div className="nav-item agents-item" onClick={() => navigate('/agents')}>
              <span className="nav-icon">🤖</span>
              <span className="nav-label">Agents</span>
            </div>
          </div>

          <div className="threads-sidebar-section">
            <h4>Channels</h4>
            <div className="channel-list">
              {channels.map((c) => {
                const selected = currentView === 'channel' && currentChannelId === c.id;
                return (
                  <div
                    key={c.id}
                    className={`channel-item ${selected ? 'selected' : ''}`}
                    onClick={() => selectItem({ type: 'channel', id: c.id })}
                  >
                    <span className="channel-icon">
                      {c.private ? <span className="channel-lock"><Lock size={12} /></span> : '#'}
                    </span>
                    <span className="channel-name">{c.name}</span>
                    {c.unread > 0 && <span className="unread-badge">{c.unread}{c.total ? '/' + c.total : ''}</span>}
                  </div>
                );
              })}
            </div>
          </div>

          <div className="threads-sidebar-section">
            <h4>Direct messages</h4>
            <div className="dm-list">
              {workspace.directMessages.map((d) => {
                const selected = currentView === 'directMessage' && currentDmId === d.id;
                return (
                  <div
                    key={d.id}
                    className={`dm-item ${selected ? 'selected' : ''}`}
                    onClick={() => selectItem({ type: 'directMessage', id: d.id })}
                  >
                    <span className="dm-avatar">
                      <div className="threads-message-avatar avatar-small" style={{ background: avatarColor(d.name) }}>
                        {avatarInitials(d.name)}
                      </div>
                    </span>
                    <span className="dm-name">{d.name}</span>
                    {d.unread > 0 && <span className="unread-badge">{d.unread}</span>}
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        <div className="threads-main">
          <div className="threads-header">
            <div className="header-left">
              <span className="header-icon">{headerIcon()}</span>
              <span className="header-title">{headerTitle()}</span>
            </div>
            <div className="header-right">
              <button className="header-action members-btn" title="Members">👤</button>
              <button className="header-action call-btn" title="Call">🎧</button>
            </div>
          </div>

          <div className="threads-main-content">
            {currentView === 'inbox' && <div className="inbox-view">No mentions or replies yet.</div>}
            {currentView === 'projects' && renderProjects()}
            {(currentView === 'channel' || currentView === 'directMessage') && (
              <div className="threads-messages">
                {currentMessages().length === 0 ? (
                  <div className="threads-empty">No messages yet</div>
                ) : (
                  currentMessages().map((msg, idx) => {
                    const prev = currentMessages()[idx - 1];
                    const isGrouped = prev && prev.author === msg.author;
                    const replyCount = currentMessages().filter((m) => m.replyTo === msg.id).length;
                    return (
                      <div key={msg.id} className={`threads-message ${isGrouped ? 'grouped' : ''}`}>
                        {isGrouped ? (
                          <div className="threads-message-avatar placeholder" />
                        ) : (
                          <div className="threads-message-avatar" style={{ background: avatarColor(msg.author) }}>
                            {avatarInitials(msg.author)}
                          </div>
                        )}
                        <div className="threads-message-content">
                          {!isGrouped && (
                            <div className="threads-message-header">
                              <span className="threads-message-author">{msg.author}</span>
                              <span className="threads-message-time">{msg.time}</span>
                            </div>
                          )}
                          <div className="threads-message-body">{msg.text}</div>
                          {msg.replyTo && <div className="threads-message-reply">↳ replying to {replyLabel(msg.replyTo)}</div>}
                          <div className="threads-message-footer">
                            <button className="msg-action reply-btn" onClick={() => setReplyTo(msg.id)}>Reply</button>
                            <button className="msg-action tag-btn" onClick={() => cycleTag()}>Tag</button>
                            <button className="msg-action react-btn">React</button>
                            {msg.tag && <span className="message-tag">{msg.tag}</span>}
                            {msg.reactions?.map((r) => (
                              <button key={r.emoji} className="reaction">
                                {r.emoji} {r.count}
                              </button>
                            ))}
                            {replyCount > 0 && <button className="reply-badge">💬 {replyCount}</button>}
                          </div>
                        </div>
                      </div>
                    );
                  })
                )}
                <div ref={messagesEndRef} />
              </div>
            )}
          </div>

          <div className={`threads-composer ${hasComposer() ? '' : 'hidden'}`}>
            {replyTo && (
              <div className="composer-context">
                <span>↳ Replying to {replyLabel(replyTo)}</span>
                <button className="cancel-reply" onClick={() => setReplyTo(null)}>×</button>
              </div>
            )}
            <input
              type="text"
              className="composer-input"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') send(); }}
              placeholder={currentView === 'channel' ? `Message #${headerTitle()}` : `Message ${headerTitle()}`}
            />
            <div className="composer-toolbar">
              <div className="toolbar-left">
                <button className="toolbar-btn mention-btn" title="Mention" onClick={() => setInput((v) => v + '@')}>@</button>
                <button className="toolbar-btn attach-btn" title="Attach" onClick={() => setInput((v) => v + '📎 ')}>📎</button>
                <button className="toolbar-btn emoji-btn" title="Emoji" onClick={() => setInput((v) => v + '☺ ')}>☺</button>
                <button className={`toolbar-btn tag-btn ${currentTag ? 'active' : ''}`} title="Tag" onClick={cycleTag}>
                  {currentTag || '#'}
                </button>
                <button className="toolbar-btn format-btn" title="Format" onClick={() => setInput((v) => v + '**')}>Aa</button>
              </div>
              <button className="composer-send" title="Send" onClick={send}>↑</button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
