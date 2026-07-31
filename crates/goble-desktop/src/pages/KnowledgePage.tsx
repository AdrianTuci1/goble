import { useStore } from '../stores/appStore';
import './Pages.css';

export default function KnowledgePage() {
  const logs = useStore((s) => s.logs);

  return (
    <div className="page">
      <div className="page-header">
        <h2>Knowledge</h2>
      </div>
      <div className="page-content">
        <div className="log-list">
          {logs.slice(-100).map((log) => (
            <div key={log.id} className="log-entry">
              <span className="log-time">{log.timestamp}</span>
              <span className="log-message">{log.message}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
