import { useState, useEffect } from 'react';
import './CTASection.css';
import { downloadLinks } from '../../config/actionLinks';
import { installCommands, primaryDownload } from '../../config/install';
import { useDetectOS } from '../../hooks/useDetectOS';

const CTASection = () => {
  const { os, arch, isMobile } = useDetectOS();
  const [toast, setToast] = useState({ show: false, message: '' });

  // The Mac column leads with the image for this machine and keeps both
  // architectures one click away.
  const macArm64 = downloadLinks.mac.arm64 || downloadLinks.mac.universal;
  const macX64 = downloadLinks.mac.x64 || downloadLinks.mac.universal;
  const macPrimary = primaryDownload('mac', arch);
  const macPrimaryLabel = arch === 'x64' ? 'Intel' : 'Apple Silicon';

  const showToast = (message) => {
    setToast({ show: true, message });
  };

  useEffect(() => {
    if (toast.show) {
      const timer = setTimeout(() => setToast({ show: false, message: '' }), 3000);
      return () => clearTimeout(timer);
    }
  }, [toast.show]);

  const handleDownloadClick = (e, link, label) => {
    if (isMobile) {
      e.preventDefault();
      showToast('Downloads are only available on desktop platforms.');
      return;
    }

    if (!link) {
      e.preventDefault();
      showToast(`${label} download is coming soon!`);
      return;
    }
  };

  const handleCopy = (text, e) => {
    e.stopPropagation();
    if (isMobile) {
      showToast('Terminal installation is only available on desktop.');
      return;
    }
    const cleanText = text.replace(/^\$\s*/, '');
    navigator.clipboard.writeText(cleanText).then(() => {
      showToast('Command copied to clipboard!');
    }).catch(() => {
      showToast('Failed to copy command');
    });
  };

  return (
    <section className="download-section" id="download">
      <div className="download-container">
        <div className="download-header">
          <span className="download-label">ALL DOWNLOADS</span>
          <h2 className="download-title">Get Octomus today</h2>
          <p className="download-subtitle">
            Uncompromising power, zero cost. Octomus is free to download and built to stay that way forever.
          </p>
        </div>

        <div className="download-grid">
          {/* Mac Column */}
          <div className={`download-col${os === 'mac' ? ' is-current' : ''}`}>
            <div className="os-header">
              <svg className="os-icon" viewBox="0 0 24 24" fill="currentColor">
                <path d="M17.05 20.28c-.98.95-2.05.8-3.08.35-1.09-.46-2.09-.48-3.24 0-1.44.62-2.2.44-3.06-.35C2.79 15.25 3.51 7.59 9.05 7.31c1.35.07 2.29.74 3.08.8 1.18-.24 2.31-.93 3.57-.84 1.51.12 2.65.72 3.4 1.8-3.12 1.87-2.39 5.98.67 7.28-.62 1.57-1.42 3.12-2.72 4.93zm-4.41-13.43c-.04-3.56 2.96-6.55 6.46-6.85.34 3.73-3.11 6.84-6.46 6.85z" />
              </svg>
              <h3 className="os-name">Mac</h3>
              {os === 'mac' && <span className="os-current">This device</span>}
            </div>
            <a
              href={(!isMobile && macPrimary) ? macPrimary : '#'}
              {...((!isMobile && macPrimary) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
              className={`download-main-card ${isMobile ? 'disabled' : ''}`}
              onClick={(e) => handleDownloadClick(e, macPrimary, `macOS ${macPrimaryLabel}`)}
            >
              <span className="file-type">macOS</span>
              <span className="version-info">{macPrimaryLabel}</span>
            </a>
            <div className="other-formats">
              <div className="format-row">
                <span>.dmg</span>
                <div className="arch-btns">
                  <a
                    href={(!isMobile && macArm64) ? macArm64 : '#'}
                    {...((!isMobile && macArm64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}${arch !== 'x64' ? ' current' : ''}`}
                    onClick={(e) => handleDownloadClick(e, macArm64, 'macOS Apple Silicon')}
                  >
                    Apple Silicon
                  </a>
                  <a
                    href={(!isMobile && macX64) ? macX64 : '#'}
                    {...((!isMobile && macX64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}${arch === 'x64' ? ' current' : ''}`}
                    onClick={(e) => handleDownloadClick(e, macX64, 'macOS Intel')}
                  >
                    Intel
                  </a>
                </div>
              </div>
            </div>
            <div 
              className={`terminal-command ${isMobile ? 'disabled' : ''}`}
              onClick={(e) => handleCopy(installCommands.mac, e)}
              title="Copy to clipboard"
            >
              <span className="command-text">$ {installCommands.mac}</span>
              <svg className="copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
              </svg>
            </div>
          </div>

          {/* Linux Column */}
          <div className={`download-col${os === 'linux' ? ' is-current' : ''}`}>
            <div className="os-header">
              <img src="/os/linux.png" alt="Linux" className="os-icon-img" />
              <h3 className="os-name">Linux</h3>
              {os === 'linux' && <span className="os-current">This device</span>}
            </div>
            <div className="multi-download-cards">
              <a
                href={(!isMobile && primaryDownload('linux', arch)) ? primaryDownload('linux', arch) : '#'}
                {...((!isMobile && primaryDownload('linux', arch)) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                className={`download-main-card half ${isMobile ? 'disabled' : ''}`}
                onClick={(e) => handleDownloadClick(e, primaryDownload('linux', arch), 'Linux Debian .deb')}
              >
                <span className="file-type">.deb</span>
                <span className="version-info">Debian, Ubuntu · {arch === 'arm64' ? 'ARM64' : 'x64'}</span>
              </a>
            </div>
            <div className="other-formats">
              <div className="format-row">
                <span>.deb</span>
                <div className="arch-btns">
                  <a
                    href={(!isMobile && downloadLinks.linux.debX64) ? downloadLinks.linux.debX64 : '#'}
                    {...((!isMobile && downloadLinks.linux.debX64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}${arch !== 'arm64' ? ' current' : ''}`}
                    onClick={(e) => handleDownloadClick(e, downloadLinks.linux.debX64, 'Linux Debian x64')}
                  >
                    x64
                  </a>
                  <a
                    href={(!isMobile && downloadLinks.linux.debArm64) ? downloadLinks.linux.debArm64 : '#'}
                    {...((!isMobile && downloadLinks.linux.debArm64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}${arch === 'arm64' ? ' current' : ''}`}
                    onClick={(e) => handleDownloadClick(e, downloadLinks.linux.debArm64, 'Linux Debian ARM64')}
                  >
                    ARM64
                  </a>
                </div>
              </div>
              <div className="format-row">
                <span>.tar.gz <small>Any distribution</small></span>
                <div className="arch-btns">
                  <a
                    href={(!isMobile && downloadLinks.linux.tarX64) ? downloadLinks.linux.tarX64 : '#'}
                    {...((!isMobile && downloadLinks.linux.tarX64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}${arch !== 'arm64' ? ' current' : ''}`}
                    onClick={(e) => handleDownloadClick(e, downloadLinks.linux.tarX64, 'Linux Tar x64')}
                  >
                    x64
                  </a>
                  <a
                    href={(!isMobile && downloadLinks.linux.tarArm64) ? downloadLinks.linux.tarArm64 : '#'}
                    {...((!isMobile && downloadLinks.linux.tarArm64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}${arch === 'arm64' ? ' current' : ''}`}
                    onClick={(e) => handleDownloadClick(e, downloadLinks.linux.tarArm64, 'Linux Tar ARM64')}
                  >
                    ARM64
                  </a>
                </div>
              </div>
              <div className="format-row">
                <span>AppImage</span>
                <div className="arch-btns">
                  <a
                    href={(!isMobile && downloadLinks.linux.appImageX64) ? downloadLinks.linux.appImageX64 : '#'}
                    {...((!isMobile && downloadLinks.linux.appImageX64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}${arch !== 'arm64' ? ' current' : ''}`}
                    onClick={(e) => handleDownloadClick(e, downloadLinks.linux.appImageX64, 'Linux AppImage x64')}
                  >
                    x64
                  </a>
                  <a
                    href={(!isMobile && downloadLinks.linux.appImageArm64) ? downloadLinks.linux.appImageArm64 : '#'}
                    {...((!isMobile && downloadLinks.linux.appImageArm64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}${arch === 'arm64' ? ' current' : ''}`}
                    onClick={(e) => handleDownloadClick(e, downloadLinks.linux.appImageArm64, 'Linux AppImage ARM64')}
                  >
                    ARM64
                  </a>
                </div>
              </div>
            </div>
            <div
              className={`terminal-command ${isMobile ? 'disabled' : ''}`}
              onClick={(e) => handleCopy(installCommands.linux, e)}
              title="Copy to clipboard"
            >
              <span className="command-text">$ {installCommands.linux}</span>
              <svg className="copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
              </svg>
            </div>
          </div>

          {/* Windows Column */}
          <div className={`download-col${os === 'windows' ? ' is-current' : ''}`}>
            <div className="os-header">
              <svg className="os-icon" viewBox="0 0 24 24" fill="currentColor">
                <path d="M0 3.449L9.75 2.1v9.451H0m11.25-9.658L24 0v11.549H11.25M0 12.451h9.75V21.9L0 20.551m11.25-8.1V24L24 21.899V12.451" />
              </svg>
              <h3 className="os-name">Windows</h3>
              {os === 'windows' && <span className="os-current">This device</span>}
            </div>
            <div className="multi-download-cards">
              <a
                href={(!isMobile && downloadLinks.windows.x64) ? downloadLinks.windows.x64 : '#'}
                {...((!isMobile && downloadLinks.windows.x64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                className={`download-main-card half ${isMobile ? 'disabled' : ''}`}
                onClick={(e) => handleDownloadClick(e, downloadLinks.windows.x64, 'Windows x64')}
              >
                <span className="file-type">.exe</span>
                <span className="version-info">Windows x64</span>
              </a>
              <a
                href={(!isMobile && downloadLinks.windows.arm64) ? downloadLinks.windows.arm64 : '#'}
                {...((!isMobile && downloadLinks.windows.arm64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                className={`download-main-card half ${isMobile ? 'disabled' : ''}`}
                onClick={(e) => handleDownloadClick(e, downloadLinks.windows.arm64, 'Windows ARM64')}
              >
                <span className="file-type">.exe</span>
                <span className="version-info">Windows ARM64</span>
              </a>
            </div>
            <div 
              className={`terminal-command ${isMobile ? 'disabled' : ''}`}
              onClick={(e) => handleCopy(installCommands.windows, e)}
              title="Copy to clipboard"
            >
              <span className="command-text">&gt; {installCommands.windows}</span>
              <svg className="copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
              </svg>
            </div>
            <div className="other-formats">
              <div className="format-row">
                <span>.exe</span>
                <div className="arch-btns">
                  <a
                    href={(!isMobile && downloadLinks.windows.x64) ? downloadLinks.windows.x64 : '#'}
                    {...((!isMobile && downloadLinks.windows.x64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}`}
                    onClick={(e) => handleDownloadClick(e, downloadLinks.windows.x64, 'Windows x64')}
                  >
                    x64
                  </a>
                  <a
                    href={(!isMobile && downloadLinks.windows.arm64) ? downloadLinks.windows.arm64 : '#'}
                    {...((!isMobile && downloadLinks.windows.arm64) ? { target: '_blank', rel: 'noopener noreferrer' } : {})}
                    className={`arch-btn ${isMobile ? 'disabled' : ''}`}
                    onClick={(e) => handleDownloadClick(e, downloadLinks.windows.arm64, 'Windows ARM64')}
                  >
                    ARM64
                  </a>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      {toast.show && (
        <div className="download-toast">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="10"></circle>
            <line x1="12" y1="16" x2="12" y2="12"></line>
            <line x1="12" y1="8" x2="12.01" y2="8"></line>
          </svg>
          <span>{toast.message}</span>
        </div>
      )}
    </section>
  );
};

export default CTASection;

