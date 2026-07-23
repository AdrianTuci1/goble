import { useStore } from '../stores/appStore';

export default function ConnectorsPage() {
  const workers = useStore((s) => s.workers);

  return (
    <div style={{ padding: 24, overflowY: 'auto', height: '100%' }}>
      <h1 style={{ fontSize: 24, fontWeight: 600, marginBottom: 24 }}>Connectori / Workeri</h1>
      {workers.length === 0 ? (
        <div style={{ color: '#737373' }}>Niciun worker conectat. Adaugă un worker din CLI folosind goble-cli.</div>
      ) : (
        workers.map((w) => (
          <div
            key={w.id}
            style={{
              padding: 16,
              borderRadius: 12,
              background: '#111111',
              border: '1px solid #1f1f1f',
              marginBottom: 12,
            }}
          >
            <div style={{ fontSize: 16, fontWeight: 500, marginBottom: 4 }}>{w.name}</div>
            <div style={{ fontSize: 13, color: '#a3a3a3' }}>ID: {w.id}</div>
            <div style={{ fontSize: 13, color: '#a3a3a3' }}>URL: {w.url}</div>
            <div style={{ fontSize: 13, color: w.paired ? '#22c55e' : '#737373', marginTop: 8 }}>
              {w.paired ? 'Paired' : 'Unpaired'}
            </div>
          </div>
        ))
      )}
    </div>
  );
}
