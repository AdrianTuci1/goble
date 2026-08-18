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
  getAuthorizedKeys,
  addAuthorizedKey,
  getClusterIdentity,
  createCluster,
  importClusterKey,
  exportIdentityWallet,
  importIdentityWallet,
  exportClusterKey,
  exportClusterBackup,
  generateWorkerInvite,
  unlockClusterIdentity,
  hasClusterIdentity,
  type ClusterIdentityInfo,
  type AuthorizedKey,
} from '../tauri/api';
import {
  User,
  Key,
  Monitor,
  Bell,
  Keyboard,
  Archive,
  Bot,
  Server,
  Smartphone,
  Download,
  ArrowLeft,
} from 'lucide-react';
import './Pages.css';
import ClusterInstallCard from '../components/ClusterInstallCard';

type SettingsTab =
  | 'profile'
  | 'keys'
  | 'appearance'
  | 'notifications'
  | 'shortcuts'
  | 'local-archive'
  | 'settings-agents'
  | 'compute'
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
    title: 'App',
    items: [
      { id: 'settings-agents', label: 'Agents', icon: Bot },
      { id: 'compute', label: 'Compute', icon: Server },
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
          <span className="settings-back-icon">
            <ArrowLeft size={16} />
          </span>
          Back
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
                    className={`settings-menu-item ${activeTab === item.id ? 'selected' : ''}`}
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
        {activeTab === 'keys' && <KeysSettings />}
        {activeTab === 'appearance' && <AppearanceSettings />}
        {activeTab === 'notifications' && <NotificationsSettings />}
        {activeTab === 'shortcuts' && <ShortcutsSettings />}
        {activeTab === 'local-archive' && <LocalArchiveSettings />}
        {activeTab === 'settings-agents' && <AgentsSettings />}
        {activeTab === 'compute' && <ComputeSettings />}
        {activeTab === 'mobile' && <MobileSettings />}
        {activeTab === 'updates' && <UpdatesSettings />}
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
      <p className="settings-page-sub">Manage your public profile information.</p>
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

function KeysSettings() {
  const [keys, setKeys] = useState<AuthorizedKey[]>([]);
  const [pem, setPem] = useState('');
  const [name, setName] = useState('');
  const [loading, setLoading] = useState(false);
  const [exportPass, setExportPass] = useState('');
  const [importWallet, setImportWallet] = useState('');
  const [importPass, setImportPass] = useState('');
  const [importing, setImporting] = useState(false);
  const [cluster, setCluster] = useState<ClusterIdentityInfo | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    getAuthorizedKeys().then(setKeys).catch(() => {});
    getClusterIdentity().then(setCluster).catch(() => {});
  }, []);

  async function handleAdd() {
    if (!pem.trim() || !name.trim()) return;
    setLoading(true);
    try {
      await addAuthorizedKey(pem.trim(), name.trim());
      setKeys(await getAuthorizedKeys());
      setPem('');
      setName('');
    } finally {
      setLoading(false);
    }
  }

  async function handleExportIdentity() {
    if (!exportPass.trim()) return;
    setMessage(null);
    try {
      const wallet = await exportIdentityWallet(exportPass.trim());
      const blob = new Blob([wallet], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `goble-identity-wallet-${cluster?.cluster_name ?? 'cluster'}.json`;
      a.click();
      URL.revokeObjectURL(url);
      setMessage('Identity wallet exported.');
    } catch (e: unknown) {
      setMessage(`Export failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  async function handleImportIdentity() {
    if (!importWallet.trim() || !importPass.trim()) return;
    setImporting(true);
    setMessage(null);
    try {
      const info = await importIdentityWallet(importWallet.trim(), importPass.trim());
      setCluster(info);
      setMessage('Identity imported successfully.');
      setImportWallet('');
      setImportPass('');
    } catch (e: unknown) {
      setMessage(`Import failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setImporting(false);
    }
  }

  function fingerprint(pem: string): string {
    let hash = 0;
    for (const c of pem.trim()) hash = c.charCodeAt(0) + ((hash << 5) - hash);
    return Math.abs(hash).toString(16).padStart(8, '0').slice(0, 16);
  }

  return (
    <div className="settings-section">
      <h2>Keys</h2>
      <p className="settings-page-sub">Manage cluster identity and authorized public keys.</p>
      <div className="settings-subsection">
        <h3>Cluster identity</h3>
        {cluster ? (
          <div className="panel-section">
            <div className="panel-label">Cluster</div>
            <div className="panel-value">{cluster.cluster_name}</div>
            <div className="panel-label">Device serial</div>
            <div className="panel-value">{cluster.device_serial}</div>
          </div>
        ) : (
          <p className="empty">No cluster identity unlocked. Create or import a cluster first.</p>
        )}
      </div>
      <div className="settings-subsection">
        <h3>Export identity wallet</h3>
        <p className="hint">Download an encrypted wallet containing your cluster CA and device credentials. Keep it safe.</p>
        <input type="password" value={exportPass} onChange={(e) => setExportPass(e.target.value)} placeholder="Passphrase" />
        <button onClick={handleExportIdentity} disabled={!exportPass.trim() || !cluster}>
          Export wallet
        </button>
      </div>
      <div className="settings-subsection">
        <h3>Import identity wallet</h3>
        <textarea value={importWallet} onChange={(e) => setImportWallet(e.target.value)} placeholder="Paste encrypted wallet JSON" rows={6} />
        <input type="password" value={importPass} onChange={(e) => setImportPass(e.target.value)} placeholder="Passphrase" />
        <button onClick={handleImportIdentity} disabled={importing || !importWallet.trim() || !importPass.trim()}>
          {importing ? 'Importing...' : 'Import wallet'}
        </button>
      </div>
      <div className="settings-subsection">
        <h3>Add public key</h3>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Key name" />
        <textarea value={pem} onChange={(e) => setPem(e.target.value)} placeholder="Paste PEM public key" rows={4} />
        <button onClick={handleAdd} disabled={loading || !pem.trim() || !name.trim()}>
          {loading ? 'Adding...' : 'Add key'}
        </button>
      </div>
      <div className="settings-subsection">
        <h3>Authorized keys</h3>
        {keys.length === 0 && <p className="empty">No authorized keys.</p>}
        <div className="key-list">
          {keys.map((k) => (
            <div key={k.id} className="key-list-item">
              <div>
                <div className="key-name">{k.name}</div>
                <div className="key-meta">Fingerprint: {k.fingerprint || fingerprint(k.public_key_pem)}</div>
              </div>
            </div>
          ))}
        </div>
      </div>
      {message && <p className="success-hint">{message}</p>}
    </div>
  );
}

function NotificationsSettings() {
  const [enabled, setEnabled] = useState(() => {
    try { return localStorage.getItem('goble-notifications-enabled') === 'true'; } catch { return false; }
  });
  const [sound, setSound] = useState(() => {
    try { return localStorage.getItem('goble-notifications-sound') === 'true'; } catch { return false; }
  });
  const [mentions, setMentions] = useState(() => {
    try { return localStorage.getItem('goble-notifications-mentions') !== 'false'; } catch { return true; }
  });

  function update(key: string, value: boolean, setter: (v: boolean) => void) {
    setter(value);
    try { localStorage.setItem(key, String(value)); } catch { /* ignored */ }
  }

  return (
    <div className="settings-section">
      <h2>Notifications</h2>
      <p className="settings-page-sub">Choose when and how Goble notifies you.</p>
      <label className="checkbox-row">
        <input type="checkbox" checked={enabled} onChange={(e) => update('goble-notifications-enabled', e.target.checked, setEnabled)} />
        Enable desktop notifications
      </label>
      <label className="checkbox-row">
        <input type="checkbox" checked={sound} onChange={(e) => update('goble-notifications-sound', e.target.checked, setSound)} />
        Play sound on new message
      </label>
      <label className="checkbox-row">
        <input type="checkbox" checked={mentions} onChange={(e) => update('goble-notifications-mentions', e.target.checked, setMentions)} />
        Notify on mentions
      </label>
    </div>
  );
}

function ShortcutsSettings() {
  return (
    <div className="settings-section">
      <h2>Keyboard shortcuts</h2>
      <p className="settings-page-sub">Default keyboard shortcuts for Goble.</p>
      <div className="shortcut-list">
        <div className="shortcut-row">
          <kbd>Cmd</kbd> / <kbd>Ctrl</kbd> + <kbd>K</kbd>
          <span>Open quick search</span>
        </div>
        <div className="shortcut-row">
          <kbd>Esc</kbd>
          <span>Close side panel / go back</span>
        </div>
        <div className="shortcut-row">
          <kbd>Cmd</kbd> / <kbd>Ctrl</kbd> + <kbd>/</kbd>
          <span>Toggle right sidebar</span>
        </div>
      </div>
    </div>
  );
}

function LocalArchiveSettings() {
  const threads = useStore((s) => s.threads);
  const threadMessages = useStore((s) => s.threadMessages);
  const profile = useStore((s) => s.userProfile);
  const [importText, setImportText] = useState('');
  const [message, setMessage] = useState<string | null>(null);

  function handleExport() {
    const payload = {
      version: 1,
      exported_at: new Date().toISOString(),
      profile,
      threads,
      messages: threadMessages,
    };
    const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `goble-archive-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    URL.revokeObjectURL(url);
    setMessage('Archive exported.');
  }

  async function handleImport() {
    if (!importText.trim()) return;
    setMessage(null);
    try {
      const data = JSON.parse(importText);
      if (data.threads && Array.isArray(data.threads)) {
        // For now just validate; real merge would require backend commands.
        setMessage(`Archive valid: ${data.threads.length} threads, ${Object.keys(data.messages || {}).length} message lists.`);
      } else {
        setMessage('Invalid archive format.');
      }
    } catch {
      setMessage('Invalid JSON.');
    }
  }

  return (
    <div className="settings-section">
      <h2>Local archive</h2>
      <p className="settings-page-sub">Export or import a local snapshot of your data.</p>
      <div className="settings-subsection">
        <h3>Export</h3>
        <p className="hint">Download a JSON snapshot of your local threads, messages, and profile.</p>
        <button onClick={handleExport}>Export archive</button>
      </div>
      <div className="settings-subsection">
        <h3>Import</h3>
        <textarea value={importText} onChange={(e) => setImportText(e.target.value)} placeholder="Paste archive JSON" rows={8} />
        <button onClick={handleImport}>Validate import</button>
      </div>
      {message && <p className="success-hint">{message}</p>}
    </div>
  );
}

function MobileSettings() {
  return (
    <div className="settings-section">
      <h2>Mobile</h2>
      <p className="settings-page-sub">Pair the mobile companion app when it becomes available.</p>
      <p className="hint">Mobile companion app is in development. When released, scan the QR code here to pair this device.</p>
    </div>
  );
}

function UpdatesSettings() {
  const [checking, setChecking] = useState(false);
  const [version] = useState('0.1.0');

  function handleCheck() {
    setChecking(true);
    setTimeout(() => setChecking(false), 1500);
  }

  return (
    <div className="settings-section">
      <h2>Updates</h2>
      <p className="settings-page-sub">Check for new Goble releases and read release notes.</p>
      <div className="panel-section">
        <div className="panel-label">Current version</div>
        <div className="panel-value">{version}</div>
      </div>
      <button onClick={handleCheck} disabled={checking}>
        {checking ? 'Checking...' : 'Check for updates'}
      </button>
      <p className="hint">Release notes and automatic updates will be available once the updater is wired.</p>
    </div>
  );
}

const THEMES: { id: DesignSystem['theme']; label: string }[] = [
  { id: 'dark', label: 'Dark' },
  { id: 'light', label: 'Light' },
  { id: 'midnight', label: 'Midnight' },
];

const FONTS: { id: DesignSystem['font']; label: string }[] = [
  { id: 'system', label: 'System' },
  { id: 'mono', label: 'Mono' },
  { id: 'serif', label: 'Serif' },
];

const RADII: { id: DesignSystem['radius']; label: string }[] = [
  { id: 'sharp', label: 'Sharp' },
  { id: 'default', label: 'Default' },
  { id: 'rounded', label: 'Rounded' },
];

const DENSITIES: { id: DesignSystem['density']; label: string }[] = [
  { id: 'compact', label: 'Compact' },
  { id: 'default', label: 'Default' },
  { id: 'spacious', label: 'Spacious' },
];

function AppearanceSettings() {
  const design = useStore((s) => s.design);
  const setDesign = useStore((s) => s.setDesign);

  function update(partial: Partial<DesignSystem>) {
    setDesign({ ...design, ...partial });
  }

  return (
    <div className="settings-section">
      <h2>Appearance</h2>
      <p className="settings-page-sub">Customize how Goble looks and feels.</p>

      <div className="settings-group">
        <h3>Theme</h3>
        <div className="settings-options">
          {THEMES.map((t) => (
            <button
              key={t.id}
              className={`settings-option ${design.theme === t.id ? 'selected' : ''}`}
              onClick={() => update({ theme: t.id })}
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
          {FONTS.map((f) => (
            <button
              key={f.id}
              className={`settings-option ${design.font === f.id ? 'selected' : ''}`}
              onClick={() => update({ font: f.id })}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-group">
        <h3>Corner radius</h3>
        <div className="settings-options">
          {RADII.map((r) => (
            <button
              key={r.id}
              className={`settings-option ${design.radius === r.id ? 'selected' : ''}`}
              onClick={() => update({ radius: r.id })}
            >
              {r.label}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-group">
        <h3>Density</h3>
        <div className="settings-options">
          {DENSITIES.map((d) => (
            <button
              key={d.id}
              className={`settings-option ${design.density === d.id ? 'selected' : ''}`}
              onClick={() => update({ density: d.id })}
            >
              {d.label}
            </button>
          ))}
        </div>
      </div>
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
      <p className="settings-page-sub">Create and manage local agents.</p>

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
      <p className="settings-page-sub">Configure workers and model providers.</p>
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
  const [workerInvite, setWorkerInvite] = useState('');
  const [workerInviteName, setWorkerInviteName] = useState('');
  const [workerInviteCopied, setWorkerInviteCopied] = useState(false);
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

  async function handleGenerateWorkerInvite() {
    if (!identity) return;
    setError(null);
    try {
      const workerId = crypto.randomUUID();
      const invite = await generateWorkerInvite(workerId, workerInviteName || undefined);
      setWorkerInvite(JSON.stringify(invite, null, 2));
      setWorkerInviteCopied(false);
    } catch (e) {
      setError(String(e));
    }
  }

  function copyWorkerInvite() {
    navigator.clipboard.writeText(workerInvite).then(() => {
      setWorkerInviteCopied(true);
      setTimeout(() => setWorkerInviteCopied(false), 1500);
    });
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
          <h3>Worker invite</h3>
          <p className="hint">Generate a one-liner invitation for a new worker. Copy it and run it on the worker host.</p>
          <input placeholder="Worker name (optional)" value={workerInviteName} onChange={(e) => setWorkerInviteName(e.target.value)} />
          <button onClick={handleGenerateWorkerInvite} disabled={!identity}>Generate worker invite</button>
          {workerInvite && (
            <div>
              <label>Invite bundle</label>
              <textarea value={workerInvite} readOnly rows={8} />
              <button onClick={copyWorkerInvite}>{workerInviteCopied ? 'Copied!' : 'Copy invite'}</button>
            </div>
          )}
        </div>
      )}

      {identity && (
        <div className="settings-subsection">
          <ClusterInstallCard />
        </div>
      )}
    </div>
  );
}
