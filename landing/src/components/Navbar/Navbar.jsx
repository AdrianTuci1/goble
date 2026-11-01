import { useState } from 'react';
import './Navbar.css';
import { getActionLinkProps, githubUrl } from '../../config/actionLinks';
import { primaryDownload } from '../../config/install';
import { useDetectOS } from '../../hooks/useDetectOS';

const Navbar = () => {
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const { os, arch, isMobile } = useDetectOS();

  const useCases = [
    {
      title: "AI Agents",
      desc: "Delegate tasks in natural language",
      icon: <img src="/nav/navigation-ai-light.svg" alt="AI Agents" />
    },
    {
      title: "Automations",
      desc: "Run repetitive tasks automatically",
      icon: <img src="/nav/navigation-automations-light.svg" alt="Automations" />
    },
    {
      title: "Sequences",
      desc: "Chain multiple commands together",
      icon: <img src="/nav/navigation-sequences-light-2.svg" alt="Sequences" />
    },
    {
      title: "Apps",
      desc: "Connect with your favorite tools",
      icon: <img src="/nav/navigation-apps-light.svg" alt="Apps" />
    }
  ];

  const getDownloadButtonProps = () => {
    switch (os) {
      case 'mac':
        return {
          href: primaryDownload('mac', arch) || '#download',
          label: 'Download',
          icon: <img src="/mac.png" alt="macOS" className="mac-icon" />
        };
      case 'windows':
        return {
          href: primaryDownload('windows', arch) || '#download',
          label: 'Download',
          icon: (
            <svg className="mac-icon" viewBox="0 0 24 24" fill="currentColor" style={{ width: '13px', height: '13px', display: 'inline-block', verticalAlign: 'middle' }}>
              <path d="M0 3.449L9.75 2.1v9.451H0m11.25-9.658L24 0v11.549H11.25M0 12.451h9.75V21.9L0 20.551m11.25-8.1V24L24 21.899V12.451" />
            </svg>
          )
        };
      case 'linux':
        return {
          href: primaryDownload('linux', arch) || '#download',
          label: 'Download',
          icon: <img src="/os/linux.png" alt="Linux" className="mac-icon" style={{ width: '13px', height: '13px', display: 'inline-block', verticalAlign: 'middle' }} />
        };
      default:
        return {
          href: '#download',
          label: 'Download',
          icon: null
        };
    }
  };

  const dlProps = getDownloadButtonProps();

  return (
    <div className="navbar-wrapper">
      <nav className="navbar">
        <div className="nav-left">
          <a href="/" className="logo">
            <img src="/icon.svg" alt="Octomus Logo" className="logo-icon" />
            octomus
          </a>
        </div>
        <div className="nav-right">
          <a {...getActionLinkProps(githubUrl)} className="btn-github" title="GitHub">
            <svg viewBox="0 0 24 24">
              <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
            </svg>
          </a>
          {!isMobile && (
            <a {...getActionLinkProps(dlProps.href)} className="btn-download">
              {dlProps.label}
              {dlProps.icon}
            </a>
          )}
        </div>
      </nav>
    </div>
  );
};

export default Navbar;

