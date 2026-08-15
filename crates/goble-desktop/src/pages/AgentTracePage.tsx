import { useEffect, useMemo, useState } from 'react';
import { useStore } from '../stores/appStore';
import { listExecutions, getExecutionTrace, onExecutionsUpdated } from '../tauri/api';
import type { ExecutionInfo, ExecutionTrace, TraceEvent } from '../tauri/api';
import './Pages.css';
import './AgentTracePage.css';

export default function AgentTracePage() {
  const executions = useStore((s) => s.executions);
  const setExecutions = useStore((s) => s.setExecutions);
  const agents = useStore((s) => s.agents);
  const workers = useStore((s) => s.workers);
  const selectedTraceId = useStore((s) => s.selectedTraceId);
  const setSelectedTraceId = useStore((s) => s.setSelectedTraceId);

  const [statusFilter, setStatusFilter] = useState<'all' | 'running' | 'success' | 'error'>('all');
  const [agentFilter, setAgentFilter] = useState<string>('all');
  const [workerFilter, setWorkerFilter] = useState<string>('all');
  const [query, setQuery] = useState('');
  const [traces, setTraces] = useState<Record<string, ExecutionTrace>>({});

  useEffect(() => {
    listExecutions().then(setExecutions);
    let unsub: (() => void) | undefined;
    (async () => {
      unsub = await onExecutionsUpdated(() => listExecutions().then(setExecutions));
    })();
    return () => unsub?.();
  }, [setExecutions]);

  useEffect(() => {
    if (selectedTraceId && !traces[selectedTraceId]) {
      getExecutionTrace(selectedTraceId).then((t) =>
        setTraces((prev) => ({ ...prev, [selectedTraceId]: t }))
      );
    }
  }, [selectedTraceId, traces]);

  const filtered = useMemo(() => {
    let list = [...executions];
    if (statusFilter !== 'all') {
      list = list.filter((e) => e.status.toLowerCase() === statusFilter);
    }
    if (agentFilter !== 'all') {
      list = list.filter((e) => e.agent_id === agentFilter);
    }
    if (workerFilter !== 'all') {
      list = list.filter((e) => e.worker_id === workerFilter);
    }
    if (query.trim()) {
      const q = query.toLowerCase();
      list = list.filter((e) => {
        const text = [e.id, e.agent_id || '', e.worker_id || '', e.status].join(' ').toLowerCase();
        return text.includes(q);
      });
    }
    return list.sort((a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime());
  }, [executions, statusFilter, agentFilter, workerFilter, query]);

  function toggleTrace(id: string) {
    if (selectedTraceId === id) {
      setSelectedTraceId(null);
    } else {
      setSelectedTraceId(id);
      if (!traces[id]) {
        getExecutionTrace(id).then((t) => setTraces((prev) => ({ ...prev, [id]: t })));
      }
    }
  }

  function agentName(id: string | null | undefined) {
    return agents.find((a) => a.id === id)?.name || id || 'n/a';
  }

  function workerName(id: string | null | undefined) {
    return workers.find((w) => w.id === id)?.name || id || 'n/a';
  }

  function formatTime(iso: string) {
    try {
      return new Date(iso).toLocaleString();
    } catch {
      return iso;
    }
  }

  function summaryFor(e: ExecutionInfo) {
    if (e.trace.events.length > 0) {
      const last = e.trace.events[e.trace.events.length - 1];
      if (last.kind === 'assistant_delta') return last.delta.slice(0, 120);
      if (last.kind === 'log') return last.message.slice(0, 120);
      if (last.kind === 'tool_call_started') return `Tool: ${last.name}`;
      if (last.kind === 'tool_call_error') return `Error: ${last.message}`;
    }
    return e.status;
  }

  return (
    <div className="page trace-page">
      <div className="page-header">
        <h2>Agent Traces</h2>
        <div className="trace-filters">
          <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value as 'all' | 'running' | 'success' | 'error')}>
            <option value="all">All statuses</option>
            <option value="running">Running</option>
            <option value="success">Success</option>
            <option value="error">Error</option>
          </select>
          <select value={agentFilter} onChange={(e) => setAgentFilter(e.target.value)}>
            <option value="all">All agents</option>
            {agents.map((a) => (
              <option key={a.id} value={a.id}>{a.name}</option>
            ))}
          </select>
          <select value={workerFilter} onChange={(e) => setWorkerFilter(e.target.value)}>
            <option value="all">All workers</option>
            {workers.map((w) => (
              <option key={w.id} value={w.id}>{w.name}</option>
            ))}
          </select>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search traces..."
          />
        </div>
      </div>
      <div className="page-content">
        {filtered.length === 0 && <p className="empty">No executions match.</p>}
        <div className="trace-stacked-list">
          {filtered.map((e) => (
            <div key={e.id} className={`trace-card ${selectedTraceId === e.id ? 'expanded' : ''}`}>
              <button className="trace-card-header" onClick={() => toggleTrace(e.id)}>
                <div className="trace-card-main">
                  <span className="trace-card-time">{formatTime(e.started_at)}</span>
                  <span className={`status-badge ${statusClass(e.status)}`}>{e.status}</span>
                  <span className="trace-card-agent">{agentName(e.agent_id)}</span>
                  <span className="trace-card-worker">{workerName(e.worker_id)}</span>
                </div>
                <div className="trace-card-summary">{summaryFor(e)}</div>
                <div className="trace-card-chevron">{selectedTraceId === e.id ? '▼' : '▶'}</div>
              </button>
              {selectedTraceId === e.id && (
                <div className="trace-card-body">
                  {traces[e.id] ? (
                    <TraceTimeline trace={traces[e.id]} />
                  ) : (
                    <p>Loading trace...</p>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function TraceTimeline({ trace }: { trace: ExecutionTrace }) {
  function formatTime(iso: string) {
    try {
      return new Date(iso).toLocaleTimeString();
    } catch {
      return iso;
    }
  }

  return (
    <div className="trace-timeline">
      {trace.events.length === 0 && <p className="empty">No trace events recorded.</p>}
      {trace.events.map((event: TraceEvent, i: number) => (
        <div key={i} className={`trace-event trace-event-${event.kind}`}>
          <span className="trace-event-time">{formatTime(event.timestamp)}</span>
          <TraceEventContent event={event} />
        </div>
      ))}
    </div>
  );
}

function TraceEventContent({ event }: { event: TraceEvent }) {
  switch (event.kind) {
    case 'log':
      return (
        <div className={`trace-event-log trace-level-${event.level.toLowerCase()}`}>
          <span className="trace-event-label">{event.level}</span>
          <span>{event.message}</span>
        </div>
      );
    case 'assistant_delta':
      return <div className="trace-event-delta">{event.delta}</div>;
    case 'tool_call_started':
      return (
        <div className="trace-event-tool">
          <span className="trace-event-label">Tool call</span>
          <span>{event.name}</span>
          <pre>{JSON.stringify(event.arguments, null, 2)}</pre>
        </div>
      );
    case 'tool_call_finished':
      return (
        <div className="trace-event-tool">
          <span className="trace-event-label">Tool result</span>
          <pre>{event.result}</pre>
        </div>
      );
    case 'tool_call_error':
      return (
        <div className="trace-event-error">
          <span className="trace-event-label">Tool error</span>
          <span>{event.message}</span>
        </div>
      );
    case 'ask_user':
      return (
        <div className="trace-event-ask">
          <span className="trace-event-label">Ask user</span>
          <span>{event.question}</span>
          <div className="trace-quick-replies">
            {event.quick_replies.map((r: string, i: number) => (
              <span key={i} className="trace-quick-reply">{r}</span>
            ))}
          </div>
        </div>
      );
    case 'done':
      return (
        <div className="trace-event-done">
          <span className="trace-event-label">Done</span>
          <span>{event.status}</span>
        </div>
      );
    default:
      return <pre>{JSON.stringify(event)}</pre>;
  }
}

function statusClass(status: string) {
  const s = status.toLowerCase();
  if (s === 'running' || s === 'pending') return 'status-running';
  if (s === 'success') return 'status-success';
  if (s.includes('error') || s.includes('failure')) return 'status-error';
  if (s === 'cancelled') return 'status-cancelled';
  return 'status-other';
}
