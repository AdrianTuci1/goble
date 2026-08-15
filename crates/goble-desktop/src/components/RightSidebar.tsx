import { useStore } from '../stores/appStore';
import { useNavigate } from 'react-router-dom';
import type { AgentInfo } from '../tauri/api';
import './RightSidebar.css';

function formatTime(iso: string) {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

export default function RightSidebar() {
  const open = useStore((s) => s.rightSidebarOpen);
  const tab = useStore((s) => s.rightSidebarTab);
  const setTab = useStore((s) => s.setRightSidebarTab);
  const setOpen = useStore((s) => s.setRightSidebarOpen);

  if (!open) return null;

  return (
    <aside className="right-sidebar active">
      <div className="right-sidebar-header">
        <div className="right-sidebar-tabs">
          <button
            className={`right-sidebar-tab ${tab === 'info' ? 'active' : ''}`}
            onClick={() => setTab('info')}
          >
            Info
          </button>
          <button
            className={`right-sidebar-tab ${tab === 'history' ? 'active' : ''}`}
            onClick={() => setTab('history')}
          >
            History
          </button>
        </div>
        <button className="right-sidebar-close" onClick={() => setOpen(false)} title="Close">×</button>
      </div>
      <div className="right-sidebar-content">
        {tab === 'info' ? <InfoPanel /> : <HistoryPanel />}
      </div>
    </aside>
  );
}

function InfoPanel() {
  const selectedAgentId = useStore((s) => s.selectedAgentId);
  const agents = useStore((s) => s.agents);
  const activeConversation = useStore((s) =>
    s.activeConversationId ? s.conversations.find((c) => c.id === s.activeConversationId) : null
  );
  const flows = useStore((s) => s.flows);
  const selectedFlowId = useStore((s) => s.selectedFlowId);

  const agent = selectedAgentId ? agents.find((a) => a.id === selectedAgentId) || null : null;
  const flow = selectedFlowId ? flows.find((f) => f.id === selectedFlowId) || null : null;

  if (flow) {
    const integrations = flow.meta.integrations as string[];
    return (
      <>
        <div className="panel-section">
          <div className="panel-label">Flow</div>
          <div className="panel-value">{flow.title}</div>
        </div>
        <div className="panel-section">
          <div className="panel-label">Created by</div>
          <div className="panel-value">{flow.meta.createdBy}</div>
        </div>
        <div className="panel-section">
          <div className="panel-label">Integrations</div>
          <div className="panel-tags">
            {(integrations as string[]).map((tag: any) => (
              <span key={tag} className="panel-tag">{tag}</span>
            ))}
          </div>
        </div>
        <div className="panel-section">
          <div className="panel-label">Schedule</div>
          <div className="panel-code">{flow.meta.cron}</div>
        </div>
      </>
    );
  }

  if (agent) {
    return <AgentInfoPanel agent={agent} />;
  }

  if (activeConversation) {
    return (
      <>
        <div className="panel-section">
          <div className="panel-label">Conversation</div>
          <div className="panel-value">{activeConversation.title}</div>
        </div>
        <div className="panel-section">
          <div className="panel-label">Model</div>
          <div className="panel-value">
            {activeConversation.provider || '-'}/{activeConversation.model || '-'}
          </div>
        </div>
      </>
    );
  }

  return <div className="panel-placeholder">Select a conversation, agent or flow to see details.</div>;
}

function AgentInfoPanel({ agent }: { agent: AgentInfo }) {
  const navigate = useNavigate();
  const executions = useStore((s) => s.executions);
  const setHistoryDetailId = useStore((s) => s.setHistoryDetailId);
  const setRightSidebarTab = useStore((s) => s.setRightSidebarTab);

  const agentExecutions = [...executions]
    .filter((e) => e.agent_id === agent.id)
    .sort((a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime())
    .slice(0, 10);

  return (
    <>
      <div className="panel-section">
        <div className="panel-label">Agent</div>
        <div className="panel-value">{agent.name}</div>
      </div>
      <div className="panel-section">
        <div className="panel-label">Description</div>
        <div className="panel-value">{agent.spec.description || '-'}</div>
      </div>
      <div className="panel-section">
        <div className="panel-label">Prompt</div>
        <div className="panel-code prompt">{agent.spec.prompt}</div>
      </div>
      <div className="panel-section">
        <div className="panel-label">Tools</div>
        <div className="panel-tags">
          {agent.spec.tools.length > 0 ? (
            agent.spec.tools.map((t: string) => <span key={t} className="panel-tag">{t}</span>)
          ) : (
            <span className="panel-placeholder">no tools configured</span>
          )}
        </div>
      </div>
      <div className="panel-section">
        <button className="btn" onClick={() => navigate(`/chat?agent=${agent.id}`)}>Chat with {agent.name}</button>
      </div>
      <div className="panel-section">
        <div className="panel-label">Recent executions</div>
        {agentExecutions.length === 0 ? (
          <span className="panel-placeholder">No executions yet</span>
        ) : (
          <div className="panel-history-list">
            {agentExecutions.map((e) => (
              <button
                key={e.id}
                className="panel-history-item"
                onClick={() => {
                  setHistoryDetailId(e.id);
                  setRightSidebarTab('history');
                }}
              >
                <span className="panel-history-time">{formatTime(e.started_at)}</span>
                <span className="panel-history-label">{e.status}</span>
              </button>
            ))}
          </div>
        )}
      </div>
    </>
  );
}

function HistoryPanel() {
  const executions = useStore((s) => s.executions);
  const historyDetailId = useStore((s) => s.historyDetailId);
  const setHistoryDetailId = useStore((s) => s.setHistoryDetailId);
  const agents = useStore((s) => s.agents);
  const workers = useStore((s) => s.workers);

  const exec = historyDetailId ? executions.find((e) => e.id === historyDetailId) || null : null;

  if (exec) {
    return (
      <>
        <button className="back-btn" onClick={() => setHistoryDetailId(null)}>← Back</button>
        <div className="panel-section">
          <div className="panel-label">Execution</div>
          <div className="panel-value">{exec.id}</div>
        </div>
        <div className="panel-section">
          <div className="panel-label">Status</div>
          <div className="panel-value">{exec.status}</div>
        </div>
        <div className="panel-section">
          <div className="panel-label">Started</div>
          <div className="panel-value">{formatTime(exec.started_at)}</div>
        </div>
        <div className="panel-section">
          <div className="panel-label">Agent</div>
          <div className="panel-value">
            {agents.find((a) => a.id === exec.agent_id)?.name || exec.agent_id || '-'}
          </div>
        </div>
        <div className="panel-section">
          <div className="panel-label">Worker</div>
          <div className="panel-value">
            {workers.find((w) => w.id === exec.worker_id)?.name || exec.worker_id || '-'}
          </div>
        </div>
      </>
    );
  }

  if (executions.length === 0) {
    return <div className="panel-placeholder">No executions yet</div>;
  }

  const sorted = [...executions].sort(
    (a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime()
  );

  return (
    <div className="panel-history-list">
      {sorted.slice(0, 50).map((e) => (
        <button
          key={e.id}
          className="panel-history-item"
          onClick={() => setHistoryDetailId(e.id)}
        >
          <span className="panel-history-time">{formatTime(e.started_at)}</span>
          <span className="panel-history-label">
            {agents.find((a) => a.id === e.agent_id)?.name || e.id} — {e.status}
          </span>
        </button>
      ))}
    </div>
  );
}
