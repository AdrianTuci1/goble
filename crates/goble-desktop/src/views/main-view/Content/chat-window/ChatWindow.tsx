import { useRef, useState, useCallback, createContext, useContext, useMemo } from 'react';
import { Info } from 'lucide-react';
import Chat, { type ChatHandle } from './chat/Chat';
import Composer, { type ComposerHandle } from './composer/Composer';
import { useMainViewStore } from '../../store/mainViewStore';
import { useChatStore } from './store/chatStore';
import './ChatWindow.css';

export interface ChatApi {
  setRenderMode: (mode: string | null) => void;
  setComposerMode: (mode: 'default' | 'inline') => void;
  getComposerMode: () => 'default' | 'inline';
  setAutoApprove: (value: boolean) => void;
  getAutoApprove: () => boolean;
  stop: () => void;
}

const ChatApiContext = createContext<ChatApi | null>(null);

export function useChatApi() {
  return useContext(ChatApiContext);
}

export default function ChatWindow() {
  const { toggleRight, openRight } = useMainViewStore();
  const { activeConversationId, messagesByChat } = useChatStore();
  const messages = activeConversationId ? messagesByChat[activeConversationId] || [] : [];
  const empty = !activeConversationId && messages.length === 0;

  const chatRef = useRef<ChatHandle>(null);
  const composerRef = useRef<ComposerHandle>(null);

  const [renderMode, setRenderModeState] = useState<string | null>(null);
  const [composerMode, setComposerModeState] = useState<'default' | 'inline'>('default');
  const [autoApprove, setAutoApproveState] = useState(false);

  const setRenderMode = useCallback((mode: string | null) => {
    setRenderModeState(mode);
  }, []);

  const setComposerMode = useCallback((mode: 'default' | 'inline') => {
    setComposerModeState(mode);
  }, []);

  const setAutoApprove = useCallback((value: boolean) => {
    setAutoApproveState(value);
  }, []);

  const stop = useCallback(() => {
    chatRef.current?.stop();
  }, []);

  const api = useMemo<ChatApi>(
    () => ({
      setRenderMode,
      setComposerMode,
      getComposerMode: () => composerMode,
      setAutoApprove,
      getAutoApprove: () => autoApprove,
      stop,
    }),
    [setRenderMode, setComposerMode, composerMode, setAutoApprove, autoApprove, stop],
  );

  function toggleInfo() {
    if (activeConversationId) {
      openRight('info');
    } else {
      toggleRight();
    }
  }

  const showRunningBar = renderMode !== null && composerMode === 'default';

  return (
    <ChatApiContext.Provider value={api}>
      <div className={`chat-window ${empty ? 'empty' : ''}`}>
        <div className="chat-header">
          <div className="chat-header-actions">
            <button className="header-btn info-btn" title="Info" onClick={toggleInfo}>
              <Info size={14} />
            </button>
          </div>
        </div>

        <Chat ref={chatRef} />

        <div className={`composer-status-bar ${showRunningBar ? 'active' : ''}`}>
          <div className="running-label">
            <span className="running-dot" />
            Running...
          </div>
          <div className="running-actions">
            <button
              className={`running-btn fast-forward ${autoApprove ? 'active' : ''}`}
              title="Auto approve"
              aria-label="Auto approve"
              onClick={() => setAutoApprove(!autoApprove)}
            >
              ⏩
            </button>
            <button className="running-btn stop" title="Stop" aria-label="Stop" onClick={stop}>
              ⏹
            </button>
          </div>
        </div>

        <Composer ref={composerRef} mode={composerMode} />
      </div>
    </ChatApiContext.Provider>
  );
}
