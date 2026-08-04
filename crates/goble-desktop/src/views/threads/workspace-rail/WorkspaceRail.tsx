import { Plus } from 'lucide-react';
import { useThreadsStore } from '../store/threadsStore';
import './WorkspaceRail.css';

export default function WorkspaceRail() {
  const { workspaces, activeWorkspaceId, setActiveWorkspace } = useThreadsStore();

  function initials(name: string) {
    return name
      .split(' ')
      .map((n) => n[0])
      .join('')
      .slice(0, 2)
      .toUpperCase();
  }

  return (
    <aside className="threads-workspace-sidebar" aria-label="Workspaces">
      <div className="workspace-list">
        {workspaces.map((ws, i) => (
          <div key={ws.id} className="workspace-wrap">
            {i > 0 && <div className="workspace-divider" />}
            <button
              className={`workspace-item ${ws.id === activeWorkspaceId ? 'selected' : ''}`}
              style={{ background: ws.color }}
              onClick={() => setActiveWorkspace(ws.id)}
              title={ws.name}
            >
              <span className="workspace-initials">{initials(ws.name)}</span>
            </button>
          </div>
        ))}
        <div className="workspace-divider" />
        <button className="workspace-add" title="Add workspace">
          <Plus size={18} />
        </button>
      </div>
    </aside>
  );
}
