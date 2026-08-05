import { X, Terminal, Code, FileText, Command, MessageSquare, Bot, Server, Globe, RefreshCw, FolderOpen, Ban, List } from 'lucide-react';
import {
  useProfileStore,
  type Profile,
  type ProfilePermission,
  type ProfileModels,
  type ProfileAllowlists,
  PERMISSION_LABELS,
  PERMISSION_ORDER,
  LEVEL_LABELS,
  AVAILABLE_MODELS,
} from '../../store/profileStore';
import './ProfileEditor.css';

interface ProfileEditorProps {
  profile: Profile;
  onClose: () => void;
}

export default function ProfileEditor({ profile, onClose }: ProfileEditorProps) {
  const updateProfile = useProfileStore((s) => s.updateProfile);
  const deleteProfile = useProfileStore((s) => s.deleteProfile);

  function updateName(name: string) {
    updateProfile(profile.id, { name });
  }

  function updateModels(models: Partial<ProfileModels>) {
    updateProfile(profile.id, { models: { ...profile.models, ...models } });
  }

  function updatePermission(permission: ProfilePermission, level: Profile[keyof Profile]) {
    updateProfile(profile.id, {
      permissions: { ...profile.permissions, [permission]: level },
    });
  }

  function updateAllowlists(allowlists: Partial<ProfileAllowlists>) {
    updateProfile(profile.id, { allowlists: { ...profile.allowlists, ...allowlists } });
  }

  function handleDelete() {
    if (profile.isDefault) return;
    deleteProfile(profile.id);
    onClose();
  }

  return (
    <div className="profile-editor" role="dialog" aria-modal="true">
      <div className="profile-editor-header">
        <h2 className="profile-editor-title">Edit Profile</h2>
        <button className="profile-editor-close" onClick={onClose} aria-label="Close editor">
          <X size={18} />
        </button>
      </div>

      <div className="profile-editor-body">
        <div className="profile-editor-field">
          <label className="profile-editor-label">Name</label>
          <input
            className="profile-editor-input"
            value={profile.name}
            onChange={(e) => updateName(e.target.value)}
            placeholder='e.g. "YOLO code"'
          />
        </div>

        <div className="profile-editor-section">
          <div className="profile-editor-section-title">Models</div>

          <div className="profile-editor-field">
            <label className="profile-editor-label">Base model</label>
            <p className="profile-editor-hint">
              This model serves as the primary engine behind the agent. It powers most interactions
              and invokes other models for tasks like planning or code generation when necessary.
            </p>
            <select
              className="profile-editor-select"
              value={profile.models.base}
              onChange={(e) => updateModels({ base: e.target.value })}
            >
              {AVAILABLE_MODELS.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                </option>
              ))}
            </select>
          </div>

          <div className="profile-editor-field">
            <label className="profile-editor-label">Full terminal use model</label>
            <p className="profile-editor-hint">
              The model used when the agent operates inside interactive terminal applications like
              database shells, debuggers, REPLs, or dev servers.
            </p>
            <select
              className="profile-editor-select"
              value={profile.models.terminal}
              onChange={(e) => updateModels({ terminal: e.target.value })}
            >
              {AVAILABLE_MODELS.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="profile-editor-section">
          <div className="profile-editor-section-title">Permissions</div>

          {PERMISSION_ORDER.map((permission) => {
            const { label, hint } = PERMISSION_LABELS[permission];
            const Icon = PERMISSION_ICONS[permission];
            return (
              <div className="profile-editor-permission" key={permission}>
                <div className="profile-editor-permission-header">
                  <span className="profile-editor-permission-icon">
                    <Icon size={18} />
                  </span>
                  <span className="profile-editor-permission-label">{label}</span>
                </div>
                <p className="profile-editor-permission-hint">{hint}</p>
                <select
                  className="profile-editor-select"
                  value={profile.permissions[permission]}
                  onChange={(e) => updatePermission(permission, e.target.value as Profile['permissions'][ProfilePermission])}
                >
                  <option value="agent-decides">{LEVEL_LABELS['agent-decides']}</option>
                  <option value="always-ask">{LEVEL_LABELS['always-ask']}</option>
                  <option value="always-allow">{LEVEL_LABELS['always-allow']}</option>
                </select>
              </div>
            );
          })}
        </div>

        <div className="profile-editor-section">
          <div className="profile-editor-section-title">Allowlists</div>

          <div className="profile-editor-field">
            <label className="profile-editor-label">
              <FolderOpen size={16} /> Directory allowlist
            </label>
            <p className="profile-editor-hint">Give the agent file access to certain directories.</p>
            <input
              className="profile-editor-input"
              value={profile.allowlists.directory}
              onChange={(e) => updateAllowlists({ directory: e.target.value })}
              placeholder="e.g. ~/code-repos/repo"
            />
          </div>

          <div className="profile-editor-field">
            <label className="profile-editor-label">
              <Command size={16} /> Command allowlist
            </label>
            <p className="profile-editor-hint">Commands the agent is allowed to run without asking.</p>
            <input
              className="profile-editor-input"
              value={profile.allowlists.command}
              onChange={(e) => updateAllowlists({ command: e.target.value })}
              placeholder="e.g. git, npm, docker"
            />
          </div>

          <div className="profile-editor-field">
            <label className="profile-editor-label">
              <List size={16} /> MCP allowlist
            </label>
            <p className="profile-editor-hint">MCP servers the agent is allowed to call.</p>
            <input
              className="profile-editor-input"
              value={profile.allowlists.mcp}
              onChange={(e) => updateAllowlists({ mcp: e.target.value })}
              placeholder="e.g. filesystem, fetch"
            />
          </div>

          <div className="profile-editor-field">
            <label className="profile-editor-label">
              <Ban size={16} /> MCP denylist
            </label>
            <p className="profile-editor-hint">MCP servers the agent is never allowed to call.</p>
            <input
              className="profile-editor-input"
              value={profile.allowlists.mcpDeny}
              onChange={(e) => updateAllowlists({ mcpDeny: e.target.value })}
              placeholder="e.g. shell, filesystem-write"
            />
          </div>
        </div>

        {!profile.isDefault && (
          <div className="profile-editor-actions">
            <button className="profile-editor-delete" onClick={handleDelete}>
              Delete profile
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

const PERMISSION_ICONS: Record<ProfilePermission, React.ComponentType<{ size?: number }>> = {
  applyCodeDiffs: Code,
  readFiles: FileText,
  executeCommands: Command,
  interactWithRunningCommands: Terminal,
  askQuestions: MessageSquare,
  runAgents: Bot,
  callMcpServers: Server,
  callWebTools: Globe,
  autoSyncPlans: RefreshCw,
};
