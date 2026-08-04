import { useNavigate } from 'react-router-dom';
import { Palette, Settings, User, Cpu, Server, Network, KeyRound, Plug, Info, ArrowLeft } from 'lucide-react';
import { useUserSettingsStore, type SettingsSection } from '../store/userSettingsStore';
import './SettingsSidebar.css';

const groups: { title: string; items: { id: SettingsSection; label: string; icon: React.ReactNode }[] }[] = [
  {
    title: 'Appearance',
    items: [{ id: 'appearance', label: 'Appearance', icon: <Palette size={18} /> }],
  },
  {
    title: 'Account',
    items: [
      { id: 'general', label: 'General', icon: <Settings size={18} /> },
      { id: 'profile', label: 'Profile', icon: <User size={18} /> },
    ],
  },
  {
    title: 'Workspace',
    items: [
      { id: 'providers', label: 'Providers', icon: <Cpu size={18} /> },
      { id: 'workers', label: 'Workers', icon: <Server size={18} /> },
      { id: 'cluster', label: 'Cluster', icon: <Network size={18} /> },
      { id: 'vault', label: 'Vault', icon: <KeyRound size={18} /> },
      { id: 'connectors', label: 'Connectors', icon: <Plug size={18} /> },
    ],
  },
  {
    title: 'About',
    items: [{ id: 'about', label: 'About', icon: <Info size={18} /> }],
  },
];

export default function SettingsSidebar() {
  const navigate = useNavigate();
  const { section, setSection } = useUserSettingsStore();

  return (
    <aside id="settings-sidebar" aria-label="Settings">
      <button className="settings-back" onClick={() => navigate('/main/chat')}>
        <span className="settings-back-icon"><ArrowLeft size={16} /></span>
        Back
      </button>
      <div className="settings-menu">
        {groups.map((group) => (
          <div key={group.title} className="settings-menu-group">
            <div className="settings-menu-title">{group.title}</div>
            {group.items.map((item) => (
              <button
                key={item.id}
                className={`settings-menu-item ${section === item.id ? 'selected' : ''}`}
                onClick={() => setSection(item.id)}
              >
                <span className="settings-menu-icon">{item.icon}</span>
                <span>{item.label}</span>
              </button>
            ))}
          </div>
        ))}
      </div>
    </aside>
  );
}
