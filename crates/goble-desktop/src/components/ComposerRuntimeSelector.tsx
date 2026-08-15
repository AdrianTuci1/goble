import { useMemo } from 'react';
import type { WorkerInfo } from '../tauri/api';
import { type RuntimeTarget } from './ComposerRuntimeUtils';

interface ComposerRuntimeSelectorProps {
  workers: WorkerInfo[];
  value: RuntimeTarget;
  onChange: (target: RuntimeTarget) => void;
}

export default function ComposerRuntimeSelector({ workers, value, onChange }: ComposerRuntimeSelectorProps) {
  const groups = useMemo(() => {
    const map = new Map<string, WorkerInfo[]>();
    for (const w of workers) {
      for (const tag of w.tags) {
        if (!map.has(tag)) map.set(tag, []);
        map.get(tag)?.push(w);
      }
    }
    return map;
  }, [workers]);

  return (
    <div className="composer-runtime-selector">
      <select
        value={value.kind}
        onChange={(e) => {
          const kind = e.target.value as RuntimeTarget['kind'];
          if (kind === 'auto') onChange({ kind: 'auto' });
          if (kind === 'local') onChange({ kind: 'local' });
          if (kind === 'tag') onChange({ kind: 'tag', tag: '' });
          if (kind === 'worker') onChange({ kind: 'worker', workerId: '' });
        }}
        title="Runtime target"
      >
        <option value="auto">Auto</option>
        <option value="local">Local</option>
        <option value="tag">Group</option>
        <option value="worker">Worker</option>
      </select>

      {value.kind === 'tag' && (
        <select
          value={value.tag}
          onChange={(e) => onChange({ kind: 'tag', tag: e.target.value })}
          title="Worker group"
        >
          <option value="">Pick group</option>
          {[...groups.keys()].map((tag) => (
            <option key={tag} value={tag}>{tag} ({groups.get(tag)?.length || 0})</option>
          ))}
        </select>
      )}

      {value.kind === 'worker' && (
        <select
          value={value.workerId}
          onChange={(e) => onChange({ kind: 'worker', workerId: e.target.value })}
          title="Specific worker"
        >
          <option value="">Pick worker</option>
          {workers.map((w) => (
            <option key={w.id} value={w.id}>{w.name}</option>
          ))}
        </select>
      )}
    </div>
  );
}
