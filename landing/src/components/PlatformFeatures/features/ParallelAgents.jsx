import React from 'react';

const ParallelAgents = () => {
  return (
    <div className="feature-visual parallel-agents-visual">
      <div className="chip-stack">
        <div className="chip pink">
          <div className="chip-icon">◌</div>
          <div className="chip-label">Pod: <span className="chip-val">api-3</span></div>
        </div>
        <div className="chip purple">
          <div className="chip-icon">☁</div>
          <div className="chip-label">Env: <span className="chip-val">production</span></div>
        </div>
        <div className="chip blue">
          <div className="chip-icon">▥</div>
          <div className="chip-label">Service: <span className="chip-val">payments</span></div>
        </div>
        <div className="chip teal">
          <div className="chip-icon">◉</div>
          <div className="chip-label">Cluster: <span className="chip-val">eu-west</span></div>
        </div>
        <div className="chip orange">
          <div className="chip-icon">◔</div>
          <div className="chip-label">Version: <span className="chip-val">v2.1</span></div>
        </div>
      </div>
    </div>
  );
};

export default ParallelAgents;
