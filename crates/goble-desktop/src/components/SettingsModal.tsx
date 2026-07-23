import { useStore } from '../stores/appStore';

export default function SettingsModal() {
  const isOpen = useStore((s) => s.isSettingsOpen);
  const setOpen = useStore((s) => s.setSettingsOpen);
  const workers = useStore((s) => s.workers);
  const logs = useStore((s) => s.logs);

  if (!isOpen) return null;

  return (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0,0,0,0.7)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
      }}
      onClick={() => setOpen(false)}
    >
      <div
        style={{
          width: 560,
          maxHeight: '80vh',
          background: '#111111',
          border: '1px solid #1f1f1f',
          borderRadius: 12,
          padding: 24,
          overflow: 'auto',
          color: '#e5e5e5',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 style={{ marginBottom: 16, fontSize: 18, fontWeight: 600 }}>Setări</h2>

        <h3 style={{ fontSize: 14, fontWeight: 500, marginBottom: 8, color: '#a3a3a3' }}>Workeri conectați</h3>
        <div style={{ marginBottom: 16 }}>
          {workers.length === 0 ? (
            <div style={{ fontSize: 13, color: '#737373' }}>Niciun worker conectat.</div>
          ) : (
            workers.map((w) => (
              <div
                key={w.id}
                style={{
                  padding: 10,
                  borderRadius: 8,
                  background: '#0a0a0a',
                  border: '1px solid #1f1f1f',
                  marginBottom: 8,
                  fontSize: 13,
                }}
              >
                <div><strong>{w.name}</strong> <span style={{ color: '#737373' }}>({w.id})</span></div>
                <div style={{ color: '#737373' }}>{w.url} {w.paired ? '✓ paired' : '○ unpaired'}</div>
              </div>
            ))
          )}
        </div>

        <h3 style={{ fontSize: 14, fontWeight: 500, marginBottom: 8, color: '#a3a3a3' }}>Loguri recente</h3>
        <div style={{ background: '#0a0a0a', borderRadius: 8, padding: 12, fontSize: 12, fontFamily: 'monospace', maxHeight: 240, overflowY: 'auto' }}>
          {logs.length === 0 ? (
            <div style={{ color: '#737373' }}>Niciun log.</div>
          ) : (
            logs.slice(-50).map((log) => (
              <div key={log.id} style={{ marginBottom: 4, color: '#a3a3a3' }}>
                <span style={{ color: '#525252' }}>{new Date(log.timestamp).toLocaleTimeString()}</span> {log.message}
              </div>
            ))
          )}
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 16 }}>
          <button
            onClick={() => setOpen(false)}
            style={{
              padding: '8px 16px',
              borderRadius: 8,
              border: 'none',
              background: '#e5e5e5',
              color: '#0a0a0a',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            Închide
          </button>
        </div>
      </div>
    </div>
  );
}
