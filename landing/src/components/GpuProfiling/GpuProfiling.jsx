import './GpuProfiling.css';

const TESTING_STEPS = [
  { time: '0:24', label: 'Ensure compatibility across various screen sizes and usability features', state: 'done' },
  { time: '0:36', label: 'Incorporate accessibility guidelines to enhance user experience', state: 'done' },
  { time: '0:24', label: 'Render in light and dark themes', state: 'done' },
  { time: '0:24', label: 'Test for offline mode', state: 'run' },
  { time: '0:30', label: 'Loading screen improvements', state: 'run' },
  { time: '0:45', label: 'User feedback integration', state: 'run' },
  { time: '1:00', label: 'Sync error handling', state: 'run' },
  { time: '1:15', label: 'Data retrieval optimization', state: 'run' },
];

const MODEL_ROWS = [
  { name: 'Fusion', selected: true },
  { name: 'Ultra' },
  { name: 'Normal' },
  { name: 'Lite' },
];

const ALL_MODELS = [
  { name: 'SWE-2', glyph: '◈' },
  { name: 'Fable 5.1', glyph: '✳' },
  { name: 'GPT-6 Astra', glyph: '◉' },
  { name: 'Grok 4.7', glyph: '◎' },
  { name: 'Gemini 3.8 Flash', glyph: '✦' },
];

const GpuProfiling = () => {
  return (
    <section className="teammate-section">
      <div className="teammate-container">
        <h2 className="teammate-title">Your teammate&rsquo;s favorite teammate</h2>

        <div className="teammate-grid">
          <article className="teammate-card">
            <div className="teammate-visual test-visual">
              <div className="test-screenshot">
                <div className="test-window-bar">
                  <span className="dot" />
                  <span className="dot" />
                  <span className="dot" />
                </div>
                <img src="/media/composer.png" alt="Octomus running an end-to-end test" />
                <span className="test-cursor" />
              </div>
              <div className="test-timeline">
                <div className="test-timeline-head">Testing 3/12</div>
                {TESTING_STEPS.map((step) => (
                  <div className="test-step" key={step.time + step.label}>
                    <span className="test-step-time">{step.time}</span>
                    <span className={`test-step-state ${step.state}`}>
                      {step.state === 'done' ? '✓' : '◌'}
                    </span>
                    <span className="test-step-label">{step.label}</span>
                  </div>
                ))}
              </div>
            </div>
            <div className="teammate-caption">
              <span className="teammate-chip">Devin&rsquo;s computer</span>
              <span className="teammate-caption-text">Flag enabled; mobile viewport on enterprise child org</span>
            </div>
            <h3 className="teammate-heading">Tests end to end on its own computer</h3>
            <p className="teammate-text">
              Devin spins up its own Windows, Mac, and Linux virtual machines as
              needed to build, test in its browser, and record itself clicking
              through what it built
            </p>
          </article>

          <article className="teammate-card">
            <div className="teammate-visual model-visual">
              <div className="model-picker">
                <div className="model-search">
                  <span className="model-search-icon">⌕</span>
                  <span>Search all models</span>
                </div>
                <div className="model-list">
                  {MODEL_ROWS.map((row) => (
                    <div className={`model-row ${row.selected ? 'selected' : ''}`} key={row.name}>
                      {row.name}
                    </div>
                  ))}
                  <div className="model-divider" />
                  <div className="model-section-label">All models</div>
                  {ALL_MODELS.map((model) => (
                    <div className="model-row" key={model.name}>
                      <span className="model-glyph">{model.glyph}</span>
                      {model.name}
                    </div>
                  ))}
                </div>
              </div>

              <div className="model-detail">
                <div className="model-detail-title">Fusion</div>
                <div className="model-detail-desc">
                  Pairs frontier intelligence with cost-efficient execution
                </div>
                <div className="model-detail-row">
                  <span>Lead</span>
                  <span className="model-detail-value">Claude Fable 5.1 ⌄</span>
                </div>
                <div className="model-detail-row">
                  <span>Effort</span>
                  <span className="model-detail-value">High ⌄</span>
                </div>
                <div className="model-detail-row">
                  <span>Sidekick</span>
                  <span className="model-detail-value">SWE-2 Medium ⌄</span>
                </div>
                <div className="model-detail-row">
                  <span>Fast mode</span>
                  <span className="model-toggle" />
                </div>
                <div className="model-detail-row model-detail-fusion">
                  <span />
                  <span className="model-detail-value">Fusion</span>
                </div>
                <div className="model-gradient-bar">
                  <span className="model-gradient-knob" />
                </div>
              </div>
            </div>
            <div className="teammate-caption">
              <span className="teammate-caption-text">
                Fusion mixes and matches models to get the most from each
              </span>
            </div>
            <h3 className="teammate-heading">Gets the best from every frontier model</h3>
            <p className="teammate-text">
              Access frontier performance for up to 60% less. Devin mixes and
              matches models to get the most from each
            </p>
          </article>
        </div>
      </div>
    </section>
  );
};

export default GpuProfiling;
