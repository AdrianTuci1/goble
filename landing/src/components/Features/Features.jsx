import './Features.css';

const Features = () => {
  const features = [
    {
      title: "Spotlight-style launcher",
      description: "Open files, commands, and workflows from a fast terminal-first launcher with keyboard-native control.",
      icon: <img src="/icons/token.svg" alt="Launcher" className="feature-icon-img" />
    },
    {
      title: "Smart command autocomplete",
      description: "Keep moving with intelligent completions that understand your context and help finish commands faster.",
      icon: <img src="/icons/crossword.svg" alt="Autocomplete" className="feature-icon-img" />
    },
    {
      title: "Agent mode on demand",
      description: "Switch from normal terminal usage to natural-language execution whenever you want Octomus to do the work.",
      icon: <img src="/icons/borg.svg" alt="Agents" className="feature-icon-img" />
    }
  ];

  return (
    <section className="features-section">
      <div className="features-header-wrapper">
        <h2 className="features-title">
          Move from idea to implementation faster with one workspace for commands, editing, and agents
        </h2>
      </div>
      <div className="features-grid">
        {features.map((feature, index) => (
          <div key={index} className="feature-item">
            <div className="feature-icon-placeholder">
              {feature.icon}
            </div>
            <div className="feature-info">
              <h3 className="feature-name">{feature.title}</h3>
              <p className="feature-description">{feature.description}</p>
            </div>
          </div>
        ))}
      </div>
    </section>
  );
};

export default Features;
