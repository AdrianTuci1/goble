import { Plus } from 'lucide-react';
import './WorkspaceRail.css';

const WORKSPACES = [{ id: 'demo', name: 'Demo', color: '#2563eb' }];

export default function WorkspaceRail() {
  return (
    <aside className="threads-workspace-sidebar" aria-label="Workspaces">
      <div className="workspace-list">
        {WORKSPACES.map((ws) => (
          <button
            key={ws.id}
            className="workspace-item selected"
            title={ws.name}
            style={{ background: ws.color }}
          >
            <span className="workspace-initials">{initials(ws.name)}</span>
          </button>
        ))}
        <div className="workspace-divider" />
        <button className="workspace-add" title="Add workspace">
          <Plus size={18} />
        </button>
      </div>
    </aside>
  );
}

function initials(name: string) {
  return name
    .split(' ')
    .map((n) => n[0])
    .join('')
    .slice(0, 2)
    .toUpperCase();
}
