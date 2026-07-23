import { useState } from 'react';
import { useLocation } from 'react-router-dom';
import { useStore } from '../stores/appStore';
import { addWorker, pairWorker, pingWorker } from '../tauri/api';

export default function Sidebar() {
  const location = useLocation();
  const workers = useStore((s) => s.workers);
  const conversations = useStore((s) => s.conversations);
  const activeChatId = useStore((s) => s.activeConversationId);
  const setActiveChatId = useStore((s) => s.setActiveConversation);
  const setSettingsOpen = useStore((s) => s.setSettingsOpen);

  const [newName, setNewName] = useState('');
  const [newUrl, setNewUrl] = useState('');
  const [pairCode, setPairCode] = useState('');
  const [selectedWorker, setSelectedWorker] = useState('');

  const navItems = [
    { path: '/chat', label: 'Chat', icon: '💬' },
    { path: '/workflows', label: 'Workflows', icon: '⚡' },
    { path: '/knowledge', label: 'Knowledge', icon: '📚' },
    { path: '/connectors', label: 'Connectors', icon: '🔌' },
    { path: '/search', label: 'Search', icon: '🔍' },
  ];

  async function handleAddWorker() {
    if (!newName || !newUrl) return;
    await addWorker(newName, newUrl);
    setNewName('');
    setNewUrl('');
  }

  async function handlePair() {
    if (!selectedWorker || !pairCode) return;
    await pairWorker(selectedWorker, pairCode);
    setPairCode('');
  }

  return (
    <aside className="sidebar">
      <div className="logo">Goble</div>
      <nav className="nav">
        {navItems.map((item) => (
          <a
            key={item.path}
            href={item.path}
            className={`nav-item ${location.pathname === item.path ? 'active' : ''}`}
          >
            <span className="nav-icon">{item.icon}</span>
            <span className="nav-label">{item.label}</span>
          </a>
        ))}
      </nav>

      <div className="sidebar-section">
        <div className="section-title">Conversations</div>
        <div className="conversation-list">
          {conversations.map((c) => (
            <button
              key={c.id}
              className={`conversation-item ${activeChatId === c.id ? 'active' : ''}`}
              onClick={() => setActiveChatId(c.id)}
            >
              {c.title}
            </button>
          ))}
        </div>
      </div>

      <div className="sidebar-section">
        <div className="section-title">Workers</div>
        <div className="worker-list">
          {workers.map((w) => (
            <div key={w.id} className={`worker-item ${w.paired ? 'paired' : ''}`}>
              <div className="worker-name">{w.name}</div>
              <div className="worker-meta">{w.paired ? 'paired' : 'unpaired'}</div>
              {w.paired && (
                <button onClick={() => pingWorker(w.id)}>Ping</button>
              )}
            </div>
          ))}
        </div>
        <div className="add-worker-form">
          <input
            placeholder="Name"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
          />
          <input
            placeholder="ws://host:port/ws"
            value={newUrl}
            onChange={(e) => setNewUrl(e.target.value)}
          />
          <button onClick={handleAddWorker}>Add worker</button>
        </div>
        <div className="pair-worker-form">
          <select value={selectedWorker} onChange={(e) => setSelectedWorker(e.target.value)}>
            <option value="">Select worker</option>
            {workers.map((w) => (
              <option key={w.id} value={w.id}>{w.name}</option>
            ))}
          </select>
          <input
            placeholder="Pairing code"
            value={pairCode}
            onChange={(e) => setPairCode(e.target.value)}
          />
          <button onClick={handlePair}>Pair</button>
        </div>
      </div>

      <button className="settings-button" onClick={() => setSettingsOpen(true)}>
        Settings
      </button>
    </aside>
  );
}
