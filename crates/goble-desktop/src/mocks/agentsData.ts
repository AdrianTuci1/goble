export interface Agent {
  id: string;
  name: string;
  description: string;
  color: string;
  initials: string;
  tags: string[];
  flowIds?: string[];
}

export const agentsData: Agent[] = [
  {
    id: 'reviewer',
    name: 'Reviewer',
    description: 'Reviews code changes and leaves inline comments.',
    color: '#2563eb',
    initials: 'R',
    tags: ['code', 'review'],
  },
  {
    id: 'refactorer',
    name: 'Refactorer',
    description: 'Refactors a module based on a target quality metric.',
    color: '#10b981',
    initials: 'R',
    tags: ['code', 'refactor'],
    flowIds: ['refactor'],
  },
  {
    id: 'deployer',
    name: 'Deployer',
    description: 'Deploys the latest release to a chosen environment.',
    color: '#f97316',
    initials: 'D',
    tags: ['ops', 'deploy'],
    flowIds: ['deploy'],
  },
  {
    id: 'onboarder',
    name: 'Onboarder',
    description: 'Walks through a repository and answers questions about it.',
    color: '#8b5cf6',
    initials: 'O',
    tags: ['docs', 'explore'],
    flowIds: ['onboard'],
  },
];

export function getAgentById(id: string): Agent | undefined {
  return agentsData.find((a) => a.id === id);
}
