export interface Workspace {
  id: string;
  name: string;
  color: string;
  channels: Channel[];
  directMessages: DirectMessage[];
  projects: Project[];
  messagesByChannel: Record<string, ThreadMessage[]>;
  directMessagesById: Record<string, ThreadMessage[]>;
  tags: string[];
}

export interface Channel {
  id: string;
  name: string;
  private: boolean;
  unread: number;
  total: number;
}

export interface DirectMessage {
  id: string;
  name: string;
  unread: number;
}

export interface Project {
  id: string;
  name: string;
  items: { id: string; title: string; status: string }[];
}

export interface ThreadMessage {
  id: string;
  author: string;
  content: string;
  timestamp: string;
  reactions: Record<string, string[]>;
  tags: string[];
  replyTo?: { author: string; content: string } | null;
  attachments?: { type: string; name: string; content?: string }[];
}

export const currentUser = {
  id: 'user-1',
  name: 'Adrian Tucicovenco',
  email: 'adrian@example.com',
};

export const initialWorkspaces: Workspace[] = [
  {
    id: 'workspace-1',
    name: 'Goble Demo',
    color: '#2563eb',
    channels: [
      { id: 'ch-general', name: 'general', private: false, unread: 0, total: 4 },
      { id: 'ch-random', name: 'random', private: false, unread: 2, total: 3 },
      { id: 'ch-design', name: 'design-system', private: false, unread: 0, total: 1 },
      { id: 'ch-private', name: 'private', private: true, unread: 0, total: 0 },
    ],
    directMessages: [
      { id: 'dm-maya', name: 'Maya Chen', unread: 0 },
      { id: 'dm-jordan', name: 'Jordan Brooks', unread: 0 },
      { id: 'dm-fizz', name: 'Fizz', unread: 0 },
    ],
    projects: [
      { id: 'proj-1', name: 'Agent UI', items: [{ id: 't1', title: 'Sidebar component', status: 'done' }, { id: 't2', title: 'Composer variants', status: 'in-progress' }] },
      { id: 'proj-2', name: 'Demo polish', items: [{ id: 't3', title: 'Mock data', status: 'todo' }] },
    ],
    messagesByChannel: {
      'ch-general': [
        { id: 'm1', author: 'Adrian', content: 'Welcome to the Goble demo workspace. 👋', timestamp: '2025-08-01T09:00:00Z', reactions: {}, tags: [], attachments: [{ type: 'image', name: 'welcome.png' }] },
        { id: 'm2', author: 'Maya Chen', content: 'Looks great! The sidebar feels like a Slack/Discord hybrid.', timestamp: '2025-08-01T09:05:00Z', reactions: { '👍': ['Adrian'] }, tags: [] },
        { id: 'm3', author: 'Fizz', content: 'Agent flows are ready for testing. Try running the refactor or deploy flows from the chat page.', timestamp: '2025-08-01T09:12:00Z', reactions: {}, tags: ['#feature'] },
      ],
      'ch-random': [
        { id: 'm4', author: 'Jordan Brooks', content: 'Random thought: what if we named the app "Goblin" instead? 🧌', timestamp: '2025-08-01T10:00:00Z', reactions: { '😂': ['Adrian', 'Maya Chen'] }, tags: [] },
      ],
      'ch-design': [
        { id: 'm5', author: 'Adrian', content: 'Design system tokens are now in the CSS variables. We can switch theme, accent, font, radius and density from Settings > Appearance.', timestamp: '2025-08-01T11:00:00Z', reactions: {}, tags: ['#design'] },
      ],
    },
    directMessagesById: {
      'dm-maya': [
        { id: 'dm1', author: 'Maya Chen', content: 'Hey Adrian, can we sync on the agent cards tomorrow?', timestamp: '2025-08-01T08:00:00Z', reactions: {}, tags: [] },
      ],
      'dm-jordan': [
        { id: 'dm2', author: 'Jordan Brooks', content: 'Sent the draft PR.', timestamp: '2025-08-01T07:30:00Z', reactions: {}, tags: [] },
      ],
      'dm-fizz': [
        { id: 'dm3', author: 'Fizz', content: 'I am ready when you are. Run any flow and I will guide you through it.', timestamp: '2025-08-01T06:00:00Z', reactions: {}, tags: [] },
      ],
    },
    tags: ['#bug', '#feature', '#question', '#release', '#design'],
  },
];

export const reactions = ['👀', '💬', '🎉', '👍', '🔥'];
