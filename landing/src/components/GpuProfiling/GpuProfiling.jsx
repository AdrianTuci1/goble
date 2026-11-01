import './GpuProfiling.css';
import GpuBackgroundBox from './GpuBackgroundBox';


const GpuProfiling = () => {
  return (
    <section className="gpu-section">
      <GpuBackgroundBox />
      <div className="gpu-container">
        <div className="gpu-top-label">
          SPOTLIGHT LAUNCHER
        </div>

        <div className="gpu-content-top">
          <h2 className="gpu-title">
            A terminal launcher built for speed, context, and control.
          </h2>
          <div className="gpu-description-box">
            <p className="gpu-text">
              Octomus gives you Spotlight-style search, intelligent command suggestions, and an easy switch into agent mode when you want the task handled for you.
            </p>
            <div className="gpu-logos">
              <div className="gpu-logo-item">
                <span className="gpu-square commands"></span>
                <span>Commands</span>
              </div>
              <div className="gpu-logo-item">
                <span className="gpu-square autocomplete"></span>
                <span>Autocomplete</span>
              </div>
              <div className="gpu-logo-item">
                <span className="gpu-square agents"></span>
                <span>Agents</span>
              </div>
            </div>
          </div>
        </div>

        <div className="gpu-dashboard-placeholder">
          <img
            src="/misc/second.png"
            alt="Spotlight Launcher"
            className="gpu-dashboard-img"
          />
        </div>
      </div>
    </section>

  );
};

export default GpuProfiling;
