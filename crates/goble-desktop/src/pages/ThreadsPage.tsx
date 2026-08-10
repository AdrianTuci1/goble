import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import './ThreadsPage.css';
import { useStore, type Participant } from '../stores/appStore';
import {
  addThreadParticipant,
  createThread,
  getThreadMessages,
  getThreadParticipants,
  listAgents,
  listThreads,
  onThreadMessagesUpdated,
  onThreadsUpdated,
  postThreadMessage,
} from '../tauri/api';
import { Lock } from 'lucide-react';
import { getInitials } from '../utils/designSystem';

function participantKey(p: Participant) {
  return `${p.kind}:${p.id}`;
}

export default function ThreadsPage() {
  const navigate = useNavigate();
  const threads = useStore((s) => s.threads);
  const setThreads = useStore((s) => s.setThreads);
  const activeThreadId = useStore((s) => s.activeThreadId);
  const setActiveThreadId = useStore((s) => s.setActiveThreadId);
  const threadMessages = useStore((s) => s.threadMessages);
  const setThreadMessages = useStore((s) => s.setThreadMessages);
  const addThreadMessage = useStore((s) => s.addThreadMessage);
  const threadParticipants = useStore((s) => s.threadParticipants);
  const setThreadParticipants = useStore((s) => s.setThreadParticipants);
  const agents = useStore((s) => s.agents);
  const setAgents = useStore((s) => s.setAgents);
  const replyToMessageId = useStore((s) => s.replyToMessageId);
  const setReplyToMessageId = useStore((s) => s.setReplyToMessageId);
  const pendingTags = useStore((s) => s.pendingTags);
  const togglePendingTag = useStore((s) => s.togglePendingTag);
  const setPendingTags = useStore((s) => s.setPendingTags);
  const participantsPanelOpen = useStore((s) => s.participantsPanelOpen);
  const setParticipantsPanelOpen = useStore((s) => s.setParticipantsPanelOpen);

  const [input, setInput] = useState('');
  const [showNewChannel, setShowNewChannel] = useState(false);
  const [newChannelName, setNewChannelName] = useState('');
  const [showParticipantPicker, setShowParticipantPicker] = useState(false);
  const [participantQuery, setParticipantQuery] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    refresh();
    let unsubs: (() => void)[] = [];
    (async () => {
      unsubs.push(await onThreadsUpdated(refresh));
      unsubs.push(await onThreadMessagesUpdated((event) => loadMessages(event.payload.thread_id)));
    })();
    return () => unsubs.forEach((u) => u());
  }, []);

  useEffect(() => {
    if (activeThreadId) {
      loadMessages(activeThreadId);
      loadParticipants(activeThreadId);
    }
  }, [activeThreadId]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'auto' });
  }, [activeThreadId, threadMessages]);

  async function refresh() {
    try {
      const [t, a] = await Promise.all([listThreads(), listAgents()]);
      setThreads(t);
      setAgents(a);
    } catch {
      // ignore
    }
  }

  async function loadMessages(threadId: string) {
    try {
      const msgs = await getThreadMessages(threadId);
      setThreadMessages(threadId, msgs);
    } catch {
      // ignore
    }
  }

  async function loadParticipants(threadId: string) {
    try {
      const parts = await getThreadParticipants(threadId);
      setThreadParticipants(threadId, parts);
    } catch {
      // ignore
    }
  }

  async function handleCreateChannel() {
    if (!newChannelName.trim()) return;
    await createThread('channel', newChannelName.trim(), []);
    setNewChannelName('');
    setShowNewChannel(false);
  }

  async function handleSend() {
    if (!input.trim() || !activeThreadId) return;
    const text = input.trim();
    setInput('');
    try {
      const msg = await postThreadMessage(activeThreadId, text, {
        reply_to: replyToMessageId ?? undefined,
        tags: pendingTags,
        mentions: extractMentions(text),
      });
      addThreadMessage(activeThreadId, msg);
      setReplyToMessageId(null);
      setPendingTags([]);
    } catch {
      // fallback
    }
  }

  async function inviteAgent(agentId: string) {
    if (!activeThreadId) return;
    await addThreadParticipant(activeThreadId, { kind: 'agent', id: agentId });
    loadParticipants(activeThreadId);
  }

  const activeThread = threads.find((t) => t.id === activeThreadId);
  const messages = activeThreadId ? threadMessages[activeThreadId] || [] : [];
  const participants = activeThreadId ? threadParticipants[activeThreadId] || [] : [];
  const channels = threads.filter((t) => t.kind === 'channel');
  const dms = threads.filter((t) => t.kind === 'direct');
  const chats = threads.filter((t) => t.kind === 'chat');

  const availableAgents = agents.filter(
    (a) => !participants.some((p) => p.kind === 'agent' && p.id === a.spec.id['0'])
  );
  const filteredAgents = availableAgents.filter((a) =>
    a.name.toLowerCase().includes(participantQuery.toLowerCase()) ||
    a.spec.id['0'].toLowerCase().includes(participantQuery.toLowerCase())
  );

  return (
    <div className="threads-page">
      <div className="threads-view">
        <div className="threads-sidebar">
          <div className="threads-sidebar-nav">
            <div className="nav-item inbox-item" onClick={() => navigate('/chat')}>📥 Chat</div>
          </div>

          <div className="threads-sidebar-section">
            <h4>
              Channels
              <button className="channel-add" onClick={() => setShowNewChannel(true)}>+</button>
            </h4>
            <div className="channel-list">
              {channels.map((c) => (
                <div
                  key={c.id}
                  className={`channel-item ${activeThreadId === c.id ? 'selected' : ''}`}
                  onClick={() => setActiveThreadId(c.id)}
                >
                  <span className="channel-icon">#</span>
                  <span className="channel-name">{c.title}</span>
                </div>
              ))}
              {showNewChannel && (
                <div className="channel-item new-channel">
                  <input
                    value={newChannelName}
                    onChange={(e) => setNewChannelName(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && handleCreateChannel()}
                    placeholder="channel-name"
                    autoFocus
                  />
                </div>
              )}
            </div>
          </div>

          <div className="threads-sidebar-section">
            <h4>Direct messages</h4>
            <div className="dm-list">
              {dms.map((d) => (
                <div
                  key={d.id}
                  className={`dm-item ${activeThreadId === d.id ? 'selected' : ''}`}
                  onClick={() => setActiveThreadId(d.id)}
                >
                  <span className="dm-name">{d.title}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="threads-sidebar-section">
            <h4>Chats</h4>
            <div className="dm-list">
              {chats.map((c) => (
                <div
                  key={c.id}
                  className={`dm-item ${activeThreadId === c.id ? 'selected' : ''}`}
                  onClick={() => setActiveThreadId(c.id)}
                >
                  <span className="dm-name">{c.title}</span>
                </div>
              ))}
            </div>
          </div>
        </div>

        <div className="threads-main">
          <div className="threads-header">
            <div className="header-left">
              {activeThread?.kind === 'channel' ? '#' : activeThread?.kind === 'direct' ? <Lock size={14} /> : '💬'}
              <span className="header-title">{activeThread?.title || 'Select a thread'}</span>
            </div>
            <div className="header-right">
              <button className="header-action" onClick={() => setParticipantsPanelOpen(true)} title="Members">👤</button>
            </div>
          </div>

          <div className="threads-main-content">
            <div className="threads-messages">
              {messages.length === 0 ? (
                <div className="threads-empty">No messages yet</div>
              ) : (
                messages.map((msg) => (
                  <div key={msg.id} className={`threads-message ${msg.author.kind === 'user' ? 'own' : ''}`}>
                    <div className="threads-message-avatar" style={{ background: avatarColor(msg.author) }}>
                      {getInitials(msg.author.id)}
                    </div>
                    <div className="threads-message-content">
                      <div className="threads-message-header">
                        <span className="threads-message-author">
                          {msg.author.kind === 'agent' ? '🤖 ' : ''}
                          {msg.author.id}
                        </span>
                        {msg.reply_to && <span className="reply-indicator">↳ reply</span>}
                      </div>
                      <div className="threads-message-body">{msg.content}</div>
                      {msg.tags.length > 0 && (
                        <div className="message-tags">
                          {msg.tags.map((t) => <span key={t} className="message-tag">{t}</span>)}
                        </div>
                      )}
                      <div className="threads-message-footer">
                        <button className="msg-action" onClick={() => setReplyToMessageId(msg.id)}>Reply</button>
                        {msg.reactions.map((r) => (
                          <span key={r.participant_id + r.emoji} className="reaction">{r.emoji}</span>
                        ))}
                      </div>
                    </div>
                  </div>
                ))
              )}
              <div ref={messagesEndRef} />
            </div>
          </div>

          {replyToMessageId && (
            <div className="composer-context">
              Replying to {messages.find((m) => m.id === replyToMessageId)?.author.id}
              <button className="cancel-reply" onClick={() => setReplyToMessageId(null)}>×</button>
            </div>
          )}

          <div className={`threads-composer ${activeThreadId ? '' : 'hidden'}`}>
            <input
              type="text"
              className="composer-input"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSend()}
              placeholder={activeThread ? `Message ${activeThread.title}` : 'Select a thread'}
              disabled={!activeThreadId}
            />
            <div className="composer-toolbar">
              <div className="toolbar-left">
                <button title="Mention" onClick={() => setInput((v) => v + '@')}>@</button>
                <button className={pendingTags.length ? 'active' : ''} title="Tag" onClick={() => togglePendingTag('#todo')}>#</button>
              </div>
              <button className="composer-send" onClick={handleSend} disabled={!input.trim()}>↑</button>
            </div>
          </div>
        </div>

        {participantsPanelOpen && activeThread && (
          <div className="participants-panel">
            <div className="participants-panel-header">
              <h4>Participants</h4>
              <button onClick={() => setParticipantsPanelOpen(false)}>×</button>
            </div>
            <div className="participants-list">
              {participants.map((p) => (
                <div key={participantKey(p)} className="participant-row">
                  <span>{p.kind === 'agent' ? '🤖' : '👤'}</span>
                  <span>{p.id}</span>
                </div>
              ))}
            </div>
            <div className="add-participant">
              <input
                placeholder="Add agent..."
                value={participantQuery}
                onChange={(e) => setParticipantQuery(e.target.value)}
                onFocus={() => setShowParticipantPicker(true)}
              />
              {showParticipantPicker && filteredAgents.length > 0 && (
                <div className="participant-picker">
                  {filteredAgents.map((a) => (
                    <div
                      key={a.spec.id['0']}
                      className="participant-option"
                      onClick={() => {
                        inviteAgent(a.spec.id['0']);
                        setParticipantQuery('');
                        setShowParticipantPicker(false);
                      }}
                    >
                      🤖 {a.name}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function avatarColor(author: Participant) {
  if (author.kind === 'agent') return '#10b981';
  if (author.kind === 'user') return '#2563eb';
  return '#9ca3af';
}

function extractMentions(content: string): string[] {
  const mentions: string[] = [];
  const seen = new Set<string>();
  const re = /@(user|agent):([a-zA-Z0-9_-]+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(content)) !== null) {
    const id = `${m[1]}:${m[2]}`;
    if (!seen.has(id)) {
      seen.add(id);
      mentions.push(id);
    }
  }
  return mentions;
}
