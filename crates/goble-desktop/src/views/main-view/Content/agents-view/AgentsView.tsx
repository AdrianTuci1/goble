import { useMainViewStore } from '../../store/mainViewStore';
import { agentsData } from '../../data/agentsData';
import { useNavigate } from 'react-router-dom';
import { Sparkles } from 'lucide-react';
import './AgentsView.css';

export default function AgentsView() {
  const navigate = useNavigate();
  const { selectAgent, openRight } = useMainViewStore();

  function openAgent(agentId: string) {
    selectAgent(agentId);
    openRight('info');
  }

  function startAgent(agentId: string) {
    navigate(`/main/chat?agent=${agentId}`);
  }

  return (
    <div className="agents-view">
      <div className="agents-header">
        <h2>Agents</h2>
        <p>Pick an agent to start a task.</p>
      </div>
      <div className="agents-grid">
        {agentsData.map((agent) => (
          <div key={agent.id} className="agent-card" onClick={() => openAgent(agent.id)}>
            <div className="agent-card-header">
              <div className="agent-avatar" style={{ background: agent.color }}>{agent.initials}</div>
              <div className="agent-card-title">{agent.name}</div>
            </div>
            <div className="agent-card-body">{agent.description}</div>
            <div className="agent-card-tags">
              {agent.tags.map((t) => <span key={t} className="agent-tag">{t}</span>)}
            </div>
            <button className="agent-card-action" onClick={(e) => { e.stopPropagation(); startAgent(agent.id); }}>
              <Sparkles size={14} /> Start
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
