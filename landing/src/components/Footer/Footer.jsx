import './Footer.css';

const Footer = () => {
  return (
    <footer className="footer">
      <div className="footer-container">
        <div className="footer-brand">
          <div className="footer-logo">
            <img src="/icon.svg" alt="Octomus Logo" className="footer-logo-icon" />
            <span>octomus</span>
          </div>
          <p className="footer-tagline">Terminal-native coding with intelligent agents</p>
        </div>

        <a href="https://staticlabs.ro" target="_blank" rel="noopener noreferrer" className="footer-attribution">
          <span className="attribution-prefix">brought to you by</span>
          <div className="static-logo-wrapper">
            <img src="/staticlabs.png" alt="StaticLabs" className="footer-static-logo" />
            <span className="footer-static-text">staticlabs</span>
          </div>
        </a>
      </div>
    </footer>
  );
};

export default Footer;


