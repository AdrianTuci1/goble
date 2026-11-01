import React from 'react';
import './Cards.css';

const PerformanceCard = () => {
  return (
    <div className="card-container">
      <div className="performance-card">
        <div className="badge-top">
          <div className="mcp-status">
            <div className="status-dot"></div>
            MCP Connected
          </div>
        </div>
        
        <div className="query-box">
          <span>Why is the recommendation service slow?</span>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M9 14L4 9l5-5" />
            <path d="M4 9h10a4 4 0 0 1 4 4v4" />
          </svg>
        </div>

        <div className="analysis-status">Analyzing profiling data...</div>

        <div className="analysis-steps">
          <div className="step done">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
              <polyline points="20 6 9 17 4 12" />
            </svg>
            Queried CPU profiles (last 10 min)
          </div>
          <div className="step done">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
              <polyline points="20 6 9 17 4 12" />
            </svg>
            Compared latest deployment
          </div>
          <div className="step done">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
              <polyline points="20 6 9 17 4 12" />
            </svg>
            Identified hottest functions
          </div>
        </div>

        <div className="result-box">
          It looks like <span className="code-highlight">vector_search()</span> is using about 46% of CPU time.
        </div>
      </div>
    </div>
  );
};

export default PerformanceCard;
