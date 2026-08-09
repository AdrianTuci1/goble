#!/usr/bin/env node
// MCP mock server used by the worker runtime isolation end-to-end test.
// It verifies that the worker passed the AI API key through as an environment
// variable and that each agent gets its own isolated workspace directory.

const fs = require('fs');
const path = require('path');
const readline = require('readline');

const apiKey = process.env.AI_API_KEY;
const workspace = process.env.GOBLIN_AGENT_WORKSPACE;

if (!workspace) {
  console.error('GOBLIN_AGENT_WORKSPACE is not set');
  process.exit(1);
}

fs.writeFileSync(path.join(workspace, 'runtime-mock-init.txt'), `init-key=${apiKey || 'missing'}\n`);

const tools = [
  {
    name: 'verify_env',
    description: 'Verifies the secret env vars and workspace isolation',
    inputSchema: {
      type: 'object',
      properties: {
        agent_tag: { type: 'string' },
      },
      required: ['agent_tag'],
    },
  },
];

const rl = readline.createInterface({ input: process.stdin, output: process.stdout, terminal: false });

rl.on('line', (line) => {
  const req = JSON.parse(line);
  let result = null;

  if (req.method === 'initialize') {
    result = { protocolVersion: '2024-11-05', capabilities: {}, serverInfo: { name: 'runtime-mock', version: '1.0' } };
  } else if (req.method === 'tools/list') {
    result = { tools };
  } else if (req.method === 'tools/call') {
    const { name, arguments: args } = req.params;
    if (name === 'verify_env') {
      const marker = path.join(workspace || '.', `marker-${args.agent_tag}.txt`);
      fs.writeFileSync(marker, `key=${apiKey || 'missing'}\nworkspace=${workspace || 'missing'}\n`);
      result = {
        content: [{
          type: 'text',
          text: `verified agent ${args.agent_tag} with key ${apiKey ? 'present' : 'missing'} in workspace ${workspace || 'missing'}`,
        }],
      };
    } else {
      result = { content: [{ type: 'text', text: 'unknown tool' }] };
    }
  }

  const resp = { jsonrpc: '2.0', id: req.id, result };
  process.stdout.write(JSON.stringify(resp) + '\n');
});
