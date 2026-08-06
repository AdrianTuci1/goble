const express = require('express');
const app = express();
app.use(express.json({ limit: '50mb' }));

const PORT = process.env.PORT || 8080;

app.get('/health', (_req, res) => res.json({ ok: true }));

app.post('/task', (req, res) => {
  const { task_id, task_type, prompt, files } = req.body || {};
  console.log('[worker] task', task_id, task_type, prompt?.slice(0, 200));

  let result = { status: 'done', output: '' };

  if (task_type === 'web_search' || (prompt && prompt.toLowerCase().includes('role'))) {
    result.output = JSON.stringify({
      found: [
        { title: 'Senior Full-Stack Engineer', company: 'TechCorp', url: 'https://example.com/job/1' },
        { title: 'Platform Engineer', company: 'StartupX', url: 'https://example.com/job/2' },
        { title: 'AI Infrastructure Engineer', company: 'MLScale', url: 'https://example.com/job/3' },
      ],
    });
  } else if (task_type === 'build_nodejs' || (prompt && prompt.toLowerCase().includes('server'))) {
    result.files = [
      { path: 'package.json', content: JSON.stringify({ name: 'generated-server', version: '1.0.0', dependencies: { express: '^4.18.0' } }, null, 2) },
      { path: 'src/index.js', content: "const express = require('express');\nconst app = express();\napp.get('/', (req, res) => res.json({ ok: true }));\napp.listen(3000);" },
    ];
    result.output = 'Node.js server generated with Express.';
  } else {
    result.output = `Executed: ${prompt}`;
  }

  res.json({ task_id, ...result });
});

app.listen(PORT, '0.0.0.0', () => console.log(`[worker] listening on ${PORT}`));
