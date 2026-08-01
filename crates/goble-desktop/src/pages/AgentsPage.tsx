import { useState } from 'react';
import { useStore, type AgentInfo } from '../stores/appStore';
import { createAgent, deleteAgent } from '../tauri/api';
import './Pages.css';

function initial(name: string) {
  return String(name || '?').charAt(0).toUpperCase();
}

export default function AgentsPage() {
  const agents = useStore((s) => s.agents);
  const [showForm, setShowForm] = useState(false);
  const [name, setName] = useState('');
  const [prompt, setPrompt] = useState('');
  const [description, setDescription] = useState('');
  const [tools, setTools] = useState('');
  const setSelected = useStore((s) => s.setSelectedAgentId);
  const setRightSidebarOpen = useStore((s) => s.setRightSidebarOpen);
  const setRightSidebarTab = useStore((s) => s.setRightSidebarTab);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name || !prompt) return;
    await createAgent(name, prompt, description || undefined, tools.split(',').map((t) => t.trim()).filter(Boolean));
    setName('');
    setPrompt('');
    setDescription('');
    setTools('');
    setShowForm(false);
  }

  function openAgentInfo(agent: AgentInfo) {
    setSelected(agent.id);
    setRightSidebarTab('info');
    setRightSidebarOpen(true);
  }

  return (
    <div className="page">
      <div className="agents-view-container">
        <div className="agents-header">
          <h3>Agents</h3>
          <button className="add-agent-btn" onClick={() => setShowForm((v) => !v)}>
            + Add agent
          </button>
        </div>

        {showForm && (
          <form className="agent-form" onSubmit={handleSubmit}>
            <input
              placeholder="Agent name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
            <input
              placeholder="Description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
            <textarea
              placeholder="System prompt"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              rows={4}
            />
            <input
              placeholder="Tools (comma separated)"
              value={tools}
              onChange={(e) => setTools(e.target.value)}
            />
            <div style={{ display: 'flex', gap: 8 }}>
              <button type="submit">Create agent</button>
              <button type="button" className="secondary" onClick={() => setShowForm(false)}>Cancel</button>
            </div>
          </form>
        )}

        <div className="agents-list">
          {agents.map((a) => (
            <div key={a.id} className="agent-card" onClick={() => openAgentInfo(a)}>
              <div className="agent-avatar">{initial(a.name)}</div>
              <div className="agent-name">{a.name}</div>
              <button
                style={{ marginLeft: 'auto', background: 'transparent', border: 'none', color: 'var(--ds-muted)', cursor: 'pointer' }}
                onClick={(e) => { e.stopPropagation(); deleteAgent(a.id); }}
                title="Delete"
              >
                ×
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
