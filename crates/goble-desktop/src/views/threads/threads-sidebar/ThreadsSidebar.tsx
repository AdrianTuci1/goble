import { Inbox, MessageSquare, FolderKanban, Hash, Lock, Plus } from 'lucide-react';
import { useThreadsStore, type ThreadsNav } from '../store/threadsStore';
import './ThreadsSidebar.css';

const navItems: { id: ThreadsNav; label: string; icon: React.ReactNode }[] = [
  { id: 'inbox', label: 'Inbox', icon: <Inbox size={18} /> },
  { id: 'threads', label: 'Threads', icon: <MessageSquare size={18} /> },
  { id: 'projects', label: 'Projects', icon: <FolderKanban size={18} /> },
];

export default function ThreadsSidebar() {
  const { nav, setNav, activeChannelId, activeDmId, selectChannel, selectDm } = useThreadsStore();
  const ws = useThreadsStore((s) => s.workspaces.find((w) => w.id === s.activeWorkspaceId));
  if (!ws) return null;

  function initials(name: string) {
    return name
      .split(' ')
      .map((n) => n[0])
      .join('')
      .slice(0, 2)
      .toUpperCase();
  }

  return (
    <aside className="threads-sidebar" aria-label="Threads sidebar">
      <div className="threads-sidebar-nav">
        {navItems.map((item) => (
          <button
            key={item.id}
            className={`nav-item ${nav === item.id ? 'selected' : ''}`}
            onClick={() => setNav(item.id)}
          >
            <span className="nav-icon">{item.icon}</span>
            <span>{item.label}</span>
          </button>
        ))}
      </div>

      <div className="threads-sidebar-section">
        <h4>Channels</h4>
        <div className="channel-list">
          {ws.channels.map((ch) => (
            <button
              key={ch.id}
              className={`channel-item ${activeChannelId === ch.id ? 'selected' : ''}`}
              onClick={() => selectChannel(ch.id)}
            >
              <span className="channel-icon">{ch.private ? <Lock size={14} /> : <Hash size={14} />}</span>
              <span className="channel-name">{ch.name}</span>
              {ch.unread > 0 && <span className="unread-badge">{ch.unread}</span>}
            </button>
          ))}
          <button className="channel-item add">
            <span className="channel-icon"><Plus size={14} /></span>
            <span className="channel-name">Add channel</span>
          </button>
        </div>
      </div>

      {ws.projects.length > 0 && (
        <div className="threads-sidebar-section">
          <h4>Projects</h4>
          <div className="channel-list">
            {ws.projects.map((p) => (
              <button key={p.id} className="channel-item" onClick={() => setNav('projects')}>
                <span className="channel-icon"><FolderKanban size={14} /></span>
                <span className="channel-name">{p.name}</span>
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="threads-sidebar-section">
        <h4>Direct messages</h4>
        <div className="dm-list">
          {ws.directMessages.map((dm) => (
            <button
              key={dm.id}
              className={`dm-item ${activeDmId === dm.id ? 'selected' : ''}`}
              onClick={() => selectDm(dm.id)}
            >
              <span className="dm-avatar" style={{ background: `hsl(${Math.abs(dm.name.split('').reduce((a, b) => a + b.charCodeAt(0), 0)) % 360}, 60%, 45%)` }}>
                {initials(dm.name)}
              </span>
              <span className="dm-name">{dm.name}</span>
              {dm.unread > 0 && <span className="unread-badge">{dm.unread}</span>}
            </button>
          ))}
        </div>
      </div>
    </aside>
  );
}
