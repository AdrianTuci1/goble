import React from 'react';
import './Cards.css';

const ChatCard = () => {
  return (
    <div className="card-container">
      <div className="chat-card">
        <div className="badge-top">
          <div className="chat-header-icons">
            <div className="header-icon">
              <img src="/llms/claude.png" alt="Claude" className="chat-llm-logo" />
            </div>
            <div className="header-icon">
              <img src="/llms/openai.png" alt="OpenAI" className="chat-llm-logo" />
            </div>
            <div className="header-icon">
              <img src="/llms/image.png" alt="Gemini" className="chat-llm-logo" />
            </div>
          </div>
        </div>

        <div className="chat-greeting">
          <span className="greet-small">Hey Alex</span>
          <span className="greet-large">How can I help you?</span>
        </div>

        <div className="chat-input-area">
          <div className="chat-msg">
            Why is our GPU service slower after the last deploy?
          </div>
          <div className="chat-controls">
            <div className="chat-left-controls">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
              </svg>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <circle cx="12" cy="12" r="10" /><line x1="2" y1="12" x2="22" y2="12" /><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
              </svg>
              <span>Search</span>
            </div>
            <div style={{display: 'flex', gap: '10px'}}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" y1="19" x2="12" y2="23"/><line x1="8" y1="23" x2="16" y2="23"/>
              </svg>
              <div className="chat-send-btn">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                  <line x1="12" y1="19" x2="12" y2="5" /><polyline points="5 12 12 5 19 12" />
                </svg>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default ChatCard;
