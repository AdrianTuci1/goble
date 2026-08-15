import { getUserProfile } from '../tauri/api';
import { create } from 'zustand';
import type {
  ThreadSummary,
  ThreadMessageSummary,
  Participant,
  UserProfile,
  AgentInfo,
  WorkerInfo,
  Conversation,
  ChatMessage,
  WorkflowInfo,
  TeamInfo,
  ExecutionInfo,
  VaultSecretInfo,
  McpServerSummary,
  RuntimeState,
  ToolResultEvent,
} from '../tauri/api';

export interface DesignSystem {
  theme: 'dark' | 'light' | 'midnight';
  accent: 'blue' | 'green' | 'purple' | 'orange';
  font: 'system' | 'mono' | 'serif';
  density: 'compact' | 'default' | 'spacious';
  radius: 'sharp' | 'default' | 'rounded';
}

export interface LogEntry {
  id: string;
  timestamp: string;
  message: string;
}

export interface FlowInfo {
  id: string;
  title: string;
  meta: { createdBy: string; integrations: string[]; cron: string };
}

export type {
  ThreadSummary,
  ThreadMessageSummary,
  Participant,
  UserProfile,
  AgentInfo,
  WorkerInfo,
  Conversation,
  ChatMessage,
  WorkflowInfo,
  TeamInfo,
  ExecutionInfo,
  VaultSecretInfo,
  McpServerSummary,
};

export const DEFAULT_DESIGN: DesignSystem = {
  theme: 'dark',
  accent: 'blue',
  font: 'system',
  density: 'default',
  radius: 'default',
};

interface AppState {
  workers: WorkerInfo[];
  logs: LogEntry[];
  conversations: Conversation[];
  activeConversationId: string | null;
  messages: Record<string, ChatMessage[]>;
  agents: AgentInfo[];
  workflows: WorkflowInfo[];
  teams: TeamInfo[];
  executions: ExecutionInfo[];
  vaultSecrets: VaultSecretInfo[];
  mcpServers: McpServerSummary[];
  flows: FlowInfo[];
  agentStates: Record<string, RuntimeState>;
  agentToolResults: Record<string, ToolResultEvent[]>;
  selectedTraceId: string | null;

  threads: ThreadSummary[];
  threadMessages: Record<string, ThreadMessageSummary[]>;
  activeThreadId: string | null;
  threadParticipants: Record<string, Participant[]>;
  replyToMessageId: string | null;
  userProfile: UserProfile | null;
  pendingTags: string[];
  participantsPanelOpen: boolean;
  threadRepliesOpen: Record<string, boolean>;
  threadEmojiPickerForMessageId: string | null;
  threadPendingRuns: Record<string, { agentId: string; name: string }[]>;

  selectedFlowId: string | null;
  isWorkflowDrawerOpen: boolean;
  design: DesignSystem;
  rightSidebarOpen: boolean;
  rightSidebarTab: 'info' | 'history';
  selectedAgentId: string | null;
  historyDetailId: string | null;
  navigateFn: (path: string) => void;

  setWorkers: (workers: WorkerInfo[]) => void;
  setLogs: (logs: LogEntry[]) => void;
  addLog: (message: string) => void;
  setConversations: (conversations: Conversation[]) => void;
  addConversation: (conversation: Conversation) => void;
  updateConversation: (id: string, updates: Partial<Conversation>) => void;
  setActiveConversation: (id: string | null) => void;
  setMessages: (chatId: string, messages: ChatMessage[]) => void;
  addMessage: (chatId: string, message: ChatMessage) => void;
  updateMessage: (chatId: string, messageId: string, content: string | ((prev: string) => string)) => void;
  setAgents: (agents: AgentInfo[]) => void;
  addAgent: (agent: AgentInfo) => void;
  updateAgent: (agent: AgentInfo) => void;
  removeAgent: (id: string) => void;
  setWorkflows: (workflows: WorkflowInfo[]) => void;
  addWorkflow: (workflow: WorkflowInfo) => void;
  removeWorkflow: (id: string) => void;
  setTeams: (teams: TeamInfo[]) => void;
  addTeam: (team: TeamInfo) => void;
  setExecutions: (executions: ExecutionInfo[]) => void;
  setVaultSecrets: (secrets: VaultSecretInfo[]) => void;
  setMcpServers: (mcpServers: McpServerSummary[]) => void;
  addMcpServer: (server: McpServerSummary) => void;
  removeMcpServer: (id: string) => void;
  updateMcpServer: (server: McpServerSummary) => void;
  setFlows: (flows: FlowInfo[]) => void;
  setSelectedFlowId: (id: string | null) => void;
  toggleWorkflowDrawer: () => void;
  setDesign: (design: DesignSystem) => void;
  setRightSidebarOpen: (open: boolean) => void;
  setRightSidebarTab: (tab: 'info' | 'history') => void;
  setSelectedAgentId: (id: string | null) => void;
  setHistoryDetailId: (id: string | null) => void;
  setNavigateFn: (fn: (path: string) => void) => void;
  setAgentState: (traceId: string, state: RuntimeState) => void;
  addAgentToolResult: (traceId: string, result: ToolResultEvent) => void;
  setSelectedTraceId: (id: string | null) => void;

  setThreads: (threads: ThreadSummary[]) => void;
  addThread: (thread: ThreadSummary) => void;
  updateThread: (id: string, updates: Partial<ThreadSummary>) => void;
  setActiveThreadId: (id: string | null) => void;
  setThreadMessages: (threadId: string, messages: ThreadMessageSummary[]) => void;
  addThreadMessage: (threadId: string, message: ThreadMessageSummary) => void;
  updateThreadMessage: (threadId: string, messageId: string, content: string) => void;
  deleteThreadMessage: (threadId: string, messageId: string) => void;
  markThreadRead: (threadId: string, timestamp: string) => void;
  setThreadParticipants: (threadId: string, participants: Participant[]) => void;
  addThreadParticipantLocal: (threadId: string, participant: Participant) => void;
  removeThreadParticipantLocal: (threadId: string, participantId: string) => void;
  setReplyToMessageId: (id: string | null) => void;
  setUserProfile: (profile: UserProfile | null) => void;
  setPendingTags: (tags: string[]) => void;
  togglePendingTag: (tag: string) => void;
  setParticipantsPanelOpen: (open: boolean) => void;
  setThreadRepliesOpen: (messageId: string, open: boolean) => void;
  setThreadEmojiPickerForMessageId: (id: string | null) => void;
  addThreadPendingRun: (threadId: string, agentId: string, name: string) => void;
  removeThreadPendingRun: (threadId: string, agentId: string) => void;
}

export const useStore = create<AppState>((set) => ({
  workers: [],
  logs: [],
  conversations: [],
  activeConversationId: null,
  messages: {},
  agents: [],
  workflows: [],
  teams: [],
  executions: [],
  vaultSecrets: [],
  mcpServers: [],
  flows: [],
  agentStates: {},
  agentToolResults: {},
  selectedTraceId: null,

  threads: [],
  threadMessages: {},
  activeThreadId: null,
  threadParticipants: {},
  replyToMessageId: null,
  userProfile: null,
  pendingTags: [],
  participantsPanelOpen: false,
  threadRepliesOpen: {},
  threadEmojiPickerForMessageId: null,
  threadPendingRuns: {},

  selectedFlowId: null,
  isWorkflowDrawerOpen: false,
  design: DEFAULT_DESIGN,
  rightSidebarOpen: false,
  rightSidebarTab: 'info',
  selectedAgentId: null,
  historyDetailId: null,
  navigateFn: () => {},

  setWorkers: (workers) => set({ workers }),
  setLogs: (logs) => set({ logs }),
  addLog: (message) =>
    set((state) => ({
      logs: [
        ...state.logs,
        { id: `${Date.now()}`, timestamp: new Date().toISOString(), message },
      ],
    })),
  setConversations: (conversations) => set({ conversations }),
  addConversation: (conversation) =>
    set((state) => ({ conversations: [conversation, ...state.conversations] })),
  updateConversation: (id, updates) =>
    set((state) => ({
      conversations: state.conversations.map((c) =>
        c.id === id ? { ...c, ...updates } : c
      ),
    })),
  setActiveConversation: (id) => set({ activeConversationId: id }),
  setMessages: (chatId, messages) =>
    set((state) => ({
      messages: { ...state.messages, [chatId]: messages },
    })),
  addMessage: (chatId, message) =>
    set((state) => ({
      messages: {
        ...state.messages,
        [chatId]: [...(state.messages[chatId] || []), message],
      },
    })),
  updateMessage: (chatId, messageId, contentOrUpdater) =>
    set((state) => {
      const list = state.messages[chatId] || [];
      const existing = list.find((m) => m.id === messageId);
      const newContent =
        typeof contentOrUpdater === 'function'
          ? (contentOrUpdater as (prev: string) => string)(existing?.content ?? '')
          : contentOrUpdater;
      if (!existing) {
        return {
          messages: {
            ...state.messages,
            [chatId]: [
              ...list,
              { id: messageId, role: 'assistant', content: newContent, created_at: new Date().toISOString() },
            ],
          },
        };
      }
      return {
        messages: {
          ...state.messages,
          [chatId]: list.map((m) =>
            m.id === messageId ? { ...m, content: newContent } : m
          ),
        },
      };
    }),
  setAgents: (agents) => set({ agents }),
  addAgent: (agent) => set((state) => ({ agents: [agent, ...state.agents] })),
  updateAgent: (agent) =>
    set((state) => ({
      agents: state.agents.map((a) => (a.id === agent.id ? agent : a)),
    })),
  removeAgent: (id) =>
    set((state) => ({ agents: state.agents.filter((a) => a.id !== id) })),
  setWorkflows: (workflows) => set({ workflows }),
  addWorkflow: (workflow) =>
    set((state) => ({ workflows: [workflow, ...state.workflows] })),
  removeWorkflow: (id) =>
    set((state) => ({ workflows: state.workflows.filter((w) => w.id !== id) })),
  setTeams: (teams) => set({ teams }),
  addTeam: (team) => set((state) => ({ teams: [team, ...state.teams] })),
  setExecutions: (executions) => set({ executions }),
  setVaultSecrets: (vaultSecrets) => set({ vaultSecrets }),
  setMcpServers: (mcpServers) => set({ mcpServers }),
  addMcpServer: (server) =>
    set((state) => ({ mcpServers: [server, ...state.mcpServers] })),
  removeMcpServer: (id) =>
    set((state) => ({ mcpServers: state.mcpServers.filter((s) => s.id !== id) })),
  updateMcpServer: (server) =>
    set((state) => ({
      mcpServers: state.mcpServers.map((s) => (s.id === server.id ? server : s)),
    })),
  setFlows: (flows) => set({ flows }),
  setSelectedFlowId: (selectedFlowId) => set({ selectedFlowId }),
  toggleWorkflowDrawer: () =>
    set((state) => ({ isWorkflowDrawerOpen: !state.isWorkflowDrawerOpen })),
  setDesign: (design) => set({ design }),
  setRightSidebarOpen: (rightSidebarOpen) => set({ rightSidebarOpen }),
  setRightSidebarTab: (rightSidebarTab) => set({ rightSidebarTab }),
  setSelectedAgentId: (selectedAgentId) => set({ selectedAgentId }),
  setHistoryDetailId: (historyDetailId) => set({ historyDetailId }),
  setNavigateFn: (navigateFn) => set({ navigateFn }),
  setAgentState: (traceId, state) =>
    set((s) => ({ agentStates: { ...s.agentStates, [traceId]: state } })),
  addAgentToolResult: (traceId, result) =>
    set((s) => ({
      agentToolResults: {
        ...s.agentToolResults,
        [traceId]: [...(s.agentToolResults[traceId] || []), result],
      },
    })),
  setSelectedTraceId: (selectedTraceId) => set({ selectedTraceId }),

  setThreads: (threads) => set({ threads }),
  addThread: (thread) => set((state) => ({ threads: [thread, ...state.threads] })),
  updateThread: (id, updates) =>
    set((state) => ({
      threads: state.threads.map((t) => (t.id === id ? { ...t, ...updates } : t)),
    })),
  setActiveThreadId: (activeThreadId) => set({ activeThreadId }),
  setThreadMessages: (threadId, messages) =>
    set((state) => ({
      threadMessages: { ...state.threadMessages, [threadId]: messages },
    })),
  addThreadMessage: (threadId, message) =>
    set((state) => ({
      threadMessages: {
        ...state.threadMessages,
        [threadId]: [...(state.threadMessages[threadId] || []), message],
      },
    })),
  updateThreadMessage: (threadId, messageId, content) =>
    set((state) => {
      const list = state.threadMessages[threadId] || [];
      return {
        threadMessages: {
          ...state.threadMessages,
          [threadId]: list.map((m) =>
            m.id === messageId ? { ...m, content, updated_at: new Date().toISOString() } : m
          ),
        },
      };
    }),
  deleteThreadMessage: (threadId, messageId) =>
    set((state) => ({
      threadMessages: {
        ...state.threadMessages,
        [threadId]: (state.threadMessages[threadId] || []).filter((m) => m.id !== messageId),
      },
    })),
  markThreadRead: (threadId, timestamp) =>
    set((state) => ({
      threads: state.threads.map((t) =>
        t.id === threadId ? { ...t, last_read_at: timestamp } : t
      ),
    })),
  setThreadParticipants: (threadId, participants) =>
    set((state) => ({
      threadParticipants: { ...state.threadParticipants, [threadId]: participants },
    })),
  addThreadParticipantLocal: (threadId, participant) =>
    set((state) => {
      const existing = state.threadParticipants[threadId] || [];
      const id = `${participant.kind}:${participant.id}`;
      if (existing.some((p) => `${p.kind}:${p.id}` === id)) return state;
      return {
        threadParticipants: {
          ...state.threadParticipants,
          [threadId]: [...existing, participant],
        },
      };
    }),
  removeThreadParticipantLocal: (threadId, participantId) =>
    set((state) => ({
      threadParticipants: {
        ...state.threadParticipants,
        [threadId]: (state.threadParticipants[threadId] || []).filter(
          (p) => `${p.kind}:${p.id}` !== participantId
        ),
      },
    })),
  setReplyToMessageId: (replyToMessageId) => set({ replyToMessageId }),
  setUserProfile: (userProfile) => set({ userProfile }),
  setPendingTags: (pendingTags) => set({ pendingTags }),
  togglePendingTag: (tag) =>
    set((state) => ({
      pendingTags: state.pendingTags.includes(tag)
        ? state.pendingTags.filter((t) => t !== tag)
        : [...state.pendingTags, tag],
    })),
  setParticipantsPanelOpen: (participantsPanelOpen) => set({ participantsPanelOpen }),
  setThreadRepliesOpen: (messageId, open) =>
    set((state) => ({
      threadRepliesOpen: { ...state.threadRepliesOpen, [messageId]: open },
    })),
  setThreadEmojiPickerForMessageId: (threadEmojiPickerForMessageId) => set({ threadEmojiPickerForMessageId }),
  addThreadPendingRun: (threadId, agentId, name) =>
    set((state) => {
      const existing = state.threadPendingRuns[threadId] || [];
      if (existing.some((r) => r.agentId === agentId)) return state;
      return {
        threadPendingRuns: {
          ...state.threadPendingRuns,
          [threadId]: [...existing, { agentId, name }],
        },
      };
    }),
  removeThreadPendingRun: (threadId, agentId) =>
    set((state) => ({
      threadPendingRuns: {
        ...state.threadPendingRuns,
        [threadId]: (state.threadPendingRuns[threadId] || []).filter((r) => r.agentId !== agentId),
      },
    })),
}));

getUserProfile().then((profile: UserProfile | null) => {
  if (profile) useStore.setState({ userProfile: profile });
});
