import SettingsSidebar from './settings-sidebar/SettingsSidebar';
import SettingsContent from './settings-content/SettingsContent';
import './UserSettingsView.css';

export default function UserSettingsView() {
  return (
    <div className="user-settings-view">
      <SettingsSidebar />
      <SettingsContent />
    </div>
  );
}
