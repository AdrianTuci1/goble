import { useState } from 'react';
import { useStore } from '../stores/appStore';
import { createAgent, deleteAgent } from '../tauri/api';
import './Pages.css';

export default function AgentsPage() {
  const agents = useStore((s) => s.agents);
  const workflows = useStore((s) => s.workflows);
  const executions = useStore((s) => s.executions);
  const workers = useStore((s) => s.workers);
  const [name, setName] = useState('');
  const [prompt, setPrompt] = useState('');
  const [description, setDescription] = useState('');
  const [tools, setTools] = useState('');
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name || !prompt) return;
    await createAgent(name, prompt, description || undefined, tools.split(',').map((t) => t.trim()).filter(Boolean));
    setName('');
    setPrompt('');
    setDescription('');
    setTools('');
  }

  const selected = selectedAgent ? agents.find((a) => a.id === selectedAgent) : null;
  const agentWorkflows = selected
    ? workflows.filter((w) => w.steps.some((s) => s.agent_id['0'] === selected.id))
    : [];
  const agentExecutions = selected
    ? executions.filter((x) => x.agent_id === selected.id).sort((a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime())
    : [];

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
            <div
              key={a.id}
              className={`card ${selectedAgent === a.id ? 'selected' : ''}`}
              onClick={() => setSelectedAgent(a.id)}
            >
              <div className="card-title">{a.name}</div>
              <div className="card-row">ID: {a.id}</div>
              <div className="card-row">{a.spec.description}</div>
              <div className="card-row">Tools: {a.spec.tools.join(', ') || 'none'}</div>
              <button onClick={() => deleteAgent(a.id)}>Delete</button>
            </div>
          ))}
        </div>
      </div>

      {selected && (
        <aside className="agent-drawer">
          <div className="drawer-header">
            <h3>{selected.name}</h3>
            <button onClick={() => setSelectedAgent(null)}>×</button>
          </div>
          <div className="drawer-body">
            <div className="drawer-section">
              <div className="drawer-section-title">Details</div>
              <div className="drawer-row">ID: {selected.id}</div>
              <div className="drawer-row">Prompt: {selected.spec.prompt}</div>
              <div className="drawer-row">Tools: {selected.spec.tools.join(', ') || 'none'}</div>
            </div>

            <div className="drawer-section">
              <div className="drawer-section-title">Workflows</div>
              {agentWorkflows.length === 0 && (
                <div className="drawer-empty">No workflows use this agent.</div>
              )}
              {agentWorkflows.map((w) => (
                <div key={w.id} className="drawer-row">
                  {w.name} — {w.trigger as string}
                </div>
              ))}
            </div>

            <div className="drawer-section">
              <div className="drawer-section-title">Executions</div>
              {agentExecutions.length === 0 && (
                <div className="drawer-empty">No executions yet.</div>
              )}
              {agentExecutions.slice(0, 20).map((x) => (
                <div key={x.id} className="drawer-row">
                  <span className={`status-badge status-${x.status.toLowerCase() === 'success' ? 'success' : x.status.toLowerCase() === 'running' ? 'running' : 'other'}`}>{x.status}</span>
                  {x.id} — {new Date(x.started_at).toLocaleString()}
                  {x.worker_id && (
                    <span className="drawer-hint"> on {workers.find((w) => w.id === x.worker_id)?.name || x.worker_id}</span>
                  )}
                </div>
              ))}
            </div>
          </div>
        </aside>
      )}
    </div>
  );
}
