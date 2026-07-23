import { useStore } from '../stores/appStore';

export default function KnowledgePage() {
  const logs = useStore((s) => s.logs);

  return (
    <div style={{ padding: 24, overflowY: 'auto', height: '100%' }}>
      <h1 style={{ fontSize: 24, fontWeight: 600, marginBottom: 24 }}>Knowledge / Loguri</h1>
      <div
        style={{
          background: '#111111',
          border: '1px solid #1f1f1f',
          borderRadius: 12,
          padding: 16,
          fontFamily: 'monospace',
          fontSize: 13,
          maxHeight: 'calc(100% - 80px)',
          overflowY: 'auto',
        }}
      >
        {logs.length === 0 ? (
          <div style={{ color: '#737373' }}>Niciun log disponibil.</div>
        ) : (
          logs.map((log) => (
            <div key={log.id} style={{ marginBottom: 6, color: '#a3a3a3' }}>
              <span style={{ color: '#525252' }}>{new Date(log.timestamp).toLocaleTimeString()}</span>{' '}
              {log.message}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
