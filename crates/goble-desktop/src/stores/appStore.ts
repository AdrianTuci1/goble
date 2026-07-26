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
  provider?: string | null;
  model?: string | null;
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

export interface McpServerSummary {
  id: string;
  name: string;
  source: string;
  source_value?: string | null;
  capabilities: string[];
  auth_required: boolean;
  discovered_tools: string[];
  secret_ids: string[];
  enabled_tools: string[];
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
  mcpServers: McpServerSummary[];
  isWorkflowDrawerOpen: boolean;
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
  removeAgent: (id: string) => void;
  setWorkflows: (workflows: WorkflowInfo[]) => void;
  addWorkflow: (workflow: WorkflowInfo) => void;
  removeWorkflow: (id: string) => void;
  setTeams: (teams: TeamInfo[]) => void;
  addTeam: (team: TeamInfo) => void;
  setExecutions: (executions: ExecutionInfo[]) => void;
  setVaultSecrets: (secrets: VaultSecretInfo[]) => void;
  setMcpServers: (servers: McpServerSummary[]) => void;
  addMcpServer: (server: McpServerSummary) => void;
  removeMcpServer: (id: string) => void;
  updateMcpServer: (server: McpServerSummary) => void;
  toggleWorkflowDrawer: () => void;
}

export type { AppState };

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
  isWorkflowDrawerOpen: false,
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
              {
                id: messageId,
                role: 'assistant',
                content: newContent,
                created_at: new Date().toISOString(),
              },
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
  removeAgent: (id) =>
    set((state) => ({
      agents: state.agents.filter((a) => a.id !== id),
    })),
  setWorkflows: (workflows) => set({ workflows }),
  addWorkflow: (workflow) =>
    set((state) => ({ workflows: [workflow, ...state.workflows] })),
  removeWorkflow: (id) =>
    set((state) => ({
      workflows: state.workflows.filter((w) => w.id !== id),
    })),
  setTeams: (teams) => set({ teams }),
  addTeam: (team) => set((state) => ({ teams: [team, ...state.teams] })),
  setExecutions: (executions) => set({ executions }),
  setVaultSecrets: (vaultSecrets) => set({ vaultSecrets }),
  setMcpServers: (mcpServers) => set({ mcpServers }),
  addMcpServer: (server) =>
    set((state) => ({ mcpServers: [server, ...state.mcpServers] })),
  removeMcpServer: (id) =>
    set((state) => ({
      mcpServers: state.mcpServers.filter((s) => s.id !== id),
    })),
  updateMcpServer: (server) =>
    set((state) => ({
      mcpServers: state.mcpServers.map((s) =>
        s.id === server.id ? server : s
      ),
    })),
  toggleWorkflowDrawer: () =>
    set((state) => ({ isWorkflowDrawerOpen: !state.isWorkflowDrawerOpen })),
}));
