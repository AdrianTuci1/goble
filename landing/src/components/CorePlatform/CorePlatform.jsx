import './CorePlatform.css';

const CorePlatform = () => {
  const techIcons = [
    { name: 'TypeScript', dot: '#3178c6', icon: 'ts.png' },
    { name: 'Python', dot: '#3776ab', icon: 'py.png' },
    { name: 'Go', dot: '#00add8', icon: 'go.png' },
    { name: 'Rust', dot: '#dea584', icon: 'rust.png' },
    { name: 'JavaScript', dot: '#f7df1e', icon: 'js.png' },
    { name: 'Bash', dot: '#4eaa25', icon: 'bash.png' },
    { name: 'Docker', dot: '#2496ed', icon: 'docker.png' },
    { name: 'SQL', dot: '#f29111', icon: 'sql.png' },
    { name: 'C++', dot: '#00599c', icon: 'c++.png' },
    { name: 'Markdown', dot: '#c4c4c4', icon: 'markdown.png' },
  ];

  return (
    <section className="core-platform">
      <div className="core-header">
        <div className="label-wrapper">
          <span className="section-label">CORE PLATFORM</span>
        </div>
        <h2 className="core-title">
          A full code editor and execution layer for every language, every stack.
        </h2>
      </div>

      <div className="core-grid">
        <div className="core-card tech-card">
          <div className="tech-card-bg"></div>
          <div className="tech-grid">
            <div className="tech-row">
              {techIcons.slice(0, 3).map((tech, index) => (
                <div key={index} className="tech-box">
                  <div className="tech-icon-placeholder">
                    <div 
                      className="tech-square" 
                      style={{ background: `linear-gradient(-45deg, ${tech.dot} 0%, transparent 100%)` }}
                    ></div>
                    <img src={`/stack/${tech.icon}`} alt={tech.name} className="tech-icon" />
                  </div>
                </div>
              ))}
            </div>
            <div className="tech-row">
              {techIcons.slice(3, 7).map((tech, index) => (
                <div key={index} className="tech-box">
                  <div className="tech-icon-placeholder">
                    <div 
                      className="tech-square" 
                      style={{ background: `linear-gradient(-45deg, ${tech.dot} 0%, transparent 100%)` }}
                    ></div>
                    <img src={`/stack/${tech.icon}`} alt={tech.name} className="tech-icon" />
                  </div>
                </div>
              ))}
            </div>
            <div className="tech-row">
              {techIcons.slice(7, 10).map((tech, index) => (
                <div key={index} className="tech-box">
                  <div className="tech-icon-placeholder">
                    <div 
                      className="tech-square" 
                      style={{ background: `linear-gradient(-45deg, ${tech.dot} 0%, transparent 100%)` }}
                    ></div>
                    <img src={`/stack/${tech.icon}`} alt={tech.name} className="tech-icon" />
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>

        <div className="core-card dashboard-card">
          <div className="dashboard-card-bg"></div>
          <div className="dashboard-placeholder">
            <img src="/media/observability.png" alt="Observability Dashboard" className="observability-img" />
          </div>
        </div>
      </div>
    </section>
  );
};

export default CorePlatform;
