#!/usr/bin/env node
const readline = require('readline');

const tools = [
  {
    name: 'echo',
    description: 'Echoes input back',
    inputSchema: {
      type: 'object',
      properties: { message: { type: 'string' } },
      required: ['message']
    }
  }
];

const rl = readline.createInterface({ input: process.stdin, output: process.stdout, terminal: false });

rl.on('line', (line) => {
  const req = JSON.parse(line);
  let result = null;
  if (req.method === 'initialize') {
    result = { protocolVersion: '2024-11-05', capabilities: {}, serverInfo: { name: 'mock', version: '1.0' } };
  } else if (req.method === 'tools/list') {
    result = { tools };
  } else if (req.method === 'tools/call') {
    const { name, arguments: args } = req.params;
    if (name === 'echo') {
      result = { content: [{ type: 'text', text: `echo: ${args.message}` }] };
    } else {
      result = { content: [{ type: 'text', text: 'unknown tool' }] };
    }
  }
  const resp = { jsonrpc: '2.0', id: req.id, result };
  process.stdout.write(JSON.stringify(resp) + '\n');
});
