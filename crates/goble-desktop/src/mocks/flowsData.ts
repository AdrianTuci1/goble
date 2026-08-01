export interface FlowMeta {
  createdBy: string;
  integrations: string[];
  cron: string;
}

export interface FlowInfo {
  id: string;
  title: string;
  meta: FlowMeta;
}

export const flowsData: FlowInfo[] = [
  {
    id: 'refactor',
    title: 'Refactor flow',
    meta: {
      createdBy: 'Adrian Tucicovenco',
      integrations: ['GitHub', 'Vitest', 'Warp'],
      cron: '0 9 * * 1-5',
    },
  },
  {
    id: 'deploy',
    title: 'Deploy flow',
    meta: {
      createdBy: 'Adrian Tucicovenco',
      integrations: ['AWS', 'GitHub Actions'],
      cron: '0 10 * * 1',
    },
  },
  {
    id: 'image',
    title: 'Image analysis flow',
    meta: {
      createdBy: 'Adrian Tucicovenco',
      integrations: ['Vision API'],
      cron: '0 8 * * *',
    },
  },
  {
    id: 'onboard',
    title: 'Onboard flow',
    meta: {
      createdBy: 'Adrian Tucicovenco',
      integrations: ['GitHub', 'Warp'],
      cron: '0 9 * * 1-5',
    },
  },
  {
    id: 'configure',
    title: 'Configure environment',
    meta: {
      createdBy: 'Adrian Tucicovenco',
      integrations: ['AWS', 'Vault'],
      cron: '0 6 * * *',
    },
  },
  {
    id: 'confirm',
    title: 'Confirm action',
    meta: {
      createdBy: 'Adrian Tucicovenco',
      integrations: ['Git'],
      cron: '0 11 * * *',
    },
  },
  {
    id: 'release',
    title: 'Release flow',
    meta: {
      createdBy: 'Adrian Tucicovenco',
      integrations: ['GitHub', 'AWS', 'Slack'],
      cron: '0 14 * * 3',
    },
  },
];

export function getFlowById(id: string): FlowInfo | undefined {
  return flowsData.find((f) => f.id === id);
}
