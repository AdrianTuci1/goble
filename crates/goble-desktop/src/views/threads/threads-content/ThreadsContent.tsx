import { useState } from 'react';
import { Hash, Lock, AtSign, Paperclip, Smile, Plus, Send } from 'lucide-react';
import { useThreadsStore, activeMessages } from '../store/threadsStore';
import type { ThreadMessage } from '../data/threadsData';
import { currentUser } from '../data/threadsData';
import './ThreadsContent.css';

export default function ThreadsContent() {
  const [input, setInput] = useState('');
  const { nav, activeChannelId, activeDmId, workspaces, activeWorkspaceId, addMessage } = useThreadsStore();
  const ws = workspaces.find((w) => w.id === activeWorkspaceId);
  const messages = activeMessages({ workspaces, activeWorkspaceId, nav, activeChannelId, activeDmId } as never);
  const activeName = ws
    ? ws.channels.find((c) => c.id === activeChannelId)?.name ||
      ws.directMessages.find((d) => d.id === activeDmId)?.name ||
      ''
    : '';

  function send() {
    if (!input.trim()) return;
    const targetId = activeChannelId || activeDmId;
    if (!targetId) return;
    addMessage(targetId, {
      id: `m-${Date.now()}`,
      author: currentUser.name,
      content: input.trim(),
      timestamp: new Date().toISOString(),
      reactions: {},
      tags: [],
    });
    setInput('');
  }

  return (
    <main className="threads-main">
      <div className="threads-header">
        <div className="header-left">
          <span className="header-icon">{activeChannelId ? <Hash size={18} /> : <AtSign size={18} />}</span>
          <span>{activeName || 'Threads'}</span>
          {ws?.channels.find((c) => c.id === activeChannelId)?.private && <Lock size={14} className="header-lock" />}
        </div>
        <div className="header-right">
          <button className="header-action" title="Search">🔍</button>
          <button className="header-action" title="More">⋯</button>
        </div>
      </div>

      <div className="threads-main-content">
        {nav === 'projects' ? (
          <ProjectsView />
        ) : messages.length === 0 ? (
          <div className="threads-empty">No messages yet in {activeName || 'this thread'}.</div>
        ) : (
          <div className="threads-messages">
            {messages.map((m) => <MessageRow key={m.id} message={m} />)}
          </div>
        )}
      </div>

      <div className="threads-composer">
        <div className="composer-input-row">
          <textarea
            className="composer-input"
            placeholder="Message..."
            rows={1}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
          />
        </div>
        <div className="composer-toolbar">
          <div className="toolbar-left">
            <button className="toolbar-btn" title="Mention"><AtSign size={16} /></button>
            <button className="toolbar-btn" title="Attach"><Paperclip size={16} /></button>
            <button className="toolbar-btn" title="Emoji"><Smile size={16} /></button>
            <button className="toolbar-btn" title="Add"><Plus size={16} /></button>
          </div>
          <button className="composer-send" onClick={send} disabled={!input.trim()}><Send size={16} /></button>
        </div>
      </div>
    </main>
  );
}

function MessageRow({ message }: { message: ThreadMessage }) {
  return (
    <div className="threads-message">
      <div className="threads-message-avatar" style={{ background: `hsl(${Math.abs(message.author.split('').reduce((a, b) => a + b.charCodeAt(0), 0)) % 360}, 60%, 45%)` }}>
        {message.author.slice(0, 2).toUpperCase()}
      </div>
      <div className="threads-message-content">
        <div className="threads-message-header">
          <span className="threads-message-author">{message.author}</span>
          <span className="threads-message-time">{formatTime(message.timestamp)}</span>
        </div>
        <div className="threads-message-body">{message.content}</div>
        {message.replyTo && (
          <div className="threads-message-reply">↪ Replying to {message.replyTo.author}</div>
        )}
        {message.tags.length > 0 && (
          <div className="threads-message-footer">
            {message.tags.map((t) => <span key={t} className="message-tag">{t}</span>)}
          </div>
        )}
      </div>
    </div>
  );
}

function ProjectsView() {
  const { workspaces, activeWorkspaceId } = useThreadsStore();
  const ws = workspaces.find((w) => w.id === activeWorkspaceId);
  if (!ws || ws.projects.length === 0) return <div className="threads-empty">No projects yet.</div>;
  return (
    <div className="projects-view">
      <div className="projects-header">
        <h3>Projects</h3>
        <button className="new-project-btn">+ New project</button>
      </div>
      <div className="projects-list">
        {ws.projects.map((p) => (
          <div key={p.id} className="project-card">
            <div className="project-header">📁 {p.name}</div>
            <div className="project-tasks">
              {p.items.map((item) => (
                <div key={item.id} className="project-task">
                  <span className="project-task-status">{item.status}</span>
                  <span>{item.title}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function formatTime(iso: string) {
  try {
    return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  } catch {
    return iso;
  }
}
