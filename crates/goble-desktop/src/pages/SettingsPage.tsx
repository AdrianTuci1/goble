import { useEffect, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useStore, type DesignSystem } from '../stores/appStore';
import {
  workerLogs,
  setUserProfile,
  type UserProfile,
  getLlmSetting,
  setLlmSetting,
  LLM_PROVIDERS,
  installWorker,
  listWorkers,
  addWorker,
  pairWorker,
  pingWorker,
  createAgent,
  deleteAgent,
  listAgents,
  getClusterIdentity,
  createCluster,
  importClusterKey,
  exportClusterKey,
  exportClusterBackup,
  unlockClusterIdentity,
  hasClusterIdentity,
  type ClusterIdentityInfo,
} from '../tauri/api';
import ClusterInstallCard from '../components/ClusterInstallCard';
import {
  User,
  Key,
  Monitor,
  Bell,
  Keyboard,
  Archive,
  Users,
  Globe,
  FileText,
  Mail,
  Bot,
  Server,
  FlaskConical,
  Smartphone,
  Download,
} from 'lucide-react';
import './Pages.css';

type SettingsTab =
  | 'profile'
  | 'keys'
  | 'appearance'
  | 'notifications'
  | 'shortcuts'
  | 'local-archive'
  | 'members'
  | 'hosted-communities'
  | 'templates'
  | 'invites'
  | 'settings-agents'
  | 'compute'
  | 'experiments'
  | 'mobile'
  | 'updates';

const MENU_GROUPS = [
  {
    title: 'Personal',
    items: [
      { id: 'profile', label: 'Profile', icon: User },
      { id: 'keys', label: 'Keys', icon: Key },
      { id: 'appearance', label: 'Appearance', icon: Monitor },
      { id: 'notifications', label: 'Notifications', icon: Bell },
      { id: 'shortcuts', label: 'Shortcuts', icon: Keyboard },
      { id: 'local-archive', label: 'Local archive', icon: Archive },
    ],
  },
  {
    title: 'Communities',
    items: [
      { id: 'members', label: 'Members', icon: Users },
      { id: 'hosted-communities', label: 'Hosted communities', icon: Globe },
      { id: 'templates', label: 'Templates', icon: FileText },
      { id: 'invites', label: 'Invites', icon: Mail },
    ],
  },
  {
    title: 'App',
    items: [
      { id: 'settings-agents', label: 'Agents', icon: Bot },
      { id: 'compute', label: 'Compute', icon: Server },
      { id: 'experiments', label: 'Experiments', icon: FlaskConical },
      { id: 'mobile', label: 'Mobile', icon: Smartphone },
      { id: 'updates', label: 'Updates', icon: Download },
    ],
  },
];

export default function SettingsPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const initialTab = (location.state as { tab?: SettingsTab } | null)?.tab;
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab || 'appearance');

  return (
    <div className="settings-page">
      <aside className="settings-sidebar">
        <button className="settings-back" onClick={() => navigate(-1)}>
          ← Back
        </button>
        <div className="settings-menu">
          {MENU_GROUPS.map((group) => (
            <div key={group.title} className="settings-menu-group">
              <h4 className="settings-menu-title">{group.title}</h4>
              {group.items.map((item) => {
                const Icon = item.icon;
                return (
                  <button
                    key={item.id}
                    className={`settings-menu-item ${activeTab === item.id ? 'active' : ''}`}
                    onClick={() => setActiveTab(item.id as SettingsTab)}
                  >
                    <span className="settings-menu-icon">
                      <Icon size={18} />
                    </span>
                    <span className="settings-menu-label">{item.label}</span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      </aside>
      <main className="settings-content">
        {activeTab === 'profile' && <ProfileSettings />}
        {activeTab === 'keys' && <KeysPlaceholder />}
        {activeTab === 'appearance' && <AppearanceSettings />}
        {activeTab === 'notifications' && <NotificationsPlaceholder />}
        {activeTab === 'shortcuts' && <ShortcutsPlaceholder />}
        {activeTab === 'local-archive' && <LocalArchivePlaceholder />}
        {activeTab === 'members' && <MembersPlaceholder />}
        {activeTab === 'hosted-communities' && <HostedCommunitiesPlaceholder />}
        {activeTab === 'templates' && <TemplatesPlaceholder />}
        {activeTab === 'invites' && <InvitesPlaceholder />}
        {activeTab === 'settings-agents' && <AgentsSettings />}
        {activeTab === 'compute' && <ComputeSettings />}
        {activeTab === 'experiments' && <ExperimentsPlaceholder />}
        {activeTab === 'mobile' && <MobilePlaceholder />}
        {activeTab === 'updates' && <UpdatesPlaceholder />}
      </main>
    </div>
  );
}

function ProfileSettings() {
  const profile = useStore((s) => s.userProfile);
  const setProfile = useStore((s) => s.setUserProfile);
  const [name, setName] = useState(profile?.name ?? '');
  const [email, setEmail] = useState(profile?.email ?? '');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (profile) {
      setName(profile.name);
      setEmail(profile.email);
    }
  }, [profile]);

  async function handleSave() {
    setSaving(true);
    try {
      await setUserProfile({ ...profile, name, email } as UserProfile);
      setProfile({ ...profile, name, email } as UserProfile);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="settings-section">
      <h2>Profile</h2>
      <label>Display name</label>
      <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Your name" />
      <label>Email</label>
      <input value={email} onChange={(e) => setEmail(e.target.value)} placeholder="you@example.com" />
      <button onClick={handleSave} disabled={saving || !name}>
        {saving ? 'Saving...' : 'Save profile'}
      </button>
    </div>
  );
}

function KeysPlaceholder() {
  return (
    <div className="settings-section">
      <h2>Keys</h2>
      <p className="hint">Manage workspace and cluster keys here. Key management UI is coming soon.</p>
    </div>
  );
}

function NotificationsPlaceholder() {
  return <Placeholder title="Notifications" />;
}

function ShortcutsPlaceholder() {
  return <Placeholder title="Shortcuts" />;
}

function LocalArchivePlaceholder() {
  return <Placeholder title="Local archive" />;
}

function MembersPlaceholder() {
  return <Placeholder title="Members" />;
}

function HostedCommunitiesPlaceholder() {
  return <Placeholder title="Hosted communities" />;
}

function TemplatesPlaceholder() {
  return <Placeholder title="Templates" />;
}

function InvitesPlaceholder() {
  return <Placeholder title="Invites" />;
}

function ExperimentsPlaceholder() {
  return <Placeholder title="Experiments" />;
}

function MobilePlaceholder() {
  return <Placeholder title="Mobile" />;
}

function UpdatesPlaceholder() {
  return <Placeholder title="Updates" />;
}

function Placeholder({ title }: { title: string }) {
  return (
    <div className="settings-section">
      <h2>{title}</h2>
      <p className="hint">{title} settings are coming soon.</p>
    </div>
  );
}

function AppearanceSettings() {
  const design = useStore((s) => s.design);
  const setDesign = useStore((s) => s.setDesign);

  function update(partial: Partial<DesignSystem>) {
    setDesign({ ...design, ...partial });
  }

  return (
    <div className="settings-section">
      <h2>Appearance</h2>
      <label>Theme</label>
      <select value={design.theme} onChange={(e) => update({ theme: e.target.value as DesignSystem['theme'] })}>
        <option value="dark">Dark</option>
        <option value="light">Light</option>
        <option value="midnight">Midnight</option>
      </select>

      <label>Accent color</label>
      <select value={design.accent} onChange={(e) => update({ accent: e.target.value as DesignSystem['accent'] })}>
        <option value="blue">Blue</option>
        <option value="green">Green</option>
        <option value="purple">Purple</option>
        <option value="orange">Orange</option>
      </select>

      <label>Font</label>
      <select value={design.font} onChange={(e) => update({ font: e.target.value as DesignSystem['font'] })}>
        <option value="system">System</option>
        <option value="mono">Monospace</option>
        <option value="serif">Serif</option>
      </select>

      <label>Density</label>
      <select value={design.density} onChange={(e) => update({ density: e.target.value as DesignSystem['density'] })}>
        <option value="compact">Compact</option>
        <option value="default">Default</option>
        <option value="spacious">Spacious</option>
      </select>

      <label>Radius</label>
      <select value={design.radius} onChange={(e) => update({ radius: e.target.value as DesignSystem['radius'] })}>
        <option value="sharp">Sharp</option>
        <option value="default">Default</option>
        <option value="rounded">Rounded</option>
      </select>
    </div>
  );
}

function AgentsSettings() {
  const agents = useStore((s) => s.agents);
  const setAgents = useStore((s) => s.setAgents);
  const removeAgent = useStore((s) => s.removeAgent);
  const [name, setName] = useState('');
  const [prompt, setPrompt] = useState('');
  const [description, setDescription] = useState('');
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    listAgents().then(setAgents);
  }, [setAgents]);

  async function handleCreate() {
    if (!name.trim() || !prompt.trim()) return;
    setCreating(true);
    try {
      await createAgent(name.trim(), prompt.trim(), description.trim() || undefined, []);
      setAgents(await listAgents());
      setName('');
      setPrompt('');
      setDescription('');
    } finally {
      setCreating(false);
    }
  }

  async function handleDelete(id: string) {
    if (!confirm('Delete this agent?')) return;
    try {
      await deleteAgent(id);
      removeAgent(id);
    } catch {
      // ignore
    }
  }

  return (
    <div className="settings-section">
      <h2>Agents</h2>

      <div className="settings-subsection">
        <h3>Create agent</h3>
        <label>Name</label>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Code Reviewer" />
        <label>Description</label>
        <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Short role summary" />
        <label>Prompt</label>
        <textarea value={prompt} onChange={(e) => setPrompt(e.target.value)} placeholder="System prompt for the agent" rows={5} />
        <button onClick={handleCreate} disabled={creating || !name.trim() || !prompt.trim()}>
          {creating ? 'Creating...' : 'Create agent'}
        </button>
      </div>

      <div className="settings-subsection">
        <h3>Registered agents</h3>
        {agents.length === 0 && <p className="empty">No agents registered.</p>}
        <div className="agent-list">
          {agents.map((a) => (
            <div key={a.id} className="agent-list-item">
              <div>
                <div className="agent-name">{a.name}</div>
                <div className="agent-meta">{a.spec.description || a.spec.prompt.slice(0, 80)}</div>
              </div>
              <button className="danger" onClick={() => handleDelete(a.id)}>Delete</button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function ComputeSettings() {
  return (
    <div className="settings-section">
      <h2>Compute</h2>
      <LlmSettings />
      <WorkerSettings />
    </div>
  );
}

export function LlmSettings() {
  const [provider, setProvider] = useState('openai');
  const [apiKey, setApiKey] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [model, setModel] = useState('gpt-4o-mini');
  const [temperature, setTemperature] = useState('0.7');
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    getLlmSetting(provider).then((setting) => {
      if (setting) {
        setApiKey(setting.api_key);
        setBaseUrl(setting.base_url ?? '');
        setModel(setting.model);
        setTemperature(setting.temperature?.toString() ?? '0.7');
      } else {
        const defaults = LLM_PROVIDERS.find((p) => p.id === provider);
        setModel(defaults?.defaultModel ?? '');
      }
      setSaved(false);
    });
  }, [provider]);

  async function handleSave() {
    await setLlmSetting(
      provider,
      apiKey,
      model,
      baseUrl || undefined,
      parseFloat(temperature)
    );
    setSaved(true);
  }

  return (
    <div className="settings-section">
      <h2>LLM Provider</h2>
      <label>Provider</label>
      <select value={provider} onChange={(e) => setProvider(e.target.value)}>
        {LLM_PROVIDERS.map((p) => (
          <option key={p.id} value={p.id}>{p.name}</option>
        ))}
      </select>
      <label>Model</label>
      <input value={model} onChange={(e) => setModel(e.target.value)} />
      <label>API Key</label>
      <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="sk-..." />
      <label>Base URL (optional)</label>
      <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.openai.com/v1" />
      <label>Temperature</label>
      <input type="number" min="0" max="2" step="0.1" value={temperature} onChange={(e) => setTemperature(e.target.value)} />
      <button onClick={handleSave}>Save LLM Settings</button>
      {saved && <span className="success-hint">Saved</span>}
    </div>
  );
}

export function WorkerSettings() {
  const workers = useStore((s) => s.workers);
  const logs = useStore((s) => s.logs);
  const setWorkers = useStore((s) => s.setWorkers);
  const setLogs = useStore((s) => s.setLogs);

  const [newName, setNewName] = useState('');
  const [newUrl, setNewUrl] = useState('');
  const [pairCode, setPairCode] = useState('');
  const [selectedWorker, setSelectedWorker] = useState('');

  const [sshHost, setSshHost] = useState('');
  const [sshUser, setSshUser] = useState('root');
  const [sshPort, setSshPort] = useState('22');
  const [sshKey, setSshKey] = useState('');
  const [releaseTag, setReleaseTag] = useState('v0.1.0');
  const [installing, setInstalling] = useState(false);
  const [installResult, setInstallResult] = useState<string | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);

  useEffect(() => {
    listWorkers().then(setWorkers);
  }, [setWorkers]);

  async function handleAddWorker() {
    if (!newName || !newUrl) return;
    await addWorker(newName, newUrl);
    setNewName('');
    setNewUrl('');
    listWorkers().then(setWorkers);
  }

  async function handlePair() {
    if (!selectedWorker || !pairCode) return;
    await pairWorker(selectedWorker, pairCode);
    setPairCode('');
    listWorkers().then(setWorkers);
  }

  async function handleInstall() {
    setInstalling(true);
    setInstallResult(null);
    setInstallError(null);
    try {
      const result = await installWorker(sshHost, sshUser, parseInt(sshPort, 10), sshKey, releaseTag, pairCode);
      setInstallResult(
        `Installed ${result.platform.os}/${result.platform.arch} from ${result.asset_url}\n\n${result.install_log}`
      );
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setInstalling(false);
    }
  }

  return (
    <div className="settings-section">
      <h2>Workers</h2>

      <div className="settings-subsection">
        <h3>Registered workers</h3>
        <div className="worker-list">
          {workers.length === 0 && <p className="empty">No workers registered.</p>}
          {workers.map((w) => (
            <div key={w.id} className={`worker-item ${w.paired ? 'paired' : ''}`}>
              <div>
                <div className="worker-name">{w.name}</div>
                <div className="worker-meta">{w.url} — {w.paired ? 'paired' : 'unpaired'}</div>
              </div>
              {w.paired && <button onClick={() => pingWorker(w.id)}>Ping</button>}
            </div>
          ))}
        </div>
      </div>

      <div className="settings-subsection">
        <h3>Add worker</h3>
        <input placeholder="Name" value={newName} onChange={(e) => setNewName(e.target.value)} />
        <input placeholder="ws://host:port/ws" value={newUrl} onChange={(e) => setNewUrl(e.target.value)} />
        <button onClick={handleAddWorker}>Add worker</button>
      </div>

      <div className="settings-subsection">
        <h3>Pair worker</h3>
        <select value={selectedWorker} onChange={(e) => setSelectedWorker(e.target.value)}>
          <option value="">Select worker</option>
          {workers.map((w) => (
            <option key={w.id} value={w.id}>{w.name}</option>
          ))}
        </select>
        <input placeholder="Pairing code" value={pairCode} onChange={(e) => setPairCode(e.target.value)} />
        <button onClick={handlePair}>Pair</button>
      </div>

      <div className="settings-subsection">
        <h3>Install worker on VPS</h3>
        <label>Host</label>
        <input value={sshHost} onChange={(e) => setSshHost(e.target.value)} placeholder="1.2.3.4" />
        <label>User</label>
        <input value={sshUser} onChange={(e) => setSshUser(e.target.value)} />
        <label>SSH port</label>
        <input value={sshPort} onChange={(e) => setSshPort(e.target.value)} />
        <label>Private key</label>
        <textarea value={sshKey} onChange={(e) => setSshKey(e.target.value)} placeholder="-----BEGIN OPENSSH PRIVATE KEY-----" rows={5} />
        <label>Release tag</label>
        <input value={releaseTag} onChange={(e) => setReleaseTag(e.target.value)} />
        <button onClick={handleInstall} disabled={installing || !sshHost || !sshKey}>
          {installing ? 'Installing...' : 'Install / upgrade worker'}
        </button>
        {installResult && <pre className="install-log success">{installResult}</pre>}
        {installError && <div className="card-error">{installError}</div>}
      </div>

      <div className="settings-subsection">
        <h3>Logs</h3>
        <button onClick={() => workerLogs().then(setLogs)}>Refresh logs</button>
        <div className="log-list">
          {logs.slice(-50).map((log) => (
            <div key={log.id} className="log-entry">
              <span className="log-time">{log.timestamp}</span>
              <span className="log-message">{log.message}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export function ClusterSettings() {
  const [identity, setIdentity] = useState<ClusterIdentityInfo | null>(null);
  const [clusterName, setClusterName] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [importKey, setImportKey] = useState('');
  const [importName, setImportName] = useState('');
  const [importPassphrase, setImportPassphrase] = useState('');
  const [unlockPassphrase, setUnlockPassphrase] = useState('');
  const [exportedKey, setExportedKey] = useState('');
  const [exportedBackup, setExportedBackup] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    hasClusterIdentity().then(async (hasWallet) => {
      if (hasWallet) {
        const unlocked = await unlockClusterIdentity('');
        if (unlocked) {
          const info = await getClusterIdentity();
          setIdentity(info);
        }
      }
    });
  }, []);

  async function handleCreate() {
    if (!clusterName || !passphrase) return;
    setError(null);
    try {
      const info = await createCluster(clusterName, passphrase);
      setIdentity(info);
      setClusterName('');
      setPassphrase('');
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleImport() {
    if (!importKey || !importName || !importPassphrase) return;
    setError(null);
    try {
      const info = await importClusterKey(importKey, importName, importPassphrase);
      setIdentity(info);
      setImportKey('');
      setImportName('');
      setImportPassphrase('');
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleUnlock() {
    if (!unlockPassphrase) return;
    setError(null);
    try {
      const unlocked = await unlockClusterIdentity(unlockPassphrase);
      if (unlocked) {
        const info = await getClusterIdentity();
        setIdentity(info);
        setUnlockPassphrase('');
      } else {
        setError('No stored cluster identity found.');
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleExportKey() {
    const key = await exportClusterKey();
    setExportedKey(key);
  }

  async function handleExportBackup() {
    const backup = await exportClusterBackup();
    setExportedBackup(backup);
  }

  return (
    <div className="settings-section">
      <h2>Cluster</h2>

      {error && <div className="card-error">{error}</div>}

      {identity ? (
        <div className="settings-subsection">
          <h3>Active cluster</h3>
          {exportedKey || exportedBackup ? (
            <p className="hint success">Wallet has been exported.</p>
          ) : (
            <p className="hint warning">
              Warning: cluster identity is not exported. Use Export cluster key and Export
              encrypted backup, and save both offline. If this device is lost, the cluster key
              is unrecoverable.
            </p>
          )}
          <p><strong>{identity.cluster_name}</strong></p>
          <p className="hint">Device serial: {identity.device_serial}</p>
          <button onClick={handleExportKey}>Export cluster key</button>
          <button onClick={handleExportBackup}>Export encrypted backup</button>
          {exportedKey && (
            <div>
              <label>Cluster key (save it somewhere safe)</label>
              <textarea value={exportedKey} readOnly rows={3} />
            </div>
          )}
          {exportedBackup && (
            <div>
              <label>Encrypted backup</label>
              <textarea value={exportedBackup} readOnly rows={5} />
            </div>
          )}
        </div>
      ) : (
        <p className="hint">No cluster configured yet. Create or import one, then unlock it.</p>
      )}

      <div className="settings-subsection">
        <h3>Unlock cluster</h3>
        <input type="password" placeholder="Passphrase" value={unlockPassphrase} onChange={(e) => setUnlockPassphrase(e.target.value)} />
        <button onClick={handleUnlock} disabled={!unlockPassphrase}>Unlock</button>
      </div>

      <div className="settings-subsection">
        <h3>Create cluster</h3>
        <input placeholder="Cluster name" value={clusterName} onChange={(e) => setClusterName(e.target.value)} />
        <input type="password" placeholder="Passphrase" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
        <button onClick={handleCreate} disabled={!clusterName || !passphrase}>Create new cluster</button>
      </div>

      <div className="settings-subsection">
        <h3>Import cluster</h3>
        <input placeholder="Cluster name" value={importName} onChange={(e) => setImportName(e.target.value)} />
        <textarea placeholder="Paste cluster key" value={importKey} onChange={(e) => setImportKey(e.target.value)} rows={3} />
        <input type="password" placeholder="Passphrase" value={importPassphrase} onChange={(e) => setImportPassphrase(e.target.value)} />
        <button onClick={handleImport} disabled={!importKey || !importName || !importPassphrase}>Import cluster key</button>
      </div>

      {identity && (
        <div className="settings-subsection">
          <ClusterInstallCard />
        </div>
      )}
    </div>
  );
}
