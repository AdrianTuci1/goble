import { useStore } from '../stores/appStore';
import { listExecutions } from '../tauri/api';

function statusClass(status: string) {
  switch (status) {
    case 'running': return 'status-running';
    case 'success': return 'status-success';
    case 'error': return 'status-error';
    default: return 'status-other';
  }
}

export default function ExecutionsPage() {
  const executions = useStore((s) => s.executions);

  return (
    <div className="page">
      <div className="page-header">
        <h2>Executions</h2>
        <button onClick={() => listExecutions().then((e) => useStore.getState().setExecutions(e))}>
          Refresh
        </button>
      </div>
      <div className="page-content">
        {executions.length === 0 && <p>No executions recorded.</p>}
        {executions.map((e) => (
          <div key={e.id} className="card">
            <div className="card-title">Trace {e.id.slice(0, 8)}</div>
            <div className={`status-badge ${statusClass(e.status)}`}>{e.status}</div>
            <div className="card-row">Agent: {e.agent_id || 'n/a'}</div>
            <div className="card-row">Worker: {e.worker_id || 'n/a'}</div>
            <div className="card-row">Started: {e.started_at}</div>
            {e.finished_at && <div className="card-row">Finished: {e.finished_at}</div>}
          </div>
        ))}
      </div>
    </div>
  );
}
