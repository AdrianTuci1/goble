import { config } from 'dotenv';
import { createServer } from 'http';
import { readFile } from 'fs/promises';
import { join, resolve, extname, dirname } from 'path';
import { fileURLToPath } from 'url';
import { existsSync } from 'fs';
import { Readable } from 'stream';
import { handlers, runReplays } from './handlers.js';

config({ path: resolve(dirname(fileURLToPath(import.meta.url)), '../../.env') });

const distDir = resolve(process.env.GOBLE_E2E_DIST || join(dirname(fileURLToPath(import.meta.url)), '../../dist'));
const port = Number(process.env.GOBLE_E2E_PORT || '1450');

const bridgeScript = `
<script>
  window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || { metadata: { currentWindow: { label: 'main' } } };
  window.__goble_e2e_setup__ = true;
  window.__goble_e2e_invoke__ = async function (cmd, args) {
    const res = await fetch('/__goble_invoke__', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ cmd, args: args ?? {} }),
    });
    if (!res.ok) throw new Error('invoke failed: ' + res.status);
    return res.json().catch(() => undefined);
  };
  window.__goble_e2e_listeners__ = {};
  window.__goble_e2e_emit__ = function (event, payload) {
    const listeners = window.__goble_e2e_listeners__[event] || [];
    listeners.forEach((cb) => cb(payload));
  };
  const es = new EventSource('/__goble_events__');
  es.onmessage = function (ev) {
    try {
      const data = JSON.parse(ev.data);
      window.__goble_e2e_emit__('harness:event', data);
    } catch (e) {
      console.error('SSE parse error', e);
    }
  };
</script>
`;

function contentTypeFor(ext) {
  const map = {
    '.html': 'text/html',
    '.js': 'text/javascript',
    '.mjs': 'text/javascript',
    '.css': 'text/css',
    '.json': 'application/json',
    '.svg': 'image/svg+xml',
    '.png': 'image/png',
    '.ico': 'image/x-icon',
  };
  return map[ext] || 'application/octet-stream';
}

async function serveIndexHtml(res, indexPath) {
  let html = await readFile(indexPath, 'utf-8');
  html = html.replace('</head>', `${bridgeScript}</head>`);
  res.writeHead(200, { 'Content-Type': 'text/html' });
  res.end(html);
}

const clients = new Map();

const server = createServer(async (req, res) => {
  const url = new URL(req.url || '/', `http://${req.headers.host}`);

  if (url.pathname === '/health') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ ok: true }));
    return;
  }

  if (url.pathname === '/__goble_invoke__') {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', async () => {
      try {
        const { cmd, args } = JSON.parse(body || '{}');
        const result = await handlers.invoke(cmd, args);
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify(result ?? null));
      } catch (e) {
        console.error('[e2e server] invoke error', e);
        res.writeHead(500, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: e.message }));
      }
    });
    return;
  }

  if (url.pathname === '/__goble_events__') {
    res.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      'Connection': 'keep-alive',
    });
    const id = crypto.randomUUID();
    clients.set(id, res);
    res.write(`:ok\n\n`);
    const cleanup = handlers.on('harness:event', (payload) => {
      res.write(`data: ${JSON.stringify(payload)}\n\n`);
    });
    req.on('close', () => {
      cleanup();
      clients.delete(id);
    });
    return;
  }

  if (url.pathname === '/') {
    await serveIndexHtml(res, join(distDir, 'index.html'));
    return;
  }

  let filePath = join(distDir, url.pathname);
  if (!existsSync(filePath)) {
    filePath = join(distDir, 'index.html');
  }

  if (filePath.endsWith('index.html')) {
    await serveIndexHtml(res, filePath);
    return;
  }

  try {
    const data = await readFile(filePath);
    res.writeHead(200, { 'Content-Type': contentTypeFor(extname(filePath)) });
    res.end(data);
  } catch (e) {
    res.writeHead(404);
    res.end('not found');
  }
});

server.listen(port, '0.0.0.0', () => {
  console.log(`[goble e2e server] listening on http://0.0.0.0:${port} serving ${distDir}`);
});

// Forward harness events to SSE clients (already done in handler subscription)
