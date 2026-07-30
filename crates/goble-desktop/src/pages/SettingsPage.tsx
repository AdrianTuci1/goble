import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useStore } from '../stores/appStore';
import {
  workerLogs,
  getLlmSetting,
  setLlmSetting,
  LLM_PROVIDERS,
  installWorker,
  listWorkers,
  addWorker,
  pairWorker,
  pingWorker,
  getClusterIdentity,
  createCluster,
  importClusterKey,
  exportClusterKey,
  exportClusterBackup,
  type ClusterIdentityInfo,
} from '../tauri/api';

type SettingsTab = 'profile' | 'llm' | 'workers' | 'cluster' | 'appearance';

export default function SettingsPage() {
  const navigate = useNavigate();
  const [activeTab, setActiveTab] = useState<SettingsTab>('profile');

  return (
    <div className="settings-page">
      <aside className="settings-sidebar">
        <button className="settings-back" onClick={() => navigate(-1)}>
          ← Back
        </button>
        <nav className="settings-menu">
          <SettingsMenuItem
            label="Profile"
            active={activeTab === 'profile'}
            onClick={() => setActiveTab('profile')}
          />
          <SettingsMenuItem
            label="LLM"
            active={activeTab === 'llm'}
            onClick={() => setActiveTab('llm')}
          />
          <SettingsMenuItem
            label="Workers"
            active={activeTab === 'workers'}
            onClick={() => setActiveTab('workers')}
          />
          <SettingsMenuItem
            label="Cluster"
            active={activeTab === 'cluster'}
            onClick={() => setActiveTab('cluster')}
          />
          <SettingsMenuItem
            label="Appearance"
            active={activeTab === 'appearance'}
            onClick={() => setActiveTab('appearance')}
          />
          </nav>
      </aside>
      <main className="settings-content">
        {activeTab === 'profile' && <ProfileSettings />}
        {activeTab === 'llm' && <LlmSettings />}
        {activeTab === 'workers' && <WorkerSettings />}
        {activeTab === 'cluster' && <ClusterSettings />}
        {activeTab === 'appearance' && <AppearanceSettings />}
      </main>
    </div>
  );
}

function SettingsMenuItem({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button className={`settings-menu-item ${active ? 'active' : ''}`} onClick={onClick}>
      {label}
    </button>
  );
}

function ProfileSettings() {
  const [name, setName] = useState('');
  const [timezone, setTimezone] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone);

  return (
    <div className="settings-section">
      <h2>Profile</h2>
      <label>Display name</label>
      <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Your name" />
      <label>Timezone</label>
      <input value={timezone} onChange={(e) => setTimezone(e.target.value)} />
      <button disabled>Save profile</button>
      <p className="hint">Profile persistence coming soon.</p>
    </div>
  );
}

function LlmSettings() {
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
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
      <label>Model</label>
      <input
        value={model}
        onChange={(e) => setModel(e.target.value)}
        placeholder={LLM_PROVIDERS.find((p) => p.id === provider)?.defaultModel}
      />
      <label>API Key</label>
      <input
        type="password"
        value={apiKey}
        onChange={(e) => setApiKey(e.target.value)}
        placeholder="sk-..."
      />
      <label>Base URL (optional)</label>
      <input
        value={baseUrl}
        onChange={(e) => setBaseUrl(e.target.value)}
        placeholder="https://api.openai.com/v1"
      />
      <label>Temperature</label>
      <input
        type="number"
        min="0"
        max="2"
        step="0.1"
        value={temperature}
        onChange={(e) => setTemperature(e.target.value)}
      />
      <button onClick={handleSave}>Save LLM Settings</button>
      {saved && <span className="success-hint">Saved</span>}
    </div>
  );
}

function WorkerSettings() {
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
        <input
          placeholder="ws://host:port/ws"
          value={newUrl}
          onChange={(e) => setNewUrl(e.target.value)}
        />
        <button onClick={handleAddWorker}>Add worker</button>
      </div>

      <div className="settings-subsection">
        <h3>Pair worker</h3>
        <select value={selectedWorker} onChange={(e) => setSelectedWorker(e.target.value)}>
          <option value="">Select worker</option>
          {workers.map((w) => (
            <option key={w.id} value={w.id}>
              {w.name}
            </option>
          ))}
        </select>
        <input
          placeholder="Pairing code"
          value={pairCode}
          onChange={(e) => setPairCode(e.target.value)}
        />
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
        <textarea
          value={sshKey}
          onChange={(e) => setSshKey(e.target.value)}
          placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
          rows={5}
        />
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

function ClusterSettings() {
  const [identity, setIdentity] = useState<ClusterIdentityInfo | null>(null);
  const [clusterName, setClusterName] = useState('');
  const [importKey, setImportKey] = useState('');
  const [importName, setImportName] = useState('');
  const [exportedKey, setExportedKey] = useState('');
  const [exportedBackup, setExportedBackup] = useState('');

  useEffect(() => {
    getClusterIdentity().then(setIdentity);
  }, []);

  async function handleCreate() {
    if (!clusterName) return;
    const info = await createCluster(clusterName);
    setIdentity(info);
    setClusterName('');
  }

  async function handleImport() {
    if (!importKey || !importName) return;
    const info = await importClusterKey(importKey, importName);
    setIdentity(info);
    setImportKey('');
    setImportName('');
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

      {identity ? (
        <div className="settings-subsection">
          <h3>Active cluster</h3>
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
        <p className="hint">No cluster configured yet.</p>
      )}

      <div className="settings-subsection">
        <h3>Create cluster</h3>
        <input
          placeholder="Cluster name"
          value={clusterName}
          onChange={(e) => setClusterName(e.target.value)}
        />
        <button onClick={handleCreate} disabled={!clusterName}>
          Create new cluster
        </button>
      </div>

      <div className="settings-subsection">
        <h3>Import cluster</h3>
        <input
          placeholder="Cluster name"
          value={importName}
          onChange={(e) => setImportName(e.target.value)}
        />
        <textarea
          placeholder="Paste cluster key"
          value={importKey}
          onChange={(e) => setImportKey(e.target.value)}
          rows={3}
        />
        <button onClick={handleImport} disabled={!importKey || !importName}>
          Import cluster key
        </button>
      </div>
    </div>
  );
}

function AppearanceSettings() {
  const [theme, setTheme] = useState<'light' | 'dark'>('dark');

  return (
    <div className="settings-section">
      <h2>Appearance</h2>
      <label>Theme</label>
      <select value={theme} onChange={(e) => setTheme(e.target.value as 'light' | 'dark')}>
        <option value="dark">Dark</option>
        <option value="light">Light</option>
      </select>
      <button disabled>Save appearance</button>
      <p className="hint">Theme persistence coming soon.</p>
    </div>
  );
}
