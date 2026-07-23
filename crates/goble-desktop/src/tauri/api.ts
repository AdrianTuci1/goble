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
  agent_id?: string | null;
  worker_id?: string | null;
  updated_at: string;
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

export async function createChat(title: string): Promise<string> {
  return invoke('create_chat', { title });
}

export async function chatMessages(chatId: string): Promise<ChatMessage[]> {
  return invoke('chat_messages', { chatId });
}

export async function addChatMessage(chatId: string, role: string, content: string): Promise<void> {
  return invoke('add_chat_message', { chatId, role, content });
}

export async function runAgent(workerId: string, agentId: string, prompt: string): Promise<void> {
  return invoke('run_agent', { req: { worker_id: workerId, agent_id: agentId, prompt } });
}

export async function scheduleAgent(workerId: string, agentId: string, trigger: string): Promise<void> {
  return invoke('schedule_agent', { req: { worker_id: workerId, agent_id: agentId, trigger } });
}

export async function setVaultSecret(name: string, value: string): Promise<void> {
  return invoke('set_vault_secret', { req: { name, value } });
}

export function onWorkersUpdated(callback: () => void): Promise<() => void> {
  return listen('workers:updated', callback);
}

export function onLogsUpdated(callback: () => void): Promise<() => void> {
  return listen('logs:updated', callback);
}

export function onAgentLog(callback: (payload: TauriEvent<unknown>) => void): Promise<() => void> {
  return listen('agent:log', callback);
}

export function onAgentStarted(callback: (payload: TauriEvent<unknown>) => void): Promise<() => void> {
  return listen('agent:started', callback);
}

export function onAgentFinished(callback: (payload: TauriEvent<unknown>) => void): Promise<() => void> {
  return listen('agent:finished', callback);
}

export function onChatUpdated(callback: (payload: TauriEvent<unknown>) => void): Promise<() => void> {
  return listen('chat:updated', callback);
}

export function onChatsUpdated(callback: () => void): Promise<() => void> {
  return listen('chats:updated', callback);
}
