import { useStore } from '../stores/appStore';

export default function ConnectorsPage() {
  const workers = useStore((s) => s.workers);

  return (
    <div className="page">
      <div className="page-header">
        <h2>Connectors</h2>
      </div>
      <div className="page-content">
        {workers.length === 0 && <p>No workers configured. Add a worker from the sidebar.</p>}
        {workers.map((w) => (
          <div key={w.id} className="card">
            <div className="card-title">{w.name}</div>
            <div className="card-row">URL: {w.url}</div>
            <div className="card-row">Status: {w.paired ? 'paired' : 'unpaired'}</div>
            <div className="card-row">ID: {w.id}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
