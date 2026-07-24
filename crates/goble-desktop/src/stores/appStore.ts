import { create } from 'zustand';

export interface WorkerInfo {
  id: string;
  name: string;
  url: string;
  paired: boolean;
}

export interface Conversation {
  id: string;
  title: string;
  updated_at: string;
}

export interface ChatMessage {
  id: string;
  role: string;
  content: string;
  created_at: string;
}

export interface AgentInfo {
  id: string;
  name: string;
  spec: {
    id: { 0: string };
    name: string;
    description: string;
    prompt: string;
    tools: string[];
    triggers: unknown[];
    mcp_ids: string[];
  };
  created_at: string;
  updated_at: string;
}

export interface WorkflowStep {
  id: string;
  name: string;
  agent_id: { 0: string };
  input_template: string;
  depends_on: string[];
}

export interface WorkflowInfo {
  id: string;
  name: string;
  description: string;
  steps: WorkflowStep[];
  trigger: unknown;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface TeamInfo {
  id: string;
  name: string;
  metadata: string;
  created_at: string;
  members: string[];
}

export interface ExecutionInfo {
  id: string;
  agent_id?: string | null;
  worker_id?: string | null;
  status: string;
  trace: unknown;
  started_at: string;
  finished_at?: string | null;
}

export interface VaultSecretInfo {
  key: string;
  updated_at: string;
}

export interface LogEntry {
  id: string;
  timestamp: string;
  message: string;
}

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
  isWorkflowDrawerOpen: boolean;
  isSettingsOpen: boolean;
  setWorkers: (workers: WorkerInfo[]) => void;
  setLogs: (logs: LogEntry[]) => void;
  addLog: (message: string) => void;
  setConversations: (conversations: Conversation[]) => void;
  addConversation: (conversation: Conversation) => void;
  setMessages: (chatId: string, messages: ChatMessage[]) => void;
  addMessage: (chatId: string, message: ChatMessage) => void;
  updateMessage: (chatId: string, messageId: string, content: string) => void;
  setActiveConversation: (id: string | null) => void;
  setAgents: (agents: AgentInfo[]) => void;
  addAgent: (agent: AgentInfo) => void;
  removeAgent: (id: string) => void;
  setWorkflows: (workflows: WorkflowInfo[]) => void;
  addWorkflow: (workflow: WorkflowInfo) => void;
  removeWorkflow: (id: string) => void;
  setTeams: (teams: TeamInfo[]) => void;
  addTeam: (team: TeamInfo) => void;
  setExecutions: (executions: ExecutionInfo[]) => void;
  setVaultSecrets: (secrets: VaultSecretInfo[]) => void;
  toggleWorkflowDrawer: () => void;
  setSettingsOpen: (open: boolean) => void;
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
  isWorkflowDrawerOpen: false,
  isSettingsOpen: false,
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
  updateMessage: (chatId, messageId, content) =>
    set((state) => ({
      messages: {
        ...state.messages,
        [chatId]: (state.messages[chatId] || []).map((m) =>
          m.id === messageId ? { ...m, content } : m
        ),
      },
    })),
  setActiveConversation: (id) => set({ activeConversationId: id }),
  setAgents: (agents) => set({ agents }),
  addAgent: (agent) => set((state) => ({ agents: [agent, ...state.agents] })),
  removeAgent: (id) => set((state) => ({
    agents: state.agents.filter((a) => a.id !== id),
  })),
  setWorkflows: (workflows) => set({ workflows }),
  addWorkflow: (workflow) => set((state) => ({ workflows: [workflow, ...state.workflows] })),
  removeWorkflow: (id) => set((state) => ({
    workflows: state.workflows.filter((w) => w.id !== id),
  })),
  setTeams: (teams) => set({ teams }),
  addTeam: (team) => set((state) => ({ teams: [team, ...state.teams] })),
  setExecutions: (executions) => set({ executions }),
  setVaultSecrets: (vaultSecrets) => set({ vaultSecrets }),
  toggleWorkflowDrawer: () =>
    set((state) => ({ isWorkflowDrawerOpen: !state.isWorkflowDrawerOpen })),
  setSettingsOpen: (open) => set({ isSettingsOpen: open }),
}));
