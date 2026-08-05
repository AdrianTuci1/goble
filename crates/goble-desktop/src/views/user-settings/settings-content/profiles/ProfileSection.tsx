import { useEffect } from 'react';
import { Plus, Pencil, Zap, Terminal, Code, FileText, Command, MessageSquare, Bot, Server, Globe, RefreshCw } from 'lucide-react';
import {
  useProfileStore,
  type Profile,
  type ProfilePermission,
  PERMISSION_ORDER,
  PERMISSION_LABELS,
  LEVEL_LABELS,
  AVAILABLE_MODELS,
  ensureDefaultProfile,
} from '../../store/profileStore';
import ProfileEditor from './ProfileEditor';
import './ProfileSection.css';

export default function ProfileSection() {
  const profiles = useProfileStore((s) => s.profiles);
  const activeProfileId = useProfileStore((s) => s.activeProfileId);
  const editingProfileId = useProfileStore((s) => s.editingProfileId);
  const addProfile = useProfileStore((s) => s.addProfile);
  const setEditingProfile = useProfileStore((s) => s.setEditingProfile);
  const setActiveProfile = useProfileStore((s) => s.setActiveProfile);

  useEffect(() => {
    ensureDefaultProfile();
  }, []);

  const editingProfile = editingProfileId
    ? profiles.find((p) => p.id === editingProfileId)
    : undefined;

  return (
    <div className="profile-section">
      <div className="profile-section-header">
        <div>
          <h2 className="profile-section-title">Profiles</h2>
          <p className="profile-section-subtitle">
            Profiles let you define how your Agent operates — from the actions it can take and when it
            needs approval, to the models it uses for tasks like coding and planning. You can also
            scope them to individual projects.
          </p>
        </div>
        <button className="profile-add-button" onClick={addProfile}>
          <Plus size={16} />
          Add Profile
        </button>
      </div>

      <div className="profile-cards">
        {profiles.map((profile) => (
          <ProfileCard
            key={profile.id}
            profile={profile}
            isActive={profile.id === activeProfileId}
            onEdit={() => setEditingProfile(profile.id)}
            onActivate={() => setActiveProfile(profile.id)}
          />
        ))}
      </div>

      {editingProfile && (
        <ProfileEditor profile={editingProfile} onClose={() => setEditingProfile(null)} />
      )}
    </div>
  );
}

interface ProfileCardProps {
  profile: Profile;
  isActive: boolean;
  onEdit: () => void;
  onActivate: () => void;
}

function ProfileCard({ profile, isActive, onEdit, onActivate }: ProfileCardProps) {
  const baseModelLabel = AVAILABLE_MODELS.find((m) => m.id === profile.models.base)?.label ?? profile.models.base;
  const terminalModelLabel = AVAILABLE_MODELS.find((m) => m.id === profile.models.terminal)?.label ?? profile.models.terminal;

  return (
    <div className={`profile-card ${isActive ? 'active' : ''}`}>
      <div className="profile-card-header">
        <div className="profile-card-title-row">
          <span className="profile-card-name">{profile.name}</span>
          {isActive && <span className="profile-card-active-badge">Active</span>}
        </div>
        <button className="profile-card-edit" onClick={onEdit}>
          <Pencil size={14} />
          Edit
        </button>
      </div>

      <div className="profile-card-body">
        <div className="profile-card-group-title">Models</div>
        <div className="profile-card-row">
          <Zap size={14} />
          <span className="profile-card-label">Base model:</span>
          <span className="profile-card-value">{baseModelLabel}</span>
        </div>
        <div className="profile-card-row">
          <Terminal size={14} />
          <span className="profile-card-label">Full terminal use:</span>
          <span className="profile-card-value">{terminalModelLabel}</span>
        </div>

        <div className="profile-card-group-title">Permissions</div>
        {PERMISSION_ORDER.map((permission) => {
          const Icon = PERMISSION_CARD_ICONS[permission];
          const { label } = PERMISSION_LABELS[permission];
          const level = profile.permissions[permission];
          const levelLabel = LEVEL_LABELS[level];
          return (
            <div className="profile-card-row" key={permission}>
              <Icon size={14} />
              <span className="profile-card-label">{label}:</span>
              <span className="profile-card-value">{levelLabel}</span>
            </div>
          );
        })}
      </div>

      {!isActive && (
        <button className="profile-card-activate" onClick={onActivate}>
          Activate
        </button>
      )}
    </div>
  );
}

const PERMISSION_CARD_ICONS: Record<ProfilePermission, React.ComponentType<{ size?: number }>> = {
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
