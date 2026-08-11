import { useStore } from '../stores/appStore';
import './Pages.css';

export default function AgentTracePage() {
  const executions = useStore((s) => s.executions);
  const agentStates = useStore((s) => s.agentStates);
  const agentToolResults = useStore((s) => s.agentToolResults);
  const selectedTraceId = useStore((s) => s.selectedTraceId);
  const setSelectedTraceId = useStore((s) => s.setSelectedTraceId);

  function selectTrace(id: string) {
    setSelectedTraceId(id);
  }

  const state = selectedTraceId ? agentStates[selectedTraceId] : null;
  const toolResults = selectedTraceId ? agentToolResults[selectedTraceId] || [] : [];
  const execution = selectedTraceId ? executions.find((e) => e.id === selectedTraceId) : null;

  return (
    <div className="page">
      <div className="page-header">
        <h2>Agent Traces</h2>
      </div>
      <div className="page-content trace-page">
        <div className="trace-list">
          {executions.length === 0 && <p>No executions recorded.</p>}
          {executions.map((e) => (
            <button
              key={e.id}
              className={`trace-item ${selectedTraceId === e.id ? 'selected' : ''}`}
              onClick={() => selectTrace(e.id)}
            >
              <div className="trace-item-title">Trace {e.id.slice(0, 8)}</div>
              <div className={`status-badge ${statusClass(e.status)}`}>{e.status}</div>
              <div className="trace-item-meta">{e.agent_id || 'n/a'}</div>
            </button>
          ))}
        </div>

        <div className="trace-detail">
          {!selectedTraceId && <p>Select a trace to view runtime state.</p>}
          {execution && (
            <>
              <div className="card">
                <div className="card-title">Execution {execution.id.slice(0, 8)}</div>
                <div className={`status-badge ${statusClass(execution.status)}`}>{execution.status}</div>
                <div className="card-row">Agent: {execution.agent_id || 'n/a'}</div>
                <div className="card-row">Worker: {execution.worker_id || 'n/a'}</div>
                <div className="card-row">Started: {execution.started_at}</div>
                {execution.finished_at && (
                  <div className="card-row">Finished: {execution.finished_at}</div>
                )}
              </div>

              {state && (
                <div className="card">
                  <div className="card-title">Checklist</div>
                  {state.checklist.length === 0 && <p>No checklist items.</p>}
                  <ul className="checklist">
                    {state.checklist.map((item) => (
                      <li key={item.id} className={item.done ? 'done' : ''}>
                        <input type="checkbox" checked={item.done} readOnly />
                        <span>{item.text}</span>
                      </li>
                    ))}
                  </ul>

                  <div className="card-title">Notes</div>
                  {state.notes.length === 0 && <p>No notes.</p>}
                  <ul className="note-list">
                    {state.notes.map((note, i) => (
                      <li key={i}>{note}</li>
                    ))}
                  </ul>

                  <div className="card-title">Self-feedback</div>
                  {state.self_feedback.length === 0 && <p>No self-feedback yet.</p>}
                  <ul className="note-list">
                    {state.self_feedback.map((fb, i) => (
                      <li key={i}>{fb}</li>
                    ))}
                  </ul>
                </div>
              )}

              <div className="card">
                <div className="card-title">Tool results ({toolResults.length})</div>
                {toolResults.length === 0 && <p>No tool results received.</p>}
                <div className="tool-results">
                  {toolResults.map((r, i) => (
                    <div key={i} className="tool-result">
                      <div className="tool-result-name">{r.name}</div>
                      <pre className="tool-result-output">{r.result}</pre>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function statusClass(status: string) {
  switch (status) {
    case 'running':
      return 'status-running';
    case 'success':
      return 'status-success';
    case 'error':
      return 'status-error';
    default:
      return 'status-other';
  }
}
