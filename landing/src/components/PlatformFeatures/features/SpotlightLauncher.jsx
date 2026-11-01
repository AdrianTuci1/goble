import React from 'react';

const SpotlightLauncher = () => {
  return (
    <div className="feature-visual spotlight-visual">
      <div className="search-bar">
        <svg className="search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="11" cy="11" r="8" />
          <path d="M21 21l-4.35-4.35" />
        </svg>
        <span className="search-text">filter by function: <span className="highlight">symbolizer</span></span>
        <span className="search-close">×</span>
      </div>
      <div className="code-lines">
        <div className="code-line">github.com/polarsignals/pkg/fast<span className="code-highlight">symbolizer</span>.writ</div>
        <div className="code-line">fast<span className="code-highlight">symbolizer</span>.retrieveSymbols</div>
        <div className="code-line">fast<span className="code-highlight">symbolizer</span>.writeBatch</div>
        <div className="code-line">fast<span className="code-highlight">symbolizer</span>.Symbolize</div>
      </div>
    </div>
  );
};

export default SpotlightLauncher;
