import { useState } from 'react';
import { useStore } from '../stores/appStore';
import { createTeam } from '../tauri/api';
import './Pages.css';

export default function TeamsPage() {
  const teams = useStore((s) => s.teams);
  const agents = useStore((s) => s.agents);
  const [id, setId] = useState('');
  const [name, setName] = useState('');
  const [metadata, setMetadata] = useState('{}');
  const [selectedAgents, setSelectedAgents] = useState<string[]>([]);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!id || !name) return;
    await createTeam(id, name, metadata, selectedAgents);
    setId('');
    setName('');
    setMetadata('{}');
    setSelectedAgents([]);
  }

  function toggleAgent(agentId: string) {
    setSelectedAgents((prev) =>
      prev.includes(agentId) ? prev.filter((a) => a !== agentId) : [...prev, agentId]
    );
  }

  return (
    <div className="page">
      <div className="page-header">
        <h2>Teams</h2>
      </div>
      <div className="page-content">
        <form className="team-form" onSubmit={handleSubmit}>
          <input
            placeholder="Team ID"
            value={id}
            onChange={(e) => setId(e.target.value)}
          />
          <input
            placeholder="Team name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <textarea
            placeholder="Metadata JSON"
            value={metadata}
            onChange={(e) => setMetadata(e.target.value)}
          />
          <div className="agent-selection">
            <div className="section-title">Agents</div>
            {agents.map((a) => (
              <label key={a.id} className="checkbox-label">
                <input
                  type="checkbox"
                  checked={selectedAgents.includes(a.id)}
                  onChange={() => toggleAgent(a.id)}
                />
                {a.name}
              </label>
            ))}
          </div>
          <button type="submit">Create team</button>
        </form>

        <div className="team-list">
          {teams.map((t) => (
            <div key={t.id} className="card">
              <div className="card-title">{t.name}</div>
              <div className="card-row">ID: {t.id}</div>
              <div className="card-row">Members: {t.members.join(', ') || 'none'}</div>
              <div className="card-row">Metadata: {t.metadata}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
