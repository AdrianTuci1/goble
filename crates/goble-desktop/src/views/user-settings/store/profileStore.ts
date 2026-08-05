import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type PermissionLevel = 'agent-decides' | 'always-ask' | 'always-allow';

export type ProfilePermission =
  | 'applyCodeDiffs'
  | 'readFiles'
  | 'executeCommands'
  | 'interactWithRunningCommands'
  | 'askQuestions'
  | 'runAgents'
  | 'callMcpServers'
  | 'callWebTools'
  | 'autoSyncPlans';

export interface ProfileAllowlists {
  directory: string;
  command: string;
  mcp: string;
  mcpDeny: string;
}

export interface ProfileModels {
  base: string;
  terminal: string;
}

export interface Profile {
  id: string;
  name: string;
  models: ProfileModels;
  permissions: Record<ProfilePermission, PermissionLevel>;
  allowlists: ProfileAllowlists;
  isDefault: boolean;
}

export const PERMISSION_LABELS: Record<ProfilePermission, { label: string; hint: string }> = {
  applyCodeDiffs: {
    label: 'Apply code diffs',
    hint: 'The agent can modify files by applying code diffs.',
  },
  readFiles: {
    label: 'Read files',
    hint: 'The agent can read files from the workspace.',
  },
  executeCommands: {
    label: 'Execute commands',
    hint: 'The agent can run shell commands in the terminal.',
  },
  interactWithRunningCommands: {
    label: 'Interact with running commands',
    hint: 'The agent can send input to and read output from running terminal processes.',
  },
  askQuestions: {
    label: 'Ask questions',
    hint: 'When to ask the user for clarification.',
  },
  runAgents: {
    label: 'Run agents',
    hint: 'The agent can start other agents or workflows.',
  },
  callMcpServers: {
    label: 'Call MCP servers',
    hint: 'The agent can invoke tools exposed by MCP servers.',
  },
  callWebTools: {
    label: 'Call web tools',
    hint: 'The agent can use web search and other web-based tools.',
  },
  autoSyncPlans: {
    label: 'Auto-sync plans to Warp Drive',
    hint: 'Automatically save generated plans to Warp Drive.',
  },
};

export const PERMISSION_ORDER: ProfilePermission[] = [
  'applyCodeDiffs',
  'readFiles',
  'executeCommands',
  'interactWithRunningCommands',
  'askQuestions',
  'runAgents',
  'callMcpServers',
  'callWebTools',
  'autoSyncPlans',
];

export const LEVEL_LABELS: Record<PermissionLevel, string> = {
  'agent-decides': 'Agent decides',
  'always-ask': 'Always ask',
  'always-allow': 'Always allow',
};

export const DEFAULT_MODELS: ProfileModels = {
  base: 'kimi-k3',
  terminal: 'kimi-k3',
};

export const DEFAULT_PERMISSIONS: Record<ProfilePermission, PermissionLevel> = {
  applyCodeDiffs: 'agent-decides',
  readFiles: 'agent-decides',
  executeCommands: 'always-ask',
  interactWithRunningCommands: 'always-allow',
  askQuestions: 'agent-decides',
  runAgents: 'always-ask',
  callMcpServers: 'agent-decides',
  callWebTools: 'always-allow',
  autoSyncPlans: 'always-allow',
};

export const DEFAULT_ALLOWLISTS: ProfileAllowlists = {
  directory: '',
  command: '',
  mcp: '',
  mcpDeny: '',
};

export const AVAILABLE_MODELS = [
  { id: 'kimi-k3', label: 'Kimi K3' },
  { id: 'kimi-k27', label: 'Kimi K2.7 Custom' },
  { id: 'kimi-k2', label: 'Kimi K2' },
  { id: 'claude-3-5-sonnet', label: 'Claude 3.5 Sonnet' },
  { id: 'claude-3-5-opus', label: 'Claude 3.5 Opus' },
  { id: 'gpt-4o', label: 'GPT-4o' },
  { id: 'gpt-4o-mini', label: 'GPT-4o mini' },
  { id: 'local', label: 'Local / Custom' },
];

function createDefaultProfile(): Profile {
  return {
    id: 'default',
    name: 'Default',
    models: { ...DEFAULT_MODELS },
    permissions: { ...DEFAULT_PERMISSIONS },
    allowlists: { ...DEFAULT_ALLOWLISTS },
    isDefault: true,
  };
}

interface ProfileState {
  profiles: Profile[];
  activeProfileId: string;
  editingProfileId: string | null;
  addProfile: () => void;
  updateProfile: (id: string, updates: Partial<Profile>) => void;
  deleteProfile: (id: string) => void;
  setActiveProfile: (id: string) => void;
  setEditingProfile: (id: string | null) => void;
  getProfile: (id: string) => Profile | undefined;
  getActiveProfile: () => Profile;
}

export const useProfileStore = create<ProfileState>()(
  persist(
    (set, get) => ({
      profiles: [createDefaultProfile()],
      activeProfileId: 'default',
      editingProfileId: null,

      addProfile: () => {
        const newProfile: Profile = {
          id: `profile-${Date.now()}`,
          name: 'New profile',
          models: { ...DEFAULT_MODELS },
          permissions: { ...DEFAULT_PERMISSIONS },
          allowlists: { ...DEFAULT_ALLOWLISTS },
          isDefault: false,
        };
        set((state) => ({
          profiles: [...state.profiles, newProfile],
          editingProfileId: newProfile.id,
        }));
      },

      updateProfile: (id, updates) => {
        set((state) => ({
          profiles: state.profiles.map((p) =>
            p.id === id ? { ...p, ...updates } : p
          ),
        }));
      },

      deleteProfile: (id) => {
        set((state) => {
          if (state.profiles.length <= 1) return state;
          const remaining = state.profiles.filter((p) => p.id !== id);
          const nextActive = state.activeProfileId === id
            ? remaining[0].id
            : state.activeProfileId;
          return {
            profiles: remaining,
            activeProfileId: nextActive,
            editingProfileId: state.editingProfileId === id ? null : state.editingProfileId,
          };
        });
      },

      setActiveProfile: (id) => {
        if (get().profiles.some((p) => p.id === id)) {
          set({ activeProfileId: id });
        }
      },

      setEditingProfile: (id) => set({ editingProfileId: id }),

      getProfile: (id) => get().profiles.find((p) => p.id === id),

      getActiveProfile: () => {
        const active = get().profiles.find((p) => p.id === get().activeProfileId);
        return active ?? get().profiles[0] ?? createDefaultProfile();
      },
    }),
    {
      name: 'goble-profiles',
      partialize: (state) => ({
        profiles: state.profiles,
        activeProfileId: state.activeProfileId,
      }),
    }
  )
);

export function ensureDefaultProfile() {
  const state = useProfileStore.getState();
  if (state.profiles.length === 0) {
    useProfileStore.setState({
      profiles: [createDefaultProfile()],
      activeProfileId: 'default',
    });
  }
}
