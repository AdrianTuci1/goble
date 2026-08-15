import { useEffect, useRef, useState } from 'react';
import { useStore, type Participant, type ThreadMessageSummary } from '../stores/appStore';
import {
  getThreadMessages,
  postThreadMessage,
  addThreadParticipant,
  getThreadParticipants,
  onThreadMessagesUpdated,
  onThreadsUpdated,
  listThreads,
  extractMentions,
  listWorkers,
  runAgentForThreadReply,
  type WorkerInfo,
} from '../tauri/api';
import { uid, getInitials } from '../utils/designSystem';
import ComposerRuntimeSelector, { type RuntimeTarget, runtimeTargetLabel } from './ComposerRuntimeSelector';

interface ChatAreaProps {
  threadsActive?: boolean;
}

function participantToString(p: Participant): string {
  return `${p.kind}:${p.id}`;
}

export default function ChatArea({ threadsActive }: ChatAreaProps) {
  void threadsActive;
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [pendingTraceId, setPendingTraceId] = useState<string | null>(null);
  const [showMentionPicker, setShowMentionPicker] = useState(false);
  const [mentionQuery, setMentionQuery] = useState('');
  const [mentionIndex, setMentionIndex] = useState(0);
  const [workers, setWorkers] = useState<WorkerInfo[]>([]);
  const [runtimeTarget, setRuntimeTarget] = useState<RuntimeTarget>({ kind: 'auto' });

  const threads = useStore((s) => s.threads);
  const activeThreadId = useStore((s) => s.activeThreadId);
  const threadMessages = useStore((s) => s.threadMessages[activeThreadId || ''] || []);
  const threadParticipants = useStore((s) => s.threadParticipants[activeThreadId || ''] || []);
  const replyToMessageId = useStore((s) => s.replyToMessageId);
  const pendingTags = useStore((s) => s.pendingTags);
  const agents = useStore((s) => s.agents);
  const userProfile = useStore((s) => s.userProfile);
  const setThreads = useStore((s) => s.setThreads);
  const setThreadMessages = useStore((s) => s.setThreadMessages);
  const addThreadMessage = useStore((s) => s.addThreadMessage);
  const setThreadParticipants = useStore((s) => s.setThreadParticipants);
  const setActiveThreadId = useStore((s) => s.setActiveThreadId);
  const setReplyToMessageId = useStore((s) => s.setReplyToMessageId);
  const togglePendingTag = useStore((s) => s.togglePendingTag);
  const setPendingTags = useStore((s) => s.setPendingTags);
  const setParticipantsPanelOpen = useStore((s) => s.setParticipantsPanelOpen);
  const setRightSidebarOpen = useStore((s) => s.setRightSidebarOpen);
  const setRightSidebarTab = useStore((s) => s.setRightSidebarTab);

  const activeThread = threads.find((t) => t.id === activeThreadId);
  const messages = threadMessages;

  useEffect(() => {
    listThreads().then(setThreads).catch(() => {});
  }, [setThreads]);

  useEffect(() => {
    listWorkers().then(setWorkers).catch(() => {});
  }, []);

  useEffect(() => {
    if (!activeThreadId) {
      const first = threads[0];
      if (first) setActiveThreadId(first.id);
      return;
    }
    getThreadMessages(activeThreadId).then((msgs) => setThreadMessages(activeThreadId, msgs)).catch(() => {});
    getThreadParticipants(activeThreadId).then((parts) => setThreadParticipants(activeThreadId, parts)).catch(() => {});
  }, [activeThreadId, threads, setThreadMessages, setThreadParticipants, setActiveThreadId]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    let unsubs: (() => void)[] = [];
    (async () => {
      unsubs.push(await onThreadsUpdated(() => listThreads().then(setThreads).catch(() => {})));
      unsubs.push(await onThreadMessagesUpdated((event) => {
        const threadId = event.payload.thread_id;
        getThreadMessages(threadId).then((msgs) => setThreadMessages(threadId, msgs)).catch(() => {});
      }));
    })();
    return () => unsubs.forEach((u) => u());
  }, [setThreads, setThreadMessages]);

  async function handleSend() {
    if (!input.trim() || !activeThreadId) return;
    const text = input.trim();
    setInput('');
    setLoading(true);
    setPendingTraceId(null);
    try {
      const mentions = extractMentions(text);
      const message = await postThreadMessage(activeThreadId, text, {
        reply_to: replyToMessageId ?? undefined,
        tags: pendingTags,
        mentions,
      });
      addThreadMessage(activeThreadId, message);
      setReplyToMessageId(null);
      setPendingTags([]);

      const agentMentions = mentions
        .map((m) => m.replace(/^agent:/, ''))
        .filter((id) => agents.some((a) => a.id === id || a.spec.id['0'] === id));
      for (const agentId of agentMentions) {
        try {
          const traceId = `${activeThreadId}-${agentId}-${Date.now()}`;
          setPendingTraceId(traceId);
          await runAgentForThreadReply(runtimeTarget, activeThreadId, agentId, text);
        } catch {
          // Worker may be unreachable; ignore.
        }
      }
    } catch {
      // fallback: add local optimistic message
      addThreadMessage(activeThreadId, {
        id: uid(),
        thread_id: activeThreadId,
        author: userProfile ? { kind: 'user', id: userProfile.id } : { kind: 'user', id: 'me' },
        content: text,
        reply_to: replyToMessageId,
        tags: pendingTags,
        participant_mentions: [],
        reactions: [],
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      } as ThreadMessageSummary);
    } finally {
      setLoading(false);
    }
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    } else if (showMentionPicker) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setMentionIndex((i) => (i + 1) % mentionOptions.length);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setMentionIndex((i) => (i - 1 + mentionOptions.length) % mentionOptions.length);
      } else if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        insertMention(mentionOptions[mentionIndex]);
      } else if (e.key === 'Escape') {
        setShowMentionPicker(false);
      }
    }
  }

  const allMentionables: Participant[] = [
    ...threadParticipants,
    ...agents.map((a) => ({ kind: 'agent' as const, id: a.spec.id['0'] })),
  ];
  const uniqueMentionables = allMentionables.filter(
    (p, idx, arr) => arr.findIndex((x) => participantToString(x) === participantToString(p)) === idx
  );
  const mentionOptions = uniqueMentionables.filter((p) => {
    const q = mentionQuery.toLowerCase();
    return p.id.toLowerCase().includes(q) || p.kind.toLowerCase().includes(q);
  });

  function onInputChange(value: string) {
    setInput(value);
    const lastAt = value.lastIndexOf('@');
    if (lastAt >= 0 && lastAt === value.length - 1) {
      setShowMentionPicker(true);
      setMentionQuery('');
      setMentionIndex(0);
    } else if (lastAt >= 0 && !value.slice(lastAt + 1).includes(' ')) {
      setShowMentionPicker(true);
      setMentionQuery(value.slice(lastAt + 1));
      setMentionIndex(0);
    } else {
      setShowMentionPicker(false);
    }
  }

  function insertMention(p: Participant) {
    const lastAt = input.lastIndexOf('@');
    const prefix = input.slice(0, lastAt);
    const suffix = input.slice(lastAt + 1 + mentionQuery.length);
    setInput(`${prefix}@${p.kind}:${p.id}${suffix} `);
    setShowMentionPicker(false);
    inputRef.current?.focus();
  }

  async function inviteAgentToThread(agentId: string) {
    if (!activeThreadId) return;
    await addThreadParticipant(activeThreadId, { kind: 'agent', id: agentId });
    const parts = await getThreadParticipants(activeThreadId);
    setThreadParticipants(activeThreadId, parts);
  }

  if (!activeThreadId) {
    return (
      <div className="chat-view empty">
        <div className="chat-welcome">
          <div className="chat-welcome-logo">G</div>
          <h2>Welcome to Goble</h2>
          <p>Choose a thread from the sidebar or start a new conversation.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="chat-view">
      <div className="chat-header">
        <div className="chat-header-info">
          <span className="chat-header-title">{activeThread?.title || 'Thread'}</span>
          <span className="chat-header-meta">{activeThread?.kind}</span>
        </div>
        <div className="chat-header-actions">
          <button className="chat-header-btn" onClick={() => setParticipantsPanelOpen(true)} title="Participants">👤</button>
          <button className="chat-header-btn" onClick={() => { setRightSidebarOpen(true); setRightSidebarTab('info'); }} title="Info">ℹ️</button>
          <button className="chat-header-btn" onClick={() => { setRightSidebarOpen(true); setRightSidebarTab('history'); }} title="History">📜</button>
        </div>
      </div>

      <div className="chat-messages">
        {messages.length === 0 && (
          <div className="chat-empty">
            <p>Send a message to start the conversation.</p>
          </div>
        )}
        {messages.map((m) => (
          <MessageBubble key={m.id} message={m} onReply={() => setReplyToMessageId(m.id)} onMentionAgent={inviteAgentToThread} />
        ))}
        {loading && (
          <div className="message assistant">
            <div className="message-avatar" style={{ background: '#9ca3af' }}>AI</div>
            <div className="message-content">
              <div className="typing-indicator"><span /><span /><span /></div>
              {pendingTraceId && (
                <button
                  className="msg-action trace-link"
                  onClick={() => {
                    useStore.getState().setSelectedTraceId(pendingTraceId);
                    useStore.getState().navigateFn('/traces');
                  }}
                >
                  Trace
                </button>
              )}
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {replyToMessageId && (
        <div className="composer-context">
          Replying to {messages.find((m) => m.id === replyToMessageId)?.author.id}
          <button className="cancel-reply" onClick={() => setReplyToMessageId(null)}>×</button>
        </div>
      )}

      <div className="chat-composer">
        <div className="composer-row" style={{ position: 'relative' }}>
          <input
            ref={inputRef}
            className="composer-input"
            placeholder="Message..."
            value={input}
            onChange={(e) => onInputChange(e.target.value)}
            onKeyDown={handleKeyDown}
          />
          <button className="composer-send" onClick={handleSend} disabled={!input.trim() || loading}>↑</button>
          {showMentionPicker && mentionOptions.length > 0 && (
            <div className="mention-picker">
              {mentionOptions.map((p, idx) => (
                <div
                  key={participantToString(p)}
                  className={`mention-option ${idx === mentionIndex ? 'selected' : ''}`}
                  onClick={() => insertMention(p)}
                >
                  {p.kind === 'agent' ? '🤖' : '👤'} {p.id}
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="composer-toolbar">
          <div className="composer-toolbar-left">
            <ComposerRuntimeSelector workers={workers} value={runtimeTarget} onChange={setRuntimeTarget} />
            <span className="composer-runtime-label" title="Selected runtime target">
              {runtimeTargetLabel(runtimeTarget, workers)}
            </span>
            <button title="Mention" onClick={() => setInput((v) => v + '@')}>@</button>
            <button title="Attach">📎</button>
            <button title="Emoji">☺</button>
            <button title="Tag" className={pendingTags.length ? 'active' : ''} onClick={() => togglePendingTag('#todo')}>#</button>
            <button title="Format">Aa</button>
          </div>
          {loading && <button className="composer-cancel" onClick={() => setLoading(false)}>Cancel</button>}
        </div>
      </div>
    </div>
  );
}

function MessageBubble({
  message,
  onReply,
  onMentionAgent,
}: {
  message: ThreadMessageSummary;
  onReply: () => void;
  onMentionAgent: (id: string) => void;
}) {
  const isMe = message.author.kind === 'user';
  const author = message.author.id;
  const color = isMe ? '#22c55e' : message.author.kind === 'agent' ? '#10b981' : '#9ca3af';
  const initials = getInitials(author);

  return (
    <div className={`message ${isMe ? 'user' : 'assistant'}`}>
      <div className="message-avatar" style={{ background: color }} title={author}>
        {initials}
      </div>
      <div className="message-body">
        <div className="message-meta">
          <span className="message-author">{message.author.kind === 'agent' ? '🤖 ' : ''}{author}</span>
          {message.reply_to && <span className="reply-badge">↳ reply</span>}
        </div>
        <div className="message-content">
          <RichText text={message.content} onMentionAgent={onMentionAgent} />
        </div>
        {message.tags.length > 0 && (
          <div className="message-tags">
            {message.tags.map((t) => <span key={t} className="message-tag">{t}</span>)}
          </div>
        )}
        <div className="message-footer">
          <button className="msg-action reply-btn" onClick={onReply}>Reply</button>
          {message.reactions.map((r) => (
            <button key={r.participant_id + r.emoji} className="reaction">{r.emoji}</button>
          ))}
        </div>
      </div>
    </div>
  );
}

function RichText({ text, onMentionAgent }: { text: string; onMentionAgent: (id: string) => void }) {
  if (!text) return null;
  const html = text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/\u003e/g, '&gt;')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/@agent:([a-zA-Z0-9_-]+)/g, '<span class="mention-agent" data-agent="$1">@$1</span>')
    .replace(/@user:([a-zA-Z0-9_-]+)/g, '<span class="mention-user">@$1</span>')
    .replace(/\n/g, '<br />');
  return (
    <div
      className="rich-text"
      dangerouslySetInnerHTML={{ __html: html }}
      onClick={(e) => {
        const target = e.target as HTMLElement;
        if (target.classList.contains('mention-agent')) {
          onMentionAgent(target.dataset.agent || '');
        }
      }}
    />
  );
}
