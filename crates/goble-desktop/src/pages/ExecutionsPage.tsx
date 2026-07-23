import { useStore } from '../stores/appStore';

export default function ExecutionsPage() {
  const executions = useStore((s) => s.executions);

  return (
    <div className="page">
      <div className="page-header">
        <h2>Executions</h2>
      </div>
      <div className="page-content">
        {executions.length === 0 && <p>No executions recorded.</p>}
        {executions.map((e) => (
          <div key={e.id} className="card">
            <div className="card-title">Trace {e.id.slice(0, 8)}</div>
            <div className="card-row">Status: {e.status}</div>
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
