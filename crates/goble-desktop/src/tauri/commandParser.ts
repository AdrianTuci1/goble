export interface ParsedCommand {
  name: string;
  args: {
    name?: string;
    prompt?: string;
    description?: string;
    tools?: string[];
    [key: string]: unknown;
  };
}

export function parseCommand(prompt: string): ParsedCommand | undefined {
  const trimmed = prompt.trim();
  if (!trimmed.startsWith('/')) return undefined;
  const parts = trimmed.split(/\s+/);
  if (parts.length === 0) return undefined;
  const name = parts[0].slice(1);
  const rest = parts.slice(1).join(' ');

  switch (name) {
    case 'create_agent':
    case 'update_agent': {
      const spaceIdx = rest.indexOf(' ');
      const agentName = spaceIdx > 0 ? rest.slice(0, spaceIdx) : rest;
      const agentPrompt = spaceIdx > 0 ? rest.slice(spaceIdx + 1) : '';
      return {
        name,
        args: {
          name: agentName,
          prompt: agentPrompt,
          description: '',
          tools: [],
        },
      };
    }
    case 'create_workflow':
    case 'update_workflow': {
      const spaceIdx = rest.indexOf(' ');
      const wfName = spaceIdx > 0 ? rest.slice(0, spaceIdx) : rest;
      const trigger = spaceIdx > 0 ? rest.slice(spaceIdx + 1) : 'manual';
      return {
        name,
        args: {
          name: wfName,
          description: '',
          trigger,
          steps: [],
        },
      };
    }
    case 'create_team':
    case 'update_team': {
      const spaceIdx = rest.indexOf(' ');
      const id = spaceIdx > 0 ? rest.slice(0, spaceIdx) : rest;
      const teamName = spaceIdx > 0 ? rest.slice(spaceIdx + 1) : '';
      return {
        name,
        args: {
          id,
          name: teamName,
          metadata: {},
          agent_ids: [],
        },
      };
    }
    case 'run_command': {
      return {
        name,
        args: {
          command: parts[1] || '',
          args: parts.slice(2),
        },
      };
    }
    case 'help':
      return { name, args: {} };
    default:
      return undefined;
  }
}
