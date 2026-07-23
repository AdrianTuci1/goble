import { useState } from 'react';
import { useStore } from '../stores/appStore';

export default function SearchPage() {
  const conversations = useStore((s) => s.conversations);
  const [query, setQuery] = useState('');
  const filtered = query
    ? conversations.filter((c) => c.title.toLowerCase().includes(query.toLowerCase()))
    : conversations;

  return (
    <div style={{ padding: 24, overflowY: 'auto', height: '100%' }}>
      <h1 style={{ fontSize: 24, fontWeight: 600, marginBottom: 16 }}>Caută conversații</h1>
      <input
        type="text"
        placeholder="Caută…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        style={{
          width: '100%',
          maxWidth: 400,
          padding: '10px 14px',
          background: '#111111',
          border: '1px solid #1f1f1f',
          borderRadius: 8,
          color: '#e5e5e5',
          fontSize: 14,
          marginBottom: 24,
          outline: 'none',
        }}
      />
      {filtered.length === 0 ? (
        <div style={{ color: '#737373' }}>Nicio conversație găsită.</div>
      ) : (
        filtered.map((c) => (
          <div
            key={c.id}
            style={{
              padding: 12,
              borderRadius: 8,
              background: '#111111',
              border: '1px solid #1f1f1f',
              marginBottom: 8,
              fontSize: 14,
            }}
          >
            {c.title}
          </div>
        ))
      )}
    </div>
  );
}
