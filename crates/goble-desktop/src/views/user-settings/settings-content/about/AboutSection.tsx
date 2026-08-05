import { Heart, Shield, Code, ExternalLink } from 'lucide-react';
import './AboutSection.css';

export default function AboutSection() {
  return (
    <div className="about-section">
      <div className="about-section-header">
        <h2 className="about-section-title">About</h2>
        <p className="about-section-subtitle">
          Goble is an open-source desktop AI agent built for local-first workflows, team clusters, and
          portable device identities.
        </p>
      </div>

      <div className="about-card">
        <div className="about-logo">
          <span className="about-logo-mark">G</span>
        </div>
        <div className="about-app-info">
          <div className="about-app-name">Goble</div>
          <div className="about-app-version">Version 0.1.0</div>
          <div className="about-app-tagline">Open source · Local-first · Multi-device</div>
        </div>
      </div>

      <div className="about-card">
        <h3 className="about-card-title">
          <Heart size={16} /> Open source
        </h3>
        <p className="about-card-text">
          Goble is released under the MIT license. You are free to run, modify, and share it. If you
          don&apos;t set a display name, we generate a random one so you can get started immediately.
        </p>
        <a
          className="about-link"
          href="https://github.com/AdrianTuci1/goble"
          target="_blank"
          rel="noreferrer"
        >
          <ExternalLink size={14} /> GitHub repository
          <ExternalLink size={12} />
        </a>
      </div>

      <div className="about-card">
        <h3 className="about-card-title">
          <Shield size={16} /> Security & identity
        </h3>
        <p className="about-card-text">
          Your device identity is a single PEM key. It can own a Worker Group (cluster) or join other
          groups. The same PEM can be used on multiple devices, and you can share it by scanning the QR
          code in General settings.
        </p>
      </div>

      <div className="about-card">
        <h3 className="about-card-title">
          <Code size={16} /> Built with
        </h3>
        <div className="about-tech-list">
          <span className="about-tech-chip">Tauri</span>
          <span className="about-tech-chip">Rust</span>
          <span className="about-tech-chip">React</span>
          <span className="about-tech-chip">Vite</span>
          <span className="about-tech-chip">Zustand</span>
        </div>
      </div>
    </div>
  );
}
