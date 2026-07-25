import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Event as TauriEvent } from '@tauri-apps/api/event';

export interface WorkerConnection {
  id: string;
  name: string;
  url: string;
  paired: boolean;
}

export interface LogEntry {
  id: string;
  timestamp: string;
  message: string;
}

export interface ChatMessage {
  id: string;
  role: string;
  content: string;
  created_at: string;
}

export interface Chat {
  id: string;
  title: string;
  provider?: string;
  model?: string;
  agent_id?: string | null;
  worker_id?: string | null;
  updated_at: string;
}

export interface AgentSpec {
  id: { 0: string };
  name: string;
  description: string;
  prompt: string;
  tools: string[];
  triggers: { Cron?: { expression: string }; Manual?: unknown; Http?: { path: string }; Heartbeat?: { interval_seconds: number } }[];
  mcp_ids: string[];
}

export interface AgentInfo {
  id: string;
  name: string;
  spec: AgentSpec;
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
  trigger: AgentSpec['triggers'][number];
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

export interface ToolSchema {
  name: string;
  description: string;
  parameters: unknown;
}

export interface LlmSetting {
  api_key: string;
  base_url: string | null;
  model: string;
  temperature: number | null;
}

export interface HarnessEvent {
  type: 'assistant_delta' | 'tool_call_started' | 'tool_call_finished' | 'tool_call_error' | 'done' | 'error';
  payload?: unknown;
}

export async function runHarness(
  chatId: string,
  prompt: string,
  provider: string,
  model: string,
): Promise<void> {
  return invoke('run_harness', { req: { chat_id: chatId, prompt, provider, model } });
}

export async function listHarnessTools(): Promise<ToolSchema[]> {
  return invoke('list_harness_tools');
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

export async function getLlmSetting(provider: string): Promise<LlmSetting | null> {
  return invoke('get_llm_setting', { provider });
}

export async function setChatModel(
  chatId: string,
  provider: string,
  model: string,
): Promise<void> {
  return invoke('set_chat_model', { req: { chat_id: chatId, provider, model } });
}

export async function listWorkers(): Promise<WorkerConnection[]> {
  return invoke('list_workers');
}

export async function addWorker(name: string, url: string): Promise<WorkerConnection> {
  return invoke('add_worker', { req: { name, url } });
}

export async function pairWorker(workerId: string, pairingCode: string): Promise<boolean> {
  return invoke('pair_worker', { req: { worker_id: workerId, pairing_code: pairingCode } });
}

export async function workerLogs(): Promise<LogEntry[]> {
  return invoke('worker_logs');
}

export async function pingWorker(workerId: string): Promise<void> {
  return invoke('ping_worker', { workerId });
}

export async function addLog(message: string): Promise<void> {
  return invoke('add_log', { message });
}

export async function createChat(
  title: string,
  provider?: string,
  model?: string,
): Promise<string> {
  return invoke('create_chat', { title, provider, model });
}

export async function listChats(): Promise<Chat[]> {
  return invoke('list_chats');
}

export async function chatMessages(chatId: string): Promise<ChatMessage[]> {
  return invoke('chat_messages', { chatId });
}

export async function addChatMessage(
  chatId: string,
  role: string,
  content: string,
): Promise<void> {
  return invoke('add_chat_message', { chatId, role, content });
}

export async function listAgents(): Promise<AgentInfo[]> {
  return invoke('list_agents');
}

export async function createAgent(
  name: string,
  prompt: string,
  description?: string,
  tools: string[] = [],
): Promise<AgentInfo> {
  return invoke('create_agent', { req: { name, prompt, description, tools } });
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
  return invoke('create_workflow', { req: { name, description, steps, trigger } });
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

export async function runAgent(
  workerId: string,
  agentId: string,
  prompt: string,
): Promise<void> {
  return invoke('run_agent', { req: { worker_id: workerId, agent_id: agentId, prompt } });
}

export async function scheduleAgent(
  workerId: string,
  agentId: string,
  trigger: string,
): Promise<void> {
  return invoke('schedule_agent', { req: { worker_id: workerId, agent_id: agentId, trigger } });
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

export const LLM_PROVIDERS = [
  { id: 'openai', name: 'OpenAI', defaultModel: 'gpt-4o-mini' },
  { id: 'anthropic', name: 'Anthropic', defaultModel: 'claude-3-5-sonnet-20241022' },
  { id: 'ollama', name: 'Ollama', defaultModel: 'llama3.1' },
  { id: 'openrouter', name: 'OpenRouter', defaultModel: 'openai/gpt-4o-mini' },
];
