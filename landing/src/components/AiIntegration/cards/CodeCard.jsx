import React from 'react';
import './Cards.css';

const CodeCard = () => {
  return (
    <div className="card-container">
      <div className="code-card">
        <div className="badge-top">
          <div className="code-badge">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#4285f4" strokeWidth="2">
              <path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z" fill="#4285f4" />
            </svg>
          </div>
        </div>
        
        <div className="code-content">
          <div className="code-lines-wrapper">
            <div className="code-line">
              <span className="line-num">1</span>
              <span className="line-text"><span className="keyword">function</span> <span className="function">processNumbers</span><span className="bracket">(</span>nums<span className="bracket">) {'{'}</span></span>
            </div>
            <div className="code-line">
              <span className="line-num">2</span>
              <span className="line-text">  <span className="keyword">const</span> results <span className="keyword">=</span> <span className="bracket">[]</span></span>
            </div>
            <div className="code-line line-highlight">
              <span className="line-num">3</span>
              <span className="line-text">  <span className="keyword">for</span> <span className="bracket">(</span><span className="keyword">const</span> n <span className="keyword">of</span> nums<span className="bracket">) {'{'}</span></span>
            </div>
            <div className="code-line">
              <span className="line-num">4</span>
              <span className="line-text">    <span className="keyword">if</span> <span className="bracket">(</span>n <span className="keyword">*</span> <span className="string">2</span> <span className="keyword">&gt;</span> <span className="string">10</span><span className="bracket">)</span> results.<span className="function">push</span><span className="bracket">(</span>n <span className="keyword">*</span> <span className="string">2</span><span className="bracket">)</span></span>
            </div>
            <div className="code-line">
              <span className="line-num">5</span>
              <span className="line-text">  <span className="bracket">{'}'}</span></span>
            </div>
            <div className="code-line line-highlight">
              <span className="line-num">6</span>
              <span className="line-text">  results.<span className="function">sort</span><span className="bracket">((</span>a, b<span className="bracket">)</span> <span className="keyword">=&gt;</span> b <span className="keyword">-</span> a<span className="bracket">)</span></span>
            </div>
            <div className="code-line">
              <span className="line-num">7</span>
              <span className="line-text">  <span className="keyword">return</span> results.<span className="function">slice</span><span className="bracket">(</span><span className="string">0</span>, <span className="string">5</span><span className="bracket">)</span></span>
            </div>
            <div className="code-line">
              <span className="line-num">8</span>
              <span className="line-text"><span className="bracket">{'}'}</span></span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default CodeCard;
