import { useUserSettingsStore, type SettingsSection } from '../store/userSettingsStore';
import { useDesignStore } from '../../../shared';
import type { ThemeName, FontName, RadiusName, DensityName } from '../../../shared';
import ProvidersSection from './ProvidersSection';
import ProfileSection from './profiles/ProfileSection';
import GeneralSection from './general/GeneralSection';
import WorkerGroupsSection from './workers/WorkerGroupsSection';
import AboutSection from './about/AboutSection';
import './SettingsContent.css';

const themes: { id: ThemeName; label: string }[] = [
  { id: 'dark', label: 'Dark' },
  { id: 'light', label: 'Light' },
  { id: 'midnight', label: 'Midnight' },
];

const fonts: { id: FontName; label: string }[] = [
  { id: 'system', label: 'System' },
  { id: 'mono', label: 'Mono' },
  { id: 'serif', label: 'Serif' },
];

const radiuses: { id: RadiusName; label: string }[] = [
  { id: 'sharp', label: 'Sharp' },
  { id: 'default', label: 'Default' },
  { id: 'rounded', label: 'Rounded' },
];

const densities: { id: DensityName; label: string }[] = [
  { id: 'compact', label: 'Compact' },
  { id: 'default', label: 'Default' },
  { id: 'spacious', label: 'Spacious' },
];

export default function SettingsContent() {
  const { section } = useUserSettingsStore();
  return (
    <main id="settings-content">
      {section === 'appearance' && <AppearanceSection />}
      {section === 'providers' && <ProvidersSection />}
      {section === 'profile' && <ProfileSection />}
      {section === 'general' && <GeneralSection />}
      {section === 'workers' && <WorkerGroupsSection />}
      {section === 'about' && <AboutSection />}
      {section !== 'appearance' && section !== 'providers' && section !== 'profile' && section !== 'general' && section !== 'workers' && section !== 'about' && <PlaceholderSection section={section} />}
    </main>
  );
}

function AppearanceSection() {
  const { design, setTheme, setFont, setRadius, setDensity } = useDesignStore();
  return (
    <div className="settings-page">
      <h2>Appearance</h2>
      <p className="settings-page-sub">Customize how Goble looks and feels.</p>

      <div className="settings-group">
        <h3>Theme</h3>
        <div className="settings-options">
          {themes.map((t) => (
            <button
              key={t.id}
              className={`settings-option ${design.theme === t.id ? 'selected' : ''}`}
              onClick={() => setTheme(t.id)}
            >
              <span className="settings-swatch" data-theme={t.id} />
              {t.label}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-group">
        <h3>Font</h3>
        <div className="settings-options">
          {fonts.map((f) => (
            <button
              key={f.id}
              className={`settings-option ${design.font === f.id ? 'selected' : ''}`}
              onClick={() => setFont(f.id)}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-group">
        <h3>Corner radius</h3>
        <div className="settings-options">
          {radiuses.map((r) => (
            <button
              key={r.id}
              className={`settings-option ${design.radius === r.id ? 'selected' : ''}`}
              onClick={() => setRadius(r.id)}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-group">
        <h3>Density</h3>
        <div className="settings-options">
          {densities.map((d) => (
            <button
              key={d.id}
              className={`settings-option ${design.density === d.id ? 'selected' : ''}`}
              onClick={() => setDensity(d.id)}
            >
              {d.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

const titles: Record<SettingsSection, string> = {
  appearance: 'Appearance',
  general: 'General',
  profile: 'Profile',
  providers: 'Providers',
  workers: 'Workers',
  about: 'About',
};

function PlaceholderSection({ section }: { section: SettingsSection }) {
  return (
    <div className="settings-page">
      <h2>{titles[section]}</h2>
      <p className="settings-page-sub">Settings for {titles[section].toLowerCase()} are coming soon.</p>
    </div>
  );
}
