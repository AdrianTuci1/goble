import { invoke } from '@tauri-apps/api/core';
import { listen, type Event as TauriEvent } from '@tauri-apps/api/event';

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

export async function listWorkers(): Promise<WorkerInfo[]> {
  return invoke('list_workers');
}

export async function addWorker(name: string, url: string): Promise<void> {
  return invoke('add_worker', { name, url });
}

export async function pairWorker(workerId: string, pairingCode: string): Promise<void> {
  return invoke('pair_worker', { workerId, pairingCode });
}

export async function pingWorker(workerId: string): Promise<void> {
  return invoke('ping_worker', { workerId });
}

export interface ClusterIdentityInfo {
  cluster_name: string;
  ca_cert_pem: string;
  device_serial: string;
}

export async function getClusterIdentity(): Promise<ClusterIdentityInfo | null> {
  return invoke('get_cluster_identity');
}

export async function createCluster(name: string): Promise<ClusterIdentityInfo> {
  return invoke('create_cluster', { name });
}

export async function importClusterKey(key: string, name: string): Promise<ClusterIdentityInfo> {
  return invoke('import_cluster_key', { key, name });
}

export async function exportClusterKey(): Promise<string> {
  return invoke('export_cluster_key');
}

export async function exportClusterBackup(): Promise<string> {
  return invoke('export_cluster_backup');
}

export async function installWorker(
  host: string,
  user: string,
  port: number,
  privateKey: string,
  releaseTag: string,
  pairingCode: string,
): Promise<InstallWorkerResult> {
  return invoke('install_worker', {
    host,
    user,
    port,
    private_key: privateKey,
    release_tag: releaseTag,
    repo: null,
    pairing_code: pairingCode,
  });
}

export async function workerLogs(): Promise<{ id: string; timestamp: string; message: string }[]> {
  return invoke('worker_logs');
}

export async function createChat(title: string, provider?: string, model?: string): Promise<string> {
  return invoke('create_chat', { title, provider, model });
}

export async function listChats(): Promise<Conversation[]> {
  return invoke('list_chats');
}

export async function addChatMessage(
  chatId: string,
  role: string,
  content: string,
): Promise<void> {
  return invoke('add_chat_message', { chatId, role, content });
}

export async function chatMessages(chatId: string): Promise<ChatMessage[]> {
  return invoke('chat_messages', { chatId });
}

export async function listAgents(): Promise<AgentInfo[]> {
  return invoke('list_agents');
}

export async function createAgent(
  name: string,
  prompt: string,
  description?: string,
  tools?: string[],
): Promise<AgentInfo> {
  return invoke('create_agent', {
    req: { name, prompt, description, tools: tools ?? [] },
  });
}

export async function deleteAgent(agentId: string): Promise<void> {
  return invoke('delete_agent', { agentId });
}

export async function listWorkflows(): Promise<WorkflowInfo[]> {
  return invoke('list_workflows');
}

export async function createWorkflow(
  name: string,
  description: string,
  steps: WorkflowStep[],
  trigger: string,
): Promise<WorkflowInfo> {
  return invoke('create_workflow', {
    req: { name, description, steps, trigger },
  });
}

export async function deleteWorkflow(workflowId: string): Promise<void> {
  return invoke('delete_workflow', { workflowId });
}

export async function listTeams(): Promise<TeamInfo[]> {
  return invoke('list_teams');
}

export async function createTeam(
  id: string,
  name: string,
  metadata: string,
  agentIds: string[],
): Promise<TeamInfo> {
  return invoke('create_team', { req: { id, name, metadata, agent_ids: agentIds } });
}

export async function listExecutions(): Promise<ExecutionInfo[]> {
  return invoke('list_executions');
}

export async function listVaultSecrets(): Promise<VaultSecretInfo[]> {
  return invoke('list_vault_secrets');
}

export async function setVaultSecret(name: string, value: string): Promise<void> {
  return invoke('set_vault_secret', { req: { name, value } });
}

export async function unlockVault(passphrase: string): Promise<string[]> {
  return invoke('unlock_vault', { req: { passphrase } });
}

export async function getLlmSetting(provider: string): Promise<LlmSetting | null> {
  return invoke('get_llm_setting', { provider });
}

export async function setLlmSetting(
  provider: string,
  apiKey: string,
  model: string,
  baseUrl?: string,
  temperature?: number,
): Promise<void> {
  return invoke('set_llm_setting', {
    req: { provider, api_key: apiKey, model, base_url: baseUrl, temperature },
  });
}

export async function runAgent(
  workerId: string,
  chatId: string,
  agentId: string,
  input: string,
): Promise<void> {
  return invoke('run_agent', { req: { worker_id: workerId, chat_id: chatId, agent_id: agentId, prompt: input } });
}

export async function classifyIntent(
  provider: string,
  model: string,
  text: string,
): Promise<{ intent: string; params: Record<string, unknown> }> {
  return invoke('classify_intent', { req: { provider, model, text } });
}

export async function scheduleAgent(
  workerId: string,
  agentId: string,
  trigger: string,
): Promise<void> {
  return invoke('schedule_agent', { req: { worker_id: workerId, agent_id: agentId, trigger } });
}

export async function setChatModel(
  chatId: string,
  provider: string,
  model: string,
): Promise<void> {
  return invoke('set_chat_model', { chatId, provider, model });
}

export async function runHarness(
  chatId: string,
  input: string,
  model?: string,
): Promise<void> {
  return invoke('run_harness', { chatId, input, model });
}

export async function cancelHarness(chatId: string): Promise<void> {
  return invoke('cancel_harness', { chatId });
}

export async function listHarnessTools(): Promise<{ name: string; description?: string }[]> {
  return invoke('list_harness_tools');
}

export async function searchMcpServers(query: string): Promise<McpSearchResult[]> {
  return invoke('search_mcp_servers', { req: { query } });
}

export async function listMcpServers(): Promise<McpServerSummary[]> {
  return invoke('list_mcp_servers');
}

export async function installMcpServer(
  id: string,
  name: string,
  source: string,
  sourceValue?: string,
  secretIds: string[] = [],
): Promise<string> {
  const req: Record<string, unknown> = { id, name, source, source_value: sourceValue };
  if (secretIds.length > 0) {
    req.secret_ids = secretIds;
  }
  return invoke('install_mcp_server', { req });
}

export async function updateMcpServer(
  id: string,
  name?: string,
  sourceValue?: string,
  secretIds: string[] = [],
): Promise<string> {
  return invoke('update_mcp_server', { req: { id, name, source_value: sourceValue, secret_ids: secretIds } });
}

export async function deleteMcpServer(id: string): Promise<string> {
  return invoke('delete_mcp_server', { req: { id } });
}

export async function updateMcpServerMeta(
  id: string,
  secretIds: string[],
  enabledTools: string[],
): Promise<string> {
  return invoke('update_mcp_server_meta', {
    req: { id, secret_ids: secretIds, enabled_tools: enabledTools },
  });
}

export async function discoverMcpTools(id: string): Promise<McpTool[]> {
  return invoke('discover_mcp_tools', { req: { id } });
}

export async function testCallMcpTool(
  id: string,
  toolName: string,
  args?: Record<string, unknown>,
): Promise<unknown> {
  return invoke('test_call_mcp_tool', { req: { id, tool_name: toolName, arguments: args } });
}

export function onWorkersUpdated(callback: () => void): Promise<() => void> {
  return listen('workers:updated', callback);
}

export function onLogsUpdated(callback: () => void): Promise<() => void> {
  return listen('logs:updated', callback);
}

export function onAgentLog(
  callback: (payload: TauriEvent<unknown>) => void,
): Promise<() => void> {
  return listen('agent:log', callback);
}

export function onAgentStarted(
  callback: (payload: TauriEvent<unknown>) => void,
): Promise<() => void> {
  return listen('agent:started', callback);
}

export function onAgentFinished(
  callback: (payload: TauriEvent<unknown>) => void,
): Promise<() => void> {
  return listen('agent:finished', callback);
}

export function onChatUpdated(
  callback: (payload: TauriEvent<unknown>) => void,
): Promise<() => void> {
  return listen('chat:updated', callback);
}

export function onChatsUpdated(callback: () => void): Promise<() => void> {
  return listen('chats:updated', callback);
}

export function onAgentsUpdated(callback: () => void): Promise<() => void> {
  return listen('agents:updated', callback);
}

export function onWorkflowsUpdated(callback: () => void): Promise<() => void> {
  return listen('workflows:updated', callback);
}

export function onTeamsUpdated(callback: () => void): Promise<() => void> {
  return listen('teams:updated', callback);
}

export function onExecutionsUpdated(callback: () => void): Promise<() => void> {
  return listen('executions:updated', callback);
}

export function onVaultUpdated(callback: () => void): Promise<() => void> {
  return listen('vault:updated', callback);
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

export function onHarnessEvent(
  callback: (payload: TauriEvent<HarnessEventPayload>) => void,
): Promise<() => void> {
  return listen('harness:event', callback);
}
