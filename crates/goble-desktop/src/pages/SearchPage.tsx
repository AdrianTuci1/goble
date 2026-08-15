import { useState } from 'react';
import { useStore } from '../stores/appStore';
import './Pages.css';

export default function SearchPage() {
  const conversations = useStore((s) => s.conversations);
  const threads = useStore((s) => s.threads);
  const agents = useStore((s) => s.agents);
  const vaultSecrets = useStore((s) => s.vaultSecrets);
  const [query, setQuery] = useState('');

  const q = query.toLowerCase().trim();
  const convResults = q ? conversations.filter((c) => c.title.toLowerCase().includes(q)) : [];
  const threadResults = q ? threads.filter((t) => t.title.toLowerCase().includes(q)) : [];
  const agentResults = q ? agents.filter((a) => a.name.toLowerCase().includes(q) || a.spec.description.toLowerCase().includes(q)) : [];
  const vaultResults = q ? vaultSecrets.filter((s) => s.key.toLowerCase().includes(q)) : [];

  return (
    <div className="page">
      <div className="page-header">
        <h2>Search</h2>
      </div>
      <div className="page-content">
        <input
          className="search-input"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search conversations..."
        />
        <div className="search-results">
          {q && convResults.length === 0 && threadResults.length === 0 && agentResults.length === 0 && vaultResults.length === 0 && (
            <p className="empty">No results found.</p>
          )}
          {convResults.map((c) => (
            <div key={`conv-${c.id}`} className="card">
              <div className="card-title">💬 {c.title}</div>
              <div className="card-row">Conversation • Updated: {c.updated_at}</div>
            </div>
          ))}
          {threadResults.map((t) => (
            <div key={`thread-${t.id}`} className="card">
              <div className="card-title">📥 {t.title}</div>
              <div className="card-row">Thread • {t.kind}</div>
            </div>
          ))}
          {agentResults.map((a) => (
            <div key={`agent-${a.id}`} className="card">
              <div className="card-title">🤖 {a.name}</div>
              <div className="card-row">Agent • {a.spec.description || a.spec.prompt.slice(0, 80)}</div>
            </div>
          ))}
          {vaultResults.map((s) => (
            <div key={`vault-${s.key}`} className="card">
              <div className="card-title">🔒 {s.key}</div>
              <div className="card-row">Vault secret • Updated: {s.updated_at}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
