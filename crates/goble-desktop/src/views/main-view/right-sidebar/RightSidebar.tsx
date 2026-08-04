import { useMainViewStore, type ExecutionRecord } from '../store/mainViewStore';
import { getAgentById } from '../data/agentsData';
import { getFlowById } from '../data/flowsData';
import { X } from 'lucide-react';
import './RightSidebar.css';

export default function RightSidebar() {
  const { rightOpen, rightTab, closeRight, setRightTab, executions } = useMainViewStore();
  if (!rightOpen) return null;

  return (
    <aside className="right-sidebar" aria-label="Right sidebar">
      <div className="right-sidebar-header">
        <div className="right-sidebar-tabs">
          <button className={`right-sidebar-tab ${rightTab === 'info' ? 'active' : ''}`} onClick={() => setRightTab('info')}>
            Info
          </button>
          <button className={`right-sidebar-tab ${rightTab === 'history' ? 'active' : ''}`} onClick={() => setRightTab('history')}>
            History
          </button>
        </div>
        <button className="right-sidebar-close" onClick={closeRight} aria-label="Close">
          <X size={18} />
        </button>
      </div>
      <div className="right-sidebar-content">
        {rightTab === 'info' ? <InfoPanel /> : <HistoryPanel executions={executions} />}
      </div>
    </aside>
  );
}

function InfoPanel() {
  const { selectedAgentId, selectedFlowId } = useMainViewStore();
  const agent = selectedAgentId ? getAgentById(selectedAgentId) : null;
  const flow = selectedFlowId ? getFlowById(selectedFlowId) : null;

  if (flow) {
    return (
      <>
        <div className="right-panel-section"><div className="right-panel-label">Flow</div><div className="right-panel-value">{flow.title}</div></div>
        <div className="right-panel-section"><div className="right-panel-label">Created by</div><div className="right-panel-value">{flow.meta.createdBy}</div></div>
        <div className="right-panel-section"><div className="right-panel-label">Integrations</div><div className="right-panel-tags">{flow.meta.integrations.map((i) => <span key={i} className="right-panel-tag">{i}</span>)}</div></div>
        <div className="right-panel-section"><div className="right-panel-label">Schedule</div><div className="right-panel-code">{flow.meta.cron}</div></div>
      </>
    );
  }

  if (agent) {
    return (
      <>
        <div className="right-panel-section"><div className="right-panel-label">Agent</div><div className="right-panel-value">{agent.name}</div></div>
        <div className="right-panel-section"><div className="right-panel-label">Description</div><div className="right-panel-value">{agent.description}</div></div>
        <div className="right-panel-section"><div className="right-panel-label">Tags</div><div className="right-panel-tags">{agent.tags.map((t) => <span key={t} className="right-panel-tag">{t}</span>)}</div></div>
      </>
    );
  }

  return <div className="right-panel-placeholder">Select an agent or flow to see details.</div>;
}

function HistoryPanel({ executions }: { executions: ExecutionRecord[] }) {
  return (
    <div className="right-panel-history">
      {executions.length === 0 ? (
        <div className="right-panel-placeholder">No executions yet.</div>
      ) : (
        executions.map((e) => (
          <div key={e.id} className="right-panel-history-item">
            <span className="right-panel-history-time">{e.time}</span>
            <span className="right-panel-history-label">{e.title}</span>
          </div>
        ))
      )}
    </div>
  );
}
