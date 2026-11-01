import './Compliance.css';

const Compliance = () => {
  return (
    <section className="compliance-section">
      <div className="compliance-container">
        {/* Label Row */}
        <div className="compliance-label-row">
          <div className="label-wrapper">
            <span className="section-label">MODEL & AGENT ECOSYSTEM</span>
          </div>
        </div>

        {/* Content Row */}
        <div className="compliance-main-row">
          <div className="title-col">
            <h2 className="compliance-title">
              Run local and<br />
              cloud agents<br />
              with confidence.
            </h2>
          </div>

          <div className="partners-col">
            <div className="partner-item">
              <img src="/llms/claude.png" alt="Claude" className="partner-logo" />
              <span className="partner-name">Claude</span>
            </div>
            <div className="partner-item">
              <img src="/llms/openai.png" alt="Codex" className="partner-logo logo-openai" />
              <span className="partner-name">Codex</span>
            </div>
            <div className="partner-item">
              <img src="/llms/image.png" alt="Gemini" className="partner-logo" />
              <span className="partner-name">Gemini</span>
            </div>
            <div className="partner-item">
              <img src="/llms/any-llm.png" alt="Any LLM" className="partner-logo" />
              <span className="partner-name">Any LLM</span>
            </div>
          </div>

          <div className="desc-col">
            <div className="desc-wrapper">
              <div className="vertical-line"></div>
              <p className="compliance-desc-text">
                Octomus keeps model choice flexible,<br />
                lets you run multiple agents in<br />
                parallel, and makes every action<br />
                visible in one workspace.
              </p>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
};

export default Compliance;
