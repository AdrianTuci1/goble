import { useEffect, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useChatStore } from '../store/chatStore';
import { useMainViewStore } from '../../../store/mainViewStore';
import type { ChatMessage } from '../../../../../shared';
import { createChat, onChatUpdated, onAgentLog, onAgentStarted, onAgentFinished, runAgent } from '../../../../../shared';
import { uid, getInitials } from '../../../../../shared';
import { agentsData, type Agent } from '../../../data/agentsData';
import { flowsData, type FlowInfo } from '../../../data/flowsData';
import './Chat.css';

export default function Chat() {
  const [params] = useSearchParams();
  const agentId = params.get('agent');
  const flowId = params.get('flow');
  const messagesEndRef = useRef<HTMLDivElement>(null);

  const { activeConversationId, conversations, messagesByChat, addConversation, setActiveConversationId, setMessages, addMessage, updateMessage, typing, setTyping, setActiveTrace } = useChatStore();
  const messages = activeConversationId ? messagesByChat[activeConversationId] || [] : [];
  const activeConversation = conversations.find((c) => c.id === activeConversationId);

  const { selectAgent, selectFlow, openRight } = useMainViewStore();

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  useEffect(() => {
    if (!activeConversationId && conversations.length > 0) {
      setActiveConversationId(conversations[0].id);
    }
  }, [activeConversationId, conversations]);

  useEffect(() => {
    function onNewChat() {
      handleNewChat();
    }
    window.addEventListener('goble:new-chat', onNewChat);
    return () => window.removeEventListener('goble:new-chat', onNewChat);
  }, []);

  useEffect(() => {
    if (agentId) {
      const agent = agentsData.find((a: Agent) => a.id === agentId);
      if (agent) startAgentChat(agent.id, agent.name);
    } else if (flowId) {
      startFlowChat(flowId);
    }
  }, [agentId, flowId]);

  useEffect(() => {
    const unlistenPromises: Promise<(() => void) | null>[] = [];
    async function setup() {
      const promises = [
        onChatUpdated((event) => {
          const payload = event.payload as { chat_id?: string; message?: ChatMessage };
          if (payload.chat_id && payload.message) addMessage(payload.chat_id, payload.message);
        }),
        onAgentLog((event) => {
          const payload = event.payload as { trace_id?: string; message?: string };
          if (payload.trace_id && payload.message && activeConversationId) {
            updateMessage(activeConversationId, payload.trace_id, (prev) => prev + payload.message);
          }
        }),
        onAgentStarted((event) => {
          const payload = event.payload as { trace_id?: string; agent_id?: string };
          if (payload.trace_id && activeConversationId) {
            setActiveTrace(payload.trace_id);
            addMessage(activeConversationId, { id: payload.trace_id, role: 'assistant', content: '', created_at: new Date().toISOString() });
            setTyping(true);
            openRight('history');
          }
        }),
        onAgentFinished((event) => {
          const payload = event.payload as { trace_id?: string; status?: string };
          if (payload.trace_id && activeConversationId) {
            setTyping(false);
            updateMessage(activeConversationId, payload.trace_id, (prev) => prev || payload.status || 'Done');
          }
        }),
      ];
      for (const promise of promises) {
        unlistenPromises.push(
          promise.catch((err) => {
            console.error('Failed to register listener', err);
            return null;
          })
        );
      }
    }
    setup();
    return () => {
      unlistenPromises.forEach(async (p) => (await p)?.());
    };
  }, [activeConversationId]);

  async function handleNewChat() {
    try {
      const chatId = await createChat('New chat', 'openai', 'gpt-4o-mini');
      addConversation({ id: chatId, title: 'New chat', provider: 'openai', model: 'gpt-4o-mini', updated_at: new Date().toISOString() });
      setActiveConversationId(chatId);
      setMessages(chatId, []);
    } catch {
      const chatId = uid();
      addConversation({ id: chatId, title: 'New chat', provider: 'openai', model: 'gpt-4o-mini', updated_at: new Date().toISOString() });
      setActiveConversationId(chatId);
      setMessages(chatId, []);
    }
  }

  async function startAgentChat(agentId: string, title: string) {
    if (!activeConversationId) await handleNewChat();
    const chatId = activeConversationId || uid();
    selectAgent(agentId);
    openRight('info');
    addMessage(chatId, { id: uid(), role: 'system', content: `Started ${title} agent.`, created_at: new Date().toISOString() });
    try {
      await runAgent('local', chatId, agentId, 'start');
    } catch {
      setTimeout(() => simulateReply(chatId, agentId), 300);
    }
  }

  async function startFlowChat(flowId: string) {
    if (!activeConversationId) await handleNewChat();
    const chatId = activeConversationId || uid();
    selectFlow(flowId);
    openRight('info');
    simulateReply(chatId, flowId);
  }

  function simulateReply(chatId: string, id: string) {
    const agent = agentsData.find((a: Agent) => a.id === id);
    if (agent) {
      addMessage(chatId, { id: uid(), role: 'assistant', content: agent.description, created_at: new Date().toISOString() });
    }
    const flow = flowsData.find((f: FlowInfo) => f.id === id);
    if (flow) {
      addMessage(chatId, {
        id: uid(),
        role: 'assistant',
        content: `Flow: **${flow.title}**\nCreated by ${flow.meta.createdBy}\nIntegrations: ${flow.meta.integrations.join(', ')}\nSchedule: ${flow.meta.cron}`,
        created_at: new Date().toISOString(),
      });
    }
  }

  if (!activeConversationId) {
    return (
      <div className="chat-view empty">
        <div className="chat-welcome">
          <div className="chat-welcome-logo">G</div>
          <h2>Welcome to Goble</h2>
          <p>Choose an agent from the sidebar or start a new chat.</p>
          <button className="ui-btn ui-btn-primary" onClick={handleNewChat}>Start new chat</button>
        </div>
      </div>
    );
  }

  return (
    <div className="chat-view">
      <div className="chat-header">
        <div className="chat-header-info">
          <span className="chat-header-title">{activeConversation?.title || 'Chat'}</span>
          <span className="chat-header-meta">{activeConversation?.model || 'gpt-4o-mini'}</span>
        </div>
      </div>
      <div className="chat-messages">
        {messages.length === 0 && (
          <div className="chat-empty"><p>Send a message to start the conversation.</p></div>
        )}
        {messages.map((m) => <MessageBubble key={m.id} message={m} />)}
        {typing && (
          <div className="message assistant">
            <div className="message-avatar" style={{ background: '#9ca3af' }}>AI</div>
            <div className="message-content">
              <div className="typing-indicator"><span /><span /><span /></div>
            </div>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>
    </div>
  );
}

function MessageBubble({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';
  const author = isUser ? 'You' : isSystem ? 'System' : 'Assistant';
  const color = isUser ? '#22c55e' : isSystem ? '#6b7280' : '#9ca3af';
  return (
    <div className={`message ${isUser ? 'user' : isSystem ? 'system' : 'assistant'}`}>
      <div className="message-avatar" style={{ background: color }} title={author}>
        {getInitials(author)}
      </div>
      <div className="message-body">
        <div className="message-meta"><span className="message-author">{author}</span></div>
        <div className="message-content"><RichText text={message.content} /></div>
      </div>
    </div>
  );
}

function RichText({ text }: { text: string }) {
  if (!text) return null;
  if (text.startsWith('```') || text.includes('`')) return <pre className="code-block">{text}</pre>;
  return <div className="rich-text" dangerouslySetInnerHTML={{ __html: simpleHtml(text) }} />;
}

function simpleHtml(md: string) {
  return md
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/\n/g, '<br />');
}
