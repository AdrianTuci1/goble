import { useState } from 'react';
import './InstantSetup.css';
import { useDetectOS } from '../../hooks/useDetectOS';
import {
  installCommands,
  normalizePlatform,
  platformLabels,
  platformOrder,
  platformPrompts
} from '../../config/install';

const InstantSetup = () => {
  const { os, isMobile } = useDetectOS();
  const [copied, setCopied] = useState(false);
  const [selectedPlatform, setSelectedPlatform] = useState(null);

  // The platform the visitor picked wins; with no pick, use the one the browser
  // reports, and fall back to macOS when it reports nothing usable.
  const platform = selectedPlatform ?? normalizePlatform(isMobile ? 'mobile' : os);
  const command = installCommands[platform];

  const selectPlatform = (next) => {
    setSelectedPlatform(next);
    setCopied(false);
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(command).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <section className="setup-section" id="install">
      <div className="setup-container">
        <div className="label-wrapper">
          <span className="section-label">INSTANT SETUP</span>
        </div>
        
        <h2 className="setup-title">
          Run a single command to{' '}<br />
          launch Octomus and start{' '}<br />
          shipping faster.
        </h2>

        {/* The background starts mid-title */}
        <div className="setup-bg-box">
          <div className="setup-content-bottom">
            <div
              className="setup-platforms"
              role="tablist"
              aria-label="Installation platform"
            >
              {platformOrder.map((id) => (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={platform === id}
                  className={`platform-tab${platform === id ? ' active' : ''}`}
                  onClick={() => selectPlatform(id)}
                >
                  {platformLabels[id]}
                </button>
              ))}
            </div>

            <div className="code-box">
              <code className="code-content">
                <span className="code-prompt">{platformPrompts[platform]}</span>
                {command}
              </code>
              <button className="copy-button" onClick={handleCopy} title="Copy to clipboard">
                {copied ? (
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#10B981" strokeWidth="2.5">
                    <polyline points="20 6 9 17 4 12"></polyline>
                  </svg>
                ) : (
                  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
                    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
                  </svg>
                )}
              </button>
            </div>

            <div className="setup-description">
              A terminal-native workspace that runs on 
              <span className="tag">
                <span>Local machine</span>
              </span>, 
              scales to 
              <span className="tag">
                <span>Cloud agents</span>
              </span>, 
              works with 
              <span className="tag">
                <span>Claude, Codex, Gemini</span>
              </span>, 
              and keeps 
              <span className="tag">
                <span>the full editor</span>
              </span> 
              in the same flow, with smart completions, natural-language agents, and parallel execution built in.
            </div>
          </div>
        </div>
      </div>
    </section>
  );
};

export default InstantSetup;
