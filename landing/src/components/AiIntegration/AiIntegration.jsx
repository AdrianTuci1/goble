import './AiIntegration.css';
import PerformanceCard from './cards/PerformanceCard';
import ChatCard from './cards/ChatCard';
import CodeCard from './cards/CodeCard';

const AiIntegration = () => {
  const cards = [
    {
      title: "Model-agnostic by design",
      text: "Use Claude, Codex, Gemini, or any other LLM without changing the way you work.",
      gradient: "/gradients/mask1.png",
      component: <PerformanceCard />,
      accent: "linear-gradient(45deg, #4285f4, #8ab4f8)"
    },
    {
      title: "Natural language execution",
      text: "Describe the task in plain English and let an agent execute commands, edit code, and report back.",
      gradient: "/gradients/mask2.png",
      component: <ChatCard />,
      accent: "linear-gradient(45deg, #34a853, #4ade80)"
    },
    {
      title: "One flow for the whole loop",
      text: "Keep commands, diffs, and agent output in the same workspace so iteration stays fast and visible.",
      gradient: "/gradients/mask3.png",
      component: <CodeCard />,
      accent: "linear-gradient(45deg, #9333ea, #c084fc)"
    }
  ];

  return (
    <section className="ai-section">
      <div className="ai-header-wrapper">
        <div className="ai-top-label">
          MCP + AGENT WORKFLOWS
        </div>
        <h2 className="ai-title">
          Octomus exposes your workspace through MCP so models can act on real code, commands, and context.
        </h2>
      </div>
      
      <div className="ai-cards-grid">
        {cards.map((card, index) => (
          <div key={index} className="ai-card" style={{ backgroundImage: `url(${card.gradient})` }}>
            {/* Decorative accent square/bar */}
            <div className="card-accent" style={{ background: card.accent }}></div>
            
            <div className="ai-card-info">
              <h3 className="ai-card-title">{card.title}</h3>
              <p className="ai-card-text">{card.text}</p>
            </div>
            <div className="ai-card-visual">
              {card.component}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
};

export default AiIntegration;
