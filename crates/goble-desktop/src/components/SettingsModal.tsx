import { useStore } from '../stores/appStore';
import { workerLogs } from '../tauri/api';

export default function SettingsModal() {
  const isOpen = useStore((s) => s.isSettingsOpen);
  const setOpen = useStore((s) => s.setSettingsOpen);
  const logs = useStore((s) => s.logs);

  if (!isOpen) return null;

  return (
    <div className="modal-overlay" onClick={() => setOpen(false)}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h3>Settings</h3>
          <button onClick={() => setOpen(false)}>Close</button>
        </div>
        <div className="modal-body">
          <div className="settings-section">
            <h4>Logs</h4>
            <button onClick={() => workerLogs().then((l) => useStore.getState().setLogs(l))}>
              Refresh
            </button>
            <div className="log-list">
              {logs.slice(-50).map((log) => (
                <div key={log.id} className="log-entry">
                  <span className="log-time">{log.timestamp}</span>
                  <span className="log-message">{log.message}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
