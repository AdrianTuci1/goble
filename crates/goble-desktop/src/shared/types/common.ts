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

export interface McpSearchResult {
  id: string;
  name: string;
  source: string;
  description?: string;
  installs: number;
}

export interface McpTool {
  name: string;
  description?: string;
  input_schema?: unknown;
}

export interface LlmProvider {
  id: string;
  name: string;
  defaultModel: string;
  defaultBaseUrl?: string;
}

export const LLM_PROVIDERS: LlmProvider[] = [
  { id: 'openai', name: 'OpenAI', defaultModel: 'gpt-4o-mini' },
  { id: 'anthropic', name: 'Anthropic', defaultModel: 'claude-3-5-sonnet-20241022' },
  { id: 'openrouter', name: 'OpenRouter', defaultModel: 'openai/gpt-4o-mini' },
  { id: 'local', name: 'Local / Custom', defaultModel: '' },
];

export interface LlmSetting {
  provider: string;
  api_key: string;
  base_url?: string | null;
  model: string;
  temperature?: number;
}

export interface InstallWorkerResult {
  platform: {
    os: string;
    arch: string;
    family: string;
  };
  asset_url: string;
  install_log: string;
}

export interface LogEntry {
  id: string;
  timestamp: string;
  message: string;
}

export interface FlowMeta {
  createdBy: string;
  integrations: string[];
  cron: string;
}

export interface FlowInfo {
  id: string;
  title: string;
  meta: FlowMeta;
}

export interface ClusterIdentityInfo {
  cluster_name: string;
  ca_cert_pem: string;
  device_serial: string;
}

export interface HarnessEventPayload {
  chat_id: string;
  event: {
    type: 'AssistantDelta' | 'ToolCallStarted' | 'ToolCallFinished' | 'ToolCallError' | 'Done' | 'Error';
    payload?: unknown;
    id?: string;
    name?: string;
    arguments?: Record<string, unknown>;
    result?: string;
    message?: string;
  };
}

export type ThemeName = 'dark' | 'light' | 'midnight';
export type FontName = 'system' | 'mono' | 'serif';
export type RadiusName = 'sharp' | 'default' | 'rounded';
export type DensityName = 'compact' | 'default' | 'spacious';

export interface DesignSystem {
  theme: ThemeName;
  accent: 'blue' | 'green' | 'purple' | 'orange';
  font: FontName;
  density: DensityName;
  radius: RadiusName;
}

export const DEFAULT_DESIGN: DesignSystem = {
  theme: 'dark',
  accent: 'blue',
  font: 'system',
  density: 'default',
  radius: 'default',
};

export const accentColorMap: Record<DesignSystem['accent'], string> = {
  blue: '#2563eb',
  green: '#22c55e',
  purple: '#8b5cf6',
  orange: '#f97316',
};
