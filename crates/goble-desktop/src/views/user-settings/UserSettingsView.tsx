import { useEffect } from 'react';
import { useParams } from 'react-router-dom';
import SettingsSidebar from './settings-sidebar/SettingsSidebar';
import SettingsContent from './settings-content/SettingsContent';
import { useUserSettingsStore, type SettingsSection } from './store/userSettingsStore';
import './UserSettingsView.css';

export default function UserSettingsView() {
  const { section } = useParams<{ section: string }>();
  const setSection = useUserSettingsStore((s) => s.setSection);

  useEffect(() => {
    if (section && isSettingsSection(section)) {
      setSection(section);
    }
  }, [section, setSection]);

  return (
    <div className="user-settings-view">
      <SettingsSidebar />
      <SettingsContent />
    </div>
  );
}

function isSettingsSection(value: string): value is SettingsSection {
  return [
    'appearance',
    'general',
    'profile',
    'providers',
    'workers',
    'about',
  ].includes(value as SettingsSection);
}
