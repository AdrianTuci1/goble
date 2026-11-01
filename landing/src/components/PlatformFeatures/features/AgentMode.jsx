import React from 'react';
import { Brain, Search, Code2 } from 'lucide-react';

const AgentMode = () => {
  return (
    <div className="feature-visual agent-mode-visual">
      <div className="alert-item">
        <div className="alert-icon blue">
          <Brain size={16} color="white" />
        </div>
        <div className="alert-content">
          <div className="alert-title">Thinking</div>
          <div className="alert-sub">Analyzing repository structure...</div>
        </div>
      </div>
      <div className="alert-item">
        <div className="alert-icon pink">
          <Search size={16} color="white" />
        </div>
        <div className="alert-content">
          <div className="alert-title">Searching</div>
          <div className="alert-sub">Found 12 relevant code patterns</div>
        </div>
      </div>
      <div className="alert-item">
        <div className="alert-icon warning">
          <Code2 size={16} color="white" />
        </div>
        <div className="alert-content">
          <div className="alert-title">Executing</div>
          <div className="alert-sub">Applying refactoring plan</div>
        </div>
      </div>
    </div>
  );
};

export default AgentMode;
