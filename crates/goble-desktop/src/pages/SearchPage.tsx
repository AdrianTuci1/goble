import { useState } from 'react';
import { useStore } from '../stores/appStore';
import './Pages.css';

export default function SearchPage() {
  const conversations = useStore((s) => s.conversations);
  const [query, setQuery] = useState('');

  const filtered = query
    ? conversations.filter((c) => c.title.toLowerCase().includes(query.toLowerCase()))
    : conversations;

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
          {filtered.map((c) => (
            <div key={c.id} className="card">
              <div className="card-title">{c.title}</div>
              <div className="card-row">Updated: {c.updated_at}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
