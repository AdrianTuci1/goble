import { useState } from 'react';
import { useStore } from '../stores/appStore';
import { createAgent, deleteAgent } from '../tauri/api';
import './Pages.css';

export default function AgentsPage() {
  const agents = useStore((s) => s.agents);
  const [name, setName] = useState('');
  const [prompt, setPrompt] = useState('');
  const [description, setDescription] = useState('');
  const [tools, setTools] = useState('');

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name || !prompt) return;
    await createAgent(name, prompt, description || undefined, tools.split(',').map((t) => t.trim()).filter(Boolean));
    setName('');
    setPrompt('');
    setDescription('');
    setTools('');
  }

  return (
    <div className="page">
      <div className="page-header">
        <h2>Agents</h2>
      </div>
      <div className="page-content">
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
          />
          <input
            placeholder="Tools (comma separated)"
            value={tools}
            onChange={(e) => setTools(e.target.value)}
          />
          <button type="submit">Create agent</button>
        </form>

        <div className="agent-list">
          {agents.map((a) => (
            <div key={a.id} className="card">
              <div className="card-title">{a.name}</div>
              <div className="card-row">ID: {a.id}</div>
              <div className="card-row">{a.spec.description}</div>
              <div className="card-row">Tools: {a.spec.tools.join(', ') || 'none'}</div>
              <button onClick={() => deleteAgent(a.id)}>Delete</button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
