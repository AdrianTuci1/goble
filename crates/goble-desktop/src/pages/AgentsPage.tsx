import { useState } from 'react';
import { useStore } from '../stores/appStore';
import { createAgent, updateAgent, deleteAgent, listAgents } from '../tauri/api';
import './AgentsPage.css';

export default function AgentsPage() {
  const navigate = useStore((s) => s.navigateFn);
  const agents = useStore((s) => s.agents);
  const setAgents = useStore((s) => s.setAgents);
  const updateAgentLocal = useStore((s) => s.updateAgent);
  const removeAgent = useStore((s) => s.removeAgent);
  const setSelectedAgentId = useStore((s) => s.setSelectedAgentId);
  const setRightSidebarTab = useStore((s) => s.setRightSidebarTab);
  const setRightSidebarOpen = useStore((s) => s.setRightSidebarOpen);

  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [newDescription, setNewDescription] = useState('');
  const [newPrompt, setNewPrompt] = useState('');

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState('');
  const [editDescription, setEditDescription] = useState('');
  const [editPrompt, setEditPrompt] = useState('');
  const [saving, setSaving] = useState(false);

  function selectAgent(id: string) {
    setSelectedAgentId(id);
    setRightSidebarTab('info');
    setRightSidebarOpen(true);
  }

  async function handleCreate() {
    if (!newName.trim() || !newPrompt.trim()) return;
    setCreating(true);
    try {
      await createAgent(newName.trim(), newPrompt.trim(), newDescription.trim() || undefined, []);
      setAgents(await listAgents());
      setNewName('');
      setNewDescription('');
      setNewPrompt('');
    } finally {
      setCreating(false);
    }
  }

  function startEdit(agent: (typeof agents)[number]) {
    setEditingId(agent.id);
    setEditName(agent.name);
    setEditDescription(agent.spec.description || '');
    setEditPrompt(agent.spec.prompt);
  }

  async function handleSaveEdit(id: string) {
    if (!editName.trim() || !editPrompt.trim()) return;
    setSaving(true);
    try {
      const updated = await updateAgent(id, editName.trim(), editPrompt.trim(), editDescription.trim() || undefined, []);
      updateAgentLocal(updated);
      setEditingId(null);
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(id: string) {
    if (!confirm('Delete this agent?')) return;
    try {
      await deleteAgent(id);
      removeAgent(id);
    } catch {
      // ignore
    }
  }

  return (
    <div className="agents-page">
      <div className="agents-inner">
        <div className="agents-header">
          <h2>Agents</h2>
          <div className="agents-actions">
            <button className="btn" onClick={() => navigate('/chat')}>Open chat</button>
          </div>
        </div>

        <div className="agents-create-form">
          <h3>Create agent</h3>
          <input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="Name" />
          <input value={newDescription} onChange={(e) => setNewDescription(e.target.value)} placeholder="Description" />
          <textarea value={newPrompt} onChange={(e) => setNewPrompt(e.target.value)} placeholder="System prompt" rows={3} />
          <button onClick={handleCreate} disabled={creating || !newName.trim() || !newPrompt.trim()}>
            {creating ? 'Creating...' : 'Create agent'}
          </button>
        </div>

        <div className="agents-list">
          {agents.length === 0 && <p className="agents-empty">No agents registered.</p>}
          {agents.map((agent) => (
            <div
              key={agent.id}
              data-testid="agent-card"
              className="agent-card"
              onClick={() => selectAgent(agent.id)}
            >
              <div className="agent-card-avatar">
                {agent.name.slice(0, 2).toUpperCase()}
              </div>
            <div className="agent-card-body">
              {editingId === agent.id ? (
                <div className="agent-edit-form" onClick={(e) => e.stopPropagation()}>
                  <input value={editName} onChange={(e) => setEditName(e.target.value)} placeholder="Name" />
                  <input value={editDescription} onChange={(e) => setEditDescription(e.target.value)} placeholder="Description" />
                  <textarea value={editPrompt} onChange={(e) => setEditPrompt(e.target.value)} placeholder="System prompt" rows={3} />
                  <div className="agent-edit-actions">
                    <button onClick={() => handleSaveEdit(agent.id)} disabled={saving || !editName.trim() || !editPrompt.trim()}>
                      {saving ? 'Saving...' : 'Save'}
                    </button>
                    <button onClick={() => setEditingId(null)}>Cancel</button>
                  </div>
                </div>
              ) : (
                <>
                  <div className="agent-card-name">{agent.name}</div>
                  <div className="agent-card-desc">{agent.spec.description || agent.spec.prompt}</div>
                  <div className="agent-card-tags">
                    {(agent.spec.tools || []).map((tag: string) => (
                      <span key={tag} className="agent-tag">{tag}</span>
                    ))}
                  </div>
                </>
              )}
            </div>
            <div className="agent-card-actions" onClick={(e) => e.stopPropagation()}>
              <button className="agent-card-btn" title="Chat" onClick={() => navigate(`/chat?agent=${agent.id}`)}>Chat</button>
              <button className="agent-card-btn" title="Edit" onClick={() => startEdit(agent)}>Edit</button>
              <button className="agent-card-btn danger" title="Delete" onClick={() => handleDelete(agent.id)}>Delete</button>
            </div>
          </div>
        ))}
      </div>
      </div>
    </div>
  );
}
