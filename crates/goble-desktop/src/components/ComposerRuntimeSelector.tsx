import { useMemo } from 'react';
import type { WorkerInfo } from '../tauri/api';

export type RuntimeTarget =
  | { kind: 'auto' }
  | { kind: 'local' }
  | { kind: 'tag'; tag: string }
  | { kind: 'worker'; workerId: string };

interface ComposerRuntimeSelectorProps {
  workers: WorkerInfo[];
  value: RuntimeTarget;
  onChange: (target: RuntimeTarget) => void;
}

export function runtimeTargetLabel(target: RuntimeTarget, workers: WorkerInfo[]): string {
  switch (target.kind) {
    case 'auto':
      return 'Auto';
    case 'local':
      return 'Local';
    case 'tag':
      return `Group: ${target.tag}`;
    case 'worker': {
      const w = workers.find((x) => x.id === target.workerId);
      return w ? w.name : target.workerId;
    }
  }
}

export default function ComposerRuntimeSelector({
  workers,
  value,
  onChange,
}: ComposerRuntimeSelectorProps) {
  const tags = useMemo(() => {
    const set = new Set<string>();
    for (const w of workers) {
      for (const t of w.tags || []) set.add(t);
    }
    return Array.from(set).sort();
  }, [workers]);

  return (
    <select
      className="composer-runtime-selector"
      value={value.kind === 'tag' ? `tag:${value.tag}` : value.kind === 'worker' ? `worker:${value.workerId}` : value.kind}
      onChange={(e) => {
        const raw = e.target.value;
        if (raw.startsWith('tag:')) {
          onChange({ kind: 'tag', tag: raw.slice(4) });
        } else if (raw.startsWith('worker:')) {
          onChange({ kind: 'worker', workerId: raw.slice(7) });
        } else {
          onChange({ kind: raw as 'auto' | 'local' });
        }
      }}
      title="Runtime target"
    >
      <option value="auto">Auto</option>
      <option value="local">Local</option>
      <optgroup label="Groups">
        {tags.map((t) => (
          <option key={`tag:${t}`} value={`tag:${t}`}>
            {t}
          </option>
        ))}
      </optgroup>
      <optgroup label="Workers">
        {workers.map((w) => (
          <option key={`worker:${w.id}`} value={`worker:${w.id}`}>
            {w.name}
          </option>
        ))}
      </optgroup>
    </select>
  );
}
