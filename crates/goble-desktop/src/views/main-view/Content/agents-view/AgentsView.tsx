import { useMainViewStore } from '../../store/mainViewStore';
import { agentsData } from '../../data/agentsData';
import './AgentsView.css';

export default function AgentsView() {
  const { selectAgent, openRight } = useMainViewStore();

  function openAgent(agentId: string) {
    selectAgent(agentId);
    openRight('info');
  }

  return (
    <div className="agents-view">
      <div className="agents-view-header">
        <div>
          <h2>Agents</h2>
          <p className="agents-view-subtitle">Choose an agent to start a chat.</p>
        </div>
        <button className="add-agent-btn" onClick={() => openRight('info')}>
          + Add agent
        </button>
      </div>
      <div className="agents-grid">
        {agentsData.map((agent) => (
          <div key={agent.id} className="agent-card" onClick={() => openAgent(agent.id)}>
            <div className="agent-card-name">{agent.name}</div>
            <div className="agent-card-description">{agent.description}</div>
          </div>
        ))}
      </div>
    </div>
  );
}
