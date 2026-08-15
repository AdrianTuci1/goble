import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import './ThreadsPage.css';
import { useStore, type Participant } from '../stores/appStore';
import {
  addThreadParticipant,
  createThread,
  getThreadMessages,
  getThreadParticipants,
  getAuthorizedKeys,
  inviteUserByPublicKey,
  listAgents,
  listThreads,
  markThreadRead,
  onThreadMessageCreated,
  onThreadMessagesUpdated,
  onThreadUpdated,
  onThreadsUpdated,
  postThreadMessage,
  runAgentForThreadReply,
  addThreadReaction,
  removeThreadReaction,
  getUserProfile,
  type AuthorizedKey,
} from '../tauri/api';
import { Lock } from 'lucide-react';
import { getInitials } from '../utils/designSystem';
import ComposerRuntimeSelector, { type RuntimeTarget, runtimeTargetLabel } from '../components/ComposerRuntimeSelector';

function participantKey(p: Participant) {
  return `${p.kind}:${p.id}`;
}

const COMMON_EMOJIS = ['👍', '❤️', '😂', '🚀', '👀', '✅', '❓', '🔥'];
const TAG_SUGGESTIONS = ['#todo', '#question', '#blocked', '#decision', '#idea'];

export default function ThreadsPage() {
  const navigate = useNavigate();
  const threads = useStore((s) => s.threads);
  const setThreads = useStore((s) => s.setThreads);
  const workers = useStore((s) => s.workers);
  const agents = useStore((s) => s.agents);
  const setAgents = useStore((s) => s.setAgents);
  const activeThreadId = useStore((s) => s.activeThreadId);
  const setActiveThreadId = useStore((s) => s.setActiveThreadId);
  const threadMessages = useStore((s) => s.threadMessages);
  const setThreadMessages = useStore((s) => s.setThreadMessages);
  const addThreadMessage = useStore((s) => s.addThreadMessage);
  const threadParticipants = useStore((s) => s.threadParticipants);
  const setThreadParticipants = useStore((s) => s.setThreadParticipants);
  const replyToMessageId = useStore((s) => s.replyToMessageId);
  const setReplyToMessageId = useStore((s) => s.setReplyToMessageId);
  const pendingTags = useStore((s) => s.pendingTags);
  const togglePendingTag = useStore((s) => s.togglePendingTag);
  const setPendingTags = useStore((s) => s.setPendingTags);
  const participantsPanelOpen = useStore((s) => s.participantsPanelOpen);
  const setParticipantsPanelOpen = useStore((s) => s.setParticipantsPanelOpen);
  const threadRepliesOpen = useStore((s) => s.threadRepliesOpen);
  const setThreadRepliesOpen = useStore((s) => s.setThreadRepliesOpen);
  const threadEmojiPickerForMessageId = useStore((s) => s.threadEmojiPickerForMessageId);
  const setThreadEmojiPickerForMessageId = useStore((s) => s.setThreadEmojiPickerForMessageId);
  const markThreadReadLocal = useStore((s) => s.markThreadRead);

  const [input, setInput] = useState('');
  const [showNewChannel, setShowNewChannel] = useState(false);
  const [newChannelName, setNewChannelName] = useState('');
  const [showParticipantPicker, setShowParticipantPicker] = useState(false);
  const [participantQuery, setParticipantQuery] = useState('');
  const [showInviteUser, setShowInviteUser] = useState(false);
  const [inviteStep, setInviteStep] = useState<'key' | 'share'>('key');
  const [inviteKey, setInviteKey] = useState('');
  const [inviteName, setInviteName] = useState('');
  const [qrValue, setQrValue] = useState('');
  const [authorizedKeys, setAuthorizedKeys] = useState<AuthorizedKey[]>([]);
  const [showTagPicker, setShowTagPicker] = useState(false);
  const [runtimeTarget, setRuntimeTarget] = useState<RuntimeTarget>({ kind: 'auto' });
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const me = useStore((s) => s.userProfile);
  const myParticipantId = me ? `user:${me.id}` : null;

  useEffect(() => {
    refresh();
    let unsubs: (() => void)[] = [];
    (async () => {
      unsubs.push(await onThreadsUpdated(refresh));
      unsubs.push(
        await onThreadMessagesUpdated((event) => loadMessages(event.payload.thread_id))
      );
      unsubs.push(
        await onThreadMessageCreated((event) => {
          const { thread_id, message } = event.payload;
          addThreadMessage(thread_id, message);
          if (thread_id === activeThreadIdRef.current) {
            markAsRead(thread_id);
          }
        })
      );
      unsubs.push(await onThreadUpdated(refresh));
    })();
    return () => unsubs.forEach((u) => u());
  }, []);

  const activeThreadIdRef = useRef(activeThreadId);
  useEffect(() => {
    activeThreadIdRef.current = activeThreadId;
    if (activeThreadId) {
      loadMessages(activeThreadId);
      loadParticipants(activeThreadId);
      markAsRead(activeThreadId);
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

  async function markAsRead(threadId: string) {
    try {
      await markThreadRead(threadId);
      markThreadReadLocal(threadId, new Date().toISOString());
    } catch {
      // ignore
    }
  }

  function unreadCount(thread: Parameters<typeof setThreads>[0][number]): number {
    const messages = threadMessages[thread.id] ?? [];
    if (!messages.length) return 0;
    const lastRead = thread.last_read_at ? new Date(thread.last_read_at).getTime() : 0;
    return messages.filter((m) => new Date(m.created_at).getTime() > lastRead).length;
  }

  async function openInviteUser() {
    if (!activeThreadId) return;
    setShowInviteUser(true);
    setInviteStep('key');
    setInviteName('');
    setInviteKey('');
    setQrValue('');
    try {
      const keys = await getAuthorizedKeys();
      setAuthorizedKeys(keys);
    } catch {
      setAuthorizedKeys([]);
    }
  }

  async function handleCopyFingerprint() {
    try {
      const profile = await getUserProfile();
      if (profile?.public_key_pem) {
        const fingerprint = computeFingerprint(profile.public_key_pem);
        await navigator.clipboard.writeText(fingerprint);
      }
    } catch {
      // ignore
    }
  }

  function computeFingerprint(pem: string): string {
    let hash = 0;
    for (const c of pem.trim()) {
      hash = c.charCodeAt(0) + ((hash << 5) - hash);
    }
    return Math.abs(hash).toString(16).padStart(16, '0');
  }

  function buildQrValue() {
    if (!activeThreadId) return '';
    return JSON.stringify({
      thread_id: activeThreadId,
      public_key_pem: me?.public_key_pem || '',
      fingerprint: me?.public_key_pem ? computeFingerprint(me.public_key_pem) : '',
    });
  }

  async function handleInviteUser() {
    if (!activeThreadId || !inviteKey.trim()) return;
    try {
      await inviteUserByPublicKey(activeThreadId, inviteKey.trim(), inviteName.trim() || 'Invited');
      setInviteKey('');
      setInviteName('');
      setShowInviteUser(false);
      if (activeThreadId) loadParticipants(activeThreadId);
    } catch {
      // ignore
    }
  }

  async function handleCreateChannel() {
    if (!newChannelName.trim()) return;
    await createThread('channel', newChannelName.trim(), false, []);
    setNewChannelName('');
    setShowNewChannel(false);
  }

  async function handleSend() {
    if (!input.trim() || !activeThreadId) return;
    const text = input.trim();
    setInput('');
    try {
      const mentions = extractMentions(text);
      const msg = await postThreadMessage(activeThreadId, text, {
        reply_to: replyToMessageId ?? undefined,
        tags: pendingTags,
        mentions,
      });
      addThreadMessage(activeThreadId, msg);
      setReplyToMessageId(null);
      setPendingTags([]);
      for (const mentionId of mentions) {
        const agent = agents.find((a) => a.spec.id['0'] === mentionId);
        if (agent) {
          try {
            await runAgentForThreadReply(runtimeTarget, activeThreadId, mentionId, text);
          } catch {
            // Worker may not be reachable; ignore.
          }
        }
      }
    } catch {
      // fallback
    }
  }

  async function inviteAgent(agentId: string) {
    if (!activeThreadId) return;
    await addThreadParticipant(activeThreadId, { kind: 'agent', id: agentId });
    loadParticipants(activeThreadId);
  }

  async function toggleReaction(messageId: string, emoji: string) {
    if (!activeThreadId || !myParticipantId) return;
    const msg = messages.find((m) => m.id === messageId);
    const hasReaction = msg?.reactions.some((r) => r.emoji === emoji && r.participant_id === myParticipantId);
    try {
      if (hasReaction) {
        await removeThreadReaction(activeThreadId, messageId, emoji);
      } else {
        await addThreadReaction(activeThreadId, messageId, emoji);
      }
      loadMessages(activeThreadId);
    } catch {
      // ignore
    }
  }

  function countReactions(reactions: { emoji: string; participant_id: string }[]) {
    const counts = new Map<string, number>();
    for (const r of reactions) {
      counts.set(r.emoji, (counts.get(r.emoji) || 0) + 1);
    }
    return Array.from(counts.entries()).map(([emoji, count]) => ({
      emoji,
      count,
      me: myParticipantId ? reactions.some((r) => r.emoji === emoji && r.participant_id === myParticipantId) : false,
    }));
  }

  function getReplies(parentId: string) {
    return messages.filter((m) => m.reply_to === parentId);
  }

  function formatTime(ts: string) {
    try {
      return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    } catch {
      return '';
    }
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

  const renderMessage = (msg: (typeof messages)[0], nested = false) => {
    const replies = getReplies(msg.id);
    const replyCount = replies.length;
    const reactions = countReactions(msg.reactions);

    return (
      <div key={msg.id} className={`threads-message ${msg.author.kind === 'user' ? 'own' : ''} ${nested ? 'nested' : ''}`}>
        <div className="threads-message-avatar" style={{ background: avatarColor(msg.author) }}>
          {getInitials(msg.author.id)}
        </div>
        <div className="threads-message-content">
          <div className="threads-message-header">
            <span className="threads-message-author">
              {msg.author.kind === 'agent' ? '🤖 ' : ''}
              {msg.author.id}
            </span>
            <span className="threads-message-time">{formatTime(msg.created_at)}</span>
          </div>
          {msg.reply_to && (
            <div className="threads-message-reply">
              Replying to {messages.find((m) => m.id === msg.reply_to)?.author.id || 'unknown'}
            </div>
          )}
          <div className="threads-message-body">{msg.content}</div>
          {msg.tags.length > 0 && (
            <div className="message-tags">
              {msg.tags.map((t) => <span key={t} className="message-tag">{t}</span>)}
            </div>
          )}
          <div className="threads-message-footer">
            <button className="msg-action" onClick={() => setReplyToMessageId(msg.id)}>Reply</button>
            <button className="msg-action" onClick={() => setThreadEmojiPickerForMessageId(threadEmojiPickerForMessageId === msg.id ? null : msg.id)}>
              Add reaction
            </button>
            {replyCount > 0 && (
              <button className="reply-badge" onClick={() => setThreadRepliesOpen(msg.id, !threadRepliesOpen[msg.id])}>
                {replyCount} {replyCount === 1 ? 'reply' : 'replies'}
              </button>
            )}
            {reactions.map((r) => (
              <button
                key={r.emoji}
                className={`reaction ${r.me ? 'me' : ''}`}
                onClick={() => toggleReaction(msg.id, r.emoji)}
              >
                {r.emoji} {r.count}
              </button>
            ))}
          </div>
          {threadEmojiPickerForMessageId === msg.id && (
            <div className="emoji-picker-popover">
              {COMMON_EMOJIS.map((emoji) => (
                <button key={emoji} className="emoji-picker-btn" onClick={() => { toggleReaction(msg.id, emoji); setThreadEmojiPickerForMessageId(null); }}>
                  {emoji}
                </button>
              ))}
            </div>
          )}
          {threadRepliesOpen[msg.id] && replies.length > 0 && (
            <div className="thread-replies">
              {replies.map((r) => renderMessage(r, true))}
            </div>
          )}
        </div>
      </div>
    );
  };

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
              {channels.map((c) => {
                const unread = unreadCount(c);
                return (
                  <div
                    key={c.id}
                    className={`channel-item ${activeThreadId === c.id ? 'selected' : ''} ${unread ? 'has-unread' : ''}`}
                    onClick={() => setActiveThreadId(c.id)}
                  >
                    <span className="channel-icon">#</span>
                    <span className="channel-name">{c.title}</span>
                    {unread > 0 && <span className="unread-badge">{unread}</span>}
                  </div>
                );
              })}
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
              {dms.map((d) => {
                const unread = unreadCount(d);
                return (
                  <div
                    key={d.id}
                    className={`dm-item ${activeThreadId === d.id ? 'selected' : ''} ${unread ? 'has-unread' : ''}`}
                    onClick={() => setActiveThreadId(d.id)}
                  >
                    <span className="dm-name">{d.title}</span>
                    {unread > 0 && <span className="unread-badge">{unread}</span>}
                  </div>
                );
              })}
            </div>
          </div>

          <div className="threads-sidebar-section">
            <h4>Chats</h4>
            <div className="dm-list">
              {chats.map((c) => {
                const unread = unreadCount(c);
                return (
                  <div
                    key={c.id}
                    className={`dm-item ${activeThreadId === c.id ? 'selected' : ''} ${unread ? 'has-unread' : ''}`}
                    onClick={() => setActiveThreadId(c.id)}
                  >
                    <span className="dm-name">{c.title}</span>
                    {unread > 0 && <span className="unread-badge">{unread}</span>}
                  </div>
                );
              })}
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
                messages.filter((m) => !m.reply_to).map((msg) => renderMessage(msg))
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

          {pendingTags.length > 0 && (
            <div className="pending-tags">
              {pendingTags.map((t) => (
                <span key={t} className="pending-tag" onClick={() => togglePendingTag(t)}>
                  {t} ×
                </span>
              ))}
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
                <ComposerRuntimeSelector workers={workers} value={runtimeTarget} onChange={setRuntimeTarget} />
                <span className="composer-runtime-label" title="Selected runtime target">
                  {runtimeTargetLabel(runtimeTarget, workers)}
                </span>
                <button title="Mention" onClick={() => setInput((v) => v + '@')}>@</button>
                <button className={pendingTags.length ? 'active' : ''} title="Tag" onClick={() => setShowTagPicker((s) => !s)}>#</button>
              </div>
              <button className="composer-send" onClick={handleSend} disabled={!input.trim()}>↑</button>
            </div>
            {showTagPicker && (
              <div className="tag-picker-popover">
                {TAG_SUGGESTIONS.map((tag) => (
                  <button
                    key={tag}
                    className={`tag-picker-option ${pendingTags.includes(tag) ? 'selected' : ''}`}
                    onClick={() => togglePendingTag(tag)}
                  >
                    {tag}
                  </button>
                ))}
              </div>
            )}
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
              <button className="invite-user-button" onClick={openInviteUser}>
                + Invite user by key
              </button>
            </div>
          </div>
        )}
        {showInviteUser && (
          <div className="invite-user-modal">
            <div className="invite-user-card">
              <div className="invite-user-tabs">
                <button className={inviteStep === 'key' ? 'active' : ''} onClick={() => setInviteStep('key')}>Paste key</button>
                <button className={inviteStep === 'share' ? 'active' : ''} onClick={() => { setInviteStep('share'); setQrValue(buildQrValue()); }}>Share my key</button>
              </div>

              {inviteStep === 'key' ? (
                <>
                  <input
                    placeholder="Name (optional)"
                    value={inviteName}
                    onChange={(e) => setInviteName(e.target.value)}
                  />
                  <textarea
                    placeholder="Paste PEM public key..."
                    value={inviteKey}
                    onChange={(e) => setInviteKey(e.target.value)}
                    rows={4}
                  />
                  {authorizedKeys.length > 0 && (
                    <div className="authorized-keys">
                      {authorizedKeys.map((k) => (
                        <div
                          key={k.id}
                          className="authorized-key-option"
                          onClick={() => setInviteKey(k.public_key_pem)}
                        >
                          👤 {k.name} — {k.fingerprint.slice(0, 16)}
                        </div>
                      ))}
                    </div>
                  )}
                </>
              ) : (
                <div className="share-key-section">
                  <p>Share your public key fingerprint so others can invite you to threads.</p>
                  {me?.public_key_pem ? (
                    <>
                      <div className="fingerprint-box">{computeFingerprint(me.public_key_pem)}</div>
                      <button className="copy-btn" onClick={handleCopyFingerprint}>Copy fingerprint</button>
                      <div className="qr-placeholder">
                        <QRCode value={qrValue || buildQrValue()} size={160} />
                      </div>
                    </>
                  ) : (
                    <p className="muted">No public key configured. Set one in Settings.</p>
                  )}
                </div>
              )}

              <div className="invite-user-actions">
                <button onClick={() => setShowInviteUser(false)}>Cancel</button>
                {inviteStep === 'key' && (
                  <button onClick={handleInviteUser} disabled={!inviteKey.trim()}>
                    Invite
                  </button>
                )}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function QRCode({ value, size }: { value: string; size: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    canvas.width = size;
    canvas.height = size;
    ctx.fillStyle = '#fff';
    ctx.fillRect(0, 0, size, size);
    const cells = drawQRModules(value, size);
    const cellSize = size / cells;
    ctx.fillStyle = '#000';
    for (let y = 0; y < cells; y++) {
      for (let x = 0; x < cells; x++) {
        if (qrModule(value, x, y, cells)) {
          ctx.fillRect(Math.floor(x * cellSize), Math.floor(y * cellSize), Math.ceil(cellSize), Math.ceil(cellSize));
        }
      }
    }
  }, [value, size]);
  return <canvas ref={canvasRef} style={{ width: size, height: size }} />;
}

function drawQRModules(value: string, size: number): number {
  const len = value.length;
  const cells = Math.max(21, Math.ceil(Math.sqrt(len * 8)));
  return Math.min(cells, Math.floor(size / 4));
}

function qrModule(value: string, x: number, y: number, cells: number): boolean {
  const index = (y * cells + x) % Math.max(1, value.length * 8);
  const byteIndex = Math.floor(index / 8);
  const bitIndex = index % 8;
  if (byteIndex >= value.length) return false;
  return ((value.charCodeAt(byteIndex) >> bitIndex) & 1) === 1;
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
