import { useStore } from '../stores/appStore';
import { agentsData, type Agent } from '../mocks/agentsData';
import './AgentsPage.css';

export default function AgentsPage() {
  const navigate = useStore((s) => s.navigateFn);
  const selectedAgentId = useStore((s) => s.selectedAgentId);
  const setSelectedAgentId = useStore((s) => s.setSelectedAgentId);
  const setRightSidebarTab = useStore((s) => s.setRightSidebarTab);
  const setRightSidebarOpen = useStore((s) => s.setRightSidebarOpen);

  function selectAgent(id: string) {
    setSelectedAgentId(id);
    setRightSidebarTab('info');
    setRightSidebarOpen(true);
  }

  return (
    <div className="agents-page">
      <div className="agents-header">
        <h2>Agents</h2>
        <div className="agents-actions">
          <button className="btn" onClick={() => navigate('/chat')}>Open chat</button>
        </div>
      </div>
      <div className="agents-list">
        {agentsData.map((agent: Agent) => (
          <div
            key={agent.id}
            data-testid="agent-card"
            className={`agent-card ${selectedAgentId === agent.id ? 'selected' : ''}`}
            onClick={() => selectAgent(agent.id)}
          >
            <div className="agent-card-avatar" style={{ background: agent.color }}>
              {agent.initials}
            </div>
            <div className="agent-card-body">
              <div className="agent-card-name">{agent.name}</div>
              <div className="agent-card-desc">{agent.description}</div>
              <div className="agent-card-tags">
                {(agent.tags || []).map((tag: string) => (
                  <span key={tag} className="agent-tag">{tag}</span>
                ))}
              </div>
            </div>
            <div className="agent-card-actions">
              <button className="agent-card-btn" title="Chat" onClick={() => navigate('/chat')}>Chat</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
