import React from 'react';
import { Server, Terminal, ShieldCheck } from 'lucide-react';

const CodeEditor = () => {
  return (
    <div className="feature-visual code-editor-visual">
      <div className="settings-panel">
        <div className="settings-icon-top">
          <Server size={32} color="#3b82f6" />
        </div>
        <div className="setting-group">
          <label>Remote Host</label>
          <div className="select-box">vps-01.octomus.dev</div>
        </div>
        <div className="setting-group">
          <label>Connection Protocol</label>
          <div className="query-row">
            <div className="select-box">SSH</div>
            <div className="select-box">Port 22</div>
          </div>
        </div>
        <div className="setting-group">
          <label>Authentication</label>
          <div className="select-box">ED25519 Key <ShieldCheck size={14} style={{ marginLeft: 'auto' }} /></div>
        </div>
      </div>
    </div>
  );
};

export default CodeEditor;
