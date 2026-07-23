import { invoke } from '@tauri-apps/api/core';

export interface WorkerInfo {
  id: string;
  name: string;
  url: string;
  paired: boolean;
}

export async function listWorkers(): Promise<WorkerInfo[]> {
  return invoke('list_workers');
}

export async function workerLogs(): Promise<string[]> {
  return invoke('worker_logs');
}

export async function pingWorker(workerId: string): Promise<void> {
  return invoke('ping_worker', { workerId });
}

export async function addLog(message: string): Promise<void> {
  return invoke('add_log', { message });
}

export async function runAgent(agentId: string, prompt: string): Promise<void> {
  // placeholder until backend command is added
  await addLog(`run agent ${agentId}: ${prompt}`);
}

export async function scheduleAgent(agentId: string, trigger: string): Promise<void> {
  await addLog(`schedule agent ${agentId}: ${trigger}`);
}

export async function listVaultSecrets(): Promise<string[]> {
  return [];
}

export async function setVaultSecret(name: string, _value: string): Promise<void> {
  await addLog(`set vault secret ${name}`);
}

export async function listScheduledTasks(): Promise<Array<{ id: string; agentId: string; trigger: string; enabled: boolean }>> {
  return [];
}

export async function cancelScheduledTask(taskId: string): Promise<void> {
  await addLog(`cancel task ${taskId}`);
}
