import { useState } from 'react';
import './PlatformFeatures.css';
import './features/Features.css';
import SpotlightLauncher from './features/SpotlightLauncher';

import AgentMode from './features/AgentMode';
import CodeEditor from './features/CodeEditor';
import ParallelAgents from './features/ParallelAgents';

const PlatformFeatures = () => {
  const [activeIndex, setActiveIndex] = useState(1);

  const features = [
    {
      title: "Spotlight-style launcher.",
      description: "Search commands, files, and workflows from a fast command surface built for keyboard-first speed.",
      color: "slate",
      component: SpotlightLauncher
    },
    {
      title: "Smart autocomplete.",
      description: "Complete commands intelligently while staying in normal terminal mode and keeping full control.",
      color: "emerald",
      image: "/media/composer.png"
    },
    {
      title: "Native agent harness.",
      description: "A transparent execution layer that shows step-by-step reasoning as Octomus analyzes, plans, and executes complex tasks.",
      color: "cyan",
      component: AgentMode
    },
    {
      title: "Seamless VPS connectivity.",
      description: "Connect to any remote server via SSH and manage your files directly from Octomus.",
      color: "purple",
      component: CodeEditor
    },
    {
      title: "Parallel local and cloud agents.",
      description: "Launch multiple agents across your machine and the cloud to split bigger jobs into parallel workstreams.",
      color: "green",
      component: ParallelAgents
    }
  ];

  const nextSlide = () => {
    setActiveIndex((prev) => (prev + 1) % features.length);
  };

  const prevSlide = () => {
    setActiveIndex((prev) => (prev - 1 + features.length) % features.length);
  };

  return (
    <section className="features-carousel-section">
      <div className="carousel-header">
        <div className="header-left">
          <div className="label-wrapper">
            <span className="section-label">PLATFORM FEATURES</span>
          </div>
          <h2 className="carousel-title">
            Your workflow, accelerated by Octomus. <span className="dimmed">A terminal launcher, editor, and agent harness in one place.</span>
          </h2>
        </div>
        <div className="carousel-nav">
          <button className="nav-btn" onClick={prevSlide}>
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M19 12H5M5 12L12 19M5 12L12 5" />
            </svg>
          </button>
          <button className="nav-btn" onClick={nextSlide}>
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M5 12H19M19 12L12 5M19 12L12 19" />
            </svg>
          </button>
        </div>
      </div>

      <div className="carousel-track-wrapper">
        <div 
          className="carousel-track" 
          style={{ 
            transform: `translateX(calc(-${activeIndex} * (var(--feature-card-width) + var(--feature-card-gap))))` 
          }}
        >
          {features.map((feature, index) => (
            <div key={index} className={`feature-card ${feature.color}`}>
              <div className="feature-card-header">
                <h3 className="feature-card-title">{feature.title}</h3>
                <p className="feature-card-desc">{feature.description}</p>
              </div>
              <div className="feature-media-placeholder">
                {feature.component ? (
                  <feature.component />
                ) : (
                  <img src={feature.image} alt={feature.title} className="feature-image" />
                )}
              </div>
            </div>
          ))}

        </div>
      </div>
    </section>
  );
};

export default PlatformFeatures;

