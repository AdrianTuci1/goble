import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App.tsx';
import './index.css';

function mountErrorOverlay() {
  const overlay = document.createElement('div');
  overlay.id = 'error-overlay';
  overlay.style.cssText =
    'position:fixed;bottom:0;left:0;right:0;max-height:40vh;overflow:auto;background:rgba(0,0,0,0.9);color:#f87171;font-family:ui-monospace,monospace;font-size:12px;z-index:9999;padding:8px;white-space:pre-wrap;';
  document.body.appendChild(overlay);

  function log(msg: string) {
    const line = document.createElement('div');
    line.style.borderBottom = '1px solid #333';
    line.style.padding = '4px 0';
    line.textContent = msg;
    overlay.appendChild(line);
  }

  window.addEventListener('error', (event) => {
    log(
      `ERROR: ${event.message}\n  at ${event.filename}:${event.lineno}:${event.colno}\n${event.error?.stack || ''}`,
    );
  });

  window.addEventListener('unhandledrejection', (event) => {
    const reason = event.reason;
    log(
      `UNHANDLED REJECTION: ${reason?.message || reason}\n${reason?.stack || ''}`,
    );
  });
}

mountErrorOverlay();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
