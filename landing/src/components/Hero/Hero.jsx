import './Hero.css';
import HeroVisual from './HeroVisual/HeroVisual';
import { getActionLinkProps, githubUrl } from '../../config/actionLinks';
import { primaryDownload } from '../../config/install';
import { useDetectOS } from '../../hooks/useDetectOS';

const Hero = () => {
  const { os, arch, isMobile } = useDetectOS();

  const getHeroButtonProps = () => {
    if (isMobile) {
      return {
        href: '#download',
        label: 'View Versions',
        icon: (
          <svg className="mac-icon-hero" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" style={{ width: '14px', height: '14px' }}>
            <line x1="12" y1="5" x2="12" y2="19"></line>
            <polyline points="19 12 12 19 5 12"></polyline>
          </svg>
        )
      };
    }

    switch (os) {
      case 'mac':
        return {
          href: primaryDownload('mac', arch) || '#download',
          label: 'Download for Mac',
          icon: <img src="/mac.png" alt="macOS" className="mac-icon-hero" />
        };
      case 'windows':
        return {
          href: primaryDownload('windows', arch) || '#download',
          label: 'Download for Windows',
          icon: (
            <svg className="mac-icon-hero" viewBox="0 0 24 24" fill="currentColor" style={{ width: '14px', height: '14px' }}>
              <path d="M0 3.449L9.75 2.1v9.451H0m11.25-9.658L24 0v11.549H11.25M0 12.451h9.75V21.9L0 20.551m11.25-8.1V24L24 21.899V12.451" />
            </svg>
          )
        };
      case 'linux':
        return {
          href: primaryDownload('linux', arch) || '#download',
          label: 'Download for Linux',
          icon: <img src="/os/linux.png" alt="Linux" className="mac-icon-hero" style={{ width: '14px', height: '14px' }} />
        };
      default:
        return {
          href: '#download',
          label: 'Download',
          icon: null
        };
    }
  };

  const heroProps = getHeroButtonProps();

  return (
    <section className="hero-section">
      <div className="hero-content">
        <div className="hero-text-content">
          {/* <div className="badge">
            <span className="badge-new">New</span>
            Spotlight launcher, smart autocomplete, and agent mode • Learn more →
          </div> */}

          <h1 className="hero-title">
            One workspace for commands,<br />agents, and coding
          </h1>

          <p className="hero-subtitle">
            Octomus combines a terminal-native launcher, intelligent autocomplete, a full code editor, and one of the most advanced agent harnesses in the world.
          </p>
        </div>

        <div className="hero-actions">
          <a {...getActionLinkProps(githubUrl)} className="btn-hero-github">
            <svg viewBox="0 0 24 24">
              <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
            </svg>
            GitHub
          </a>
          <a {...getActionLinkProps(heroProps.href)} className="btn-hero-download">
            <span className="btn-hero-download-text">{heroProps.label}</span>
            {heroProps.icon}
          </a>
        </div>
      </div>

      <HeroVisual phase="hud" />
    </section>
  );
};

export default Hero;

