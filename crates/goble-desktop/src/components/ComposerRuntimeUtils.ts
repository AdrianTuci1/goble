import type { RuntimeTarget, WorkerInfo } from '../tauri/api';

export type { RuntimeTarget };

export function runtimeTargetLabel(target: RuntimeTarget, workers: WorkerInfo[]): string {
  switch (target.kind) {
    case 'auto':
      return 'Auto';
    case 'local':
      return 'Local';
    case 'tag':
      return target.tag ? `Group: ${target.tag}` : 'Group';
    case 'worker': {
      const worker = workers.find((w) => w.id === target.worker_id);
      return worker ? worker.name : target.worker_id || 'Worker';
    }
    default:
      return 'Auto';
  }
}
