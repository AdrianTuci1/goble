import { useState, useEffect } from 'react';
import { useStore } from '../stores/appStore';
import {
  searchMcpServers,
  installMcpServer,
  updateMcpServer,
  deleteMcpServer,
  discoverMcpTools,
  testCallMcpTool,
  listMcpServers,
  listVaultSecrets,
  updateMcpServerMeta,
  setVaultSecret,
} from '../shared/tauri/api';
import type { McpSearchResult, McpServerSummary, VaultSecretInfo } from '../shared/tauri/api';
import { Search, Plus, X, Key, Trash2, RefreshCw, Settings, Puzzle } from 'lucide-react';
import './Pages.css';
import './ConnectorsPage.css';

interface McpPreset {
  id: string;
  name: string;
  description: string;
  source: 'npm' | 'github' | 'local' | 'url' | 'stdio';
  sourceValue: string;
  icon: string;
  color: string;
  authRequired: boolean;
}

const PRESETS: McpPreset[] = [
  {
    id: 'mcp-postgres',
    name: 'PostgreSQL',
    description: 'Query schemas and run SQL against PostgreSQL databases.',
    source: 'npm',
    sourceValue: '@modelcontextprotocol/server-postgres',
    icon: 'P',
    color: '#336791',
    authRequired: true,
  },
  {
    id: 'mcp-filesystem',
    name: 'Filesystem',
    description: 'Read and write files inside allowed paths.',
    source: 'npm',
    sourceValue: '@modelcontextprotocol/server-filesystem',
    icon: 'F',
    color: '#f59e0b',
    authRequired: true,
  },
  {
    id: 'mcp-sequential-thinking',
    name: 'Sequential Thinking',
    description: 'Structured reasoning chain for the agent.',
    source: 'npm',
    sourceValue: '@modelcontextprotocol/server-sequential-thinking',
    icon: 'S',
    color: '#8b5cf6',
    authRequired: false,
  },
  {
    id: 'mcp-composio',
    name: 'Composio',
    description: 'Connect your agent to 1000+ apps like Gmail, Slack, GitHub, and Linear.',
    source: 'npm',
    sourceValue: 'composio-mcp',
    icon: 'C',
    color: '#7c3aed',
    authRequired: true,
  },
  {
    id: 'mcp-context7',
    name: 'Context7',
    description: 'Fetch up-to-date documentation and code examples.',
    source: 'npm',
    sourceValue: '@context7/mcp',
    icon: '7',
    color: '#10b981',
    authRequired: false,
  },
  {
    id: 'mcp-datadog',
    name: 'Datadog',
    description: 'Monitor and analyze application performance.',
    source: 'npm',
    sourceValue: 'datadog-mcp',
    icon: 'D',
    color: '#632ca6',
    authRequired: true,
  },
  {
    id: 'mcp-figma',
    name: 'Figma',
    description: 'Read Figma designs and components.',
    source: 'npm',
    sourceValue: 'figma-mcp',
    icon: 'F',
    color: '#a259ff',
    authRequired: true,
  },
  {
    id: 'mcp-github',
    name: 'GitHub',
    description: 'Manage issues, projects and code.',
    source: 'npm',
    sourceValue: '@modelcontextprotocol/server-github',
    icon: 'G',
    color: '#24292f',
    authRequired: true,
  },
  {
    id: 'mcp-granola',
    name: 'Granola',
    description: 'Access meeting notes and transcripts.',
    source: 'npm',
    sourceValue: 'granola-mcp',
    icon: 'G',
    color: '#d97706',
    authRequired: true,
  },
  {
    id: 'mcp-linear',
    name: 'Linear',
    description: 'Project management tools and issue tracking.',
    source: 'npm',
    sourceValue: 'linear-mcp',
    icon: 'L',
    color: '#5e6ad2',
    authRequired: true,
  },
  {
    id: 'mcp-notion',
    name: 'Notion',
    description: 'Retrieve documentation and pages from Notion.',
    source: 'npm',
    sourceValue: 'notion-mcp',
    icon: 'N',
    color: '#000000',
    authRequired: true,
  },
  {
    id: 'mcp-playwright',
    name: 'Playwright',
    description: 'Browser automation for web scraping and testing.',
    source: 'npm',
    sourceValue: '@modelcontextprotocol/server-playwright',
    icon: 'P',
    color: '#2ead6a',
    authRequired: false,
  },
];

const SOURCE_OPTIONS = ['npm', 'github', 'local', 'url', 'stdio'];

export default function ConnectorsPage() {
  const servers = useStore((s) => s.mcpServers);
  const vaultSecrets = useStore((s) => s.vaultSecrets);
  const removeMcpServer = useStore((s) => s.removeMcpServer);
  const setMcpServers = useStore((s) => s.setMcpServers);
  const setVaultSecrets = useStore((s) => s.setVaultSecrets);

  const [query, setQuery] = useState('');
  const [searchResults, setSearchResults] = useState<McpSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [autoSpawn, setAutoSpawn] = useState(false);
  const [showAddModal, setShowAddModal] = useState(false);
  const [drawerServer, setDrawerServer] = useState<McpServerSummary | null>(null);
  const [message, setMessage] = useState('');
  const [messageType, setMessageType] = useState<'info' | 'error'>('info');
  const [loading, setLoading] = useState(false);
  const [discovering, setDiscovering] = useState<Record<string, boolean>>({});
  const [installingPreset, setInstallingPreset] = useState<string | null>(null);

  const [form, setForm] = useState({
    id: '',
    name: '',
    source: 'npm',
    source_value: '',
    secret_ids: [] as string[],
  });
  const [editingId, setEditingId] = useState<string | null>(null);
  const [credentialPreset, setCredentialPreset] = useState<McpPreset | null>(null);
  const [credentialFields, setCredentialFields] = useState<Record<string, string>>({});
  const [savingCredentials, setSavingCredentials] = useState(false);

  const [drawerSecretIds, setDrawerSecretIds] = useState<string[]>([]);
  const [drawerEnabledTools, setDrawerEnabledTools] = useState<Set<string>>(new Set());
  const [savingMeta, setSavingMeta] = useState(false);
  const [discoveringDrawer, setDiscoveringDrawer] = useState(false);

  useEffect(() => {
    refreshServers();
  }, []);

  async function refreshServers() {
    try {
      const updated = await listMcpServers();
      setMcpServers(updated);
    } catch (e) {
      setError(`Failed to refresh servers: ${e}`);
    }
  }

  async function refreshVaultSecrets() {
    try {
      const secrets = await listVaultSecrets();
      setVaultSecrets(secrets);
    } catch (e) {
      setError(`Failed to refresh vault secrets: ${e}`);
    }
  }

  function setInfo(text: string) {
    setMessage(text);
    setMessageType('info');
  }

  function setError(text: string) {
    setMessage(text);
    setMessageType('error');
  }

  function clearMessage() {
    setMessage('');
  }

  async function handleSearch() {
    console.log('[handleSearch]', query);
    if (!query.trim()) return;
    setSearching(true);
    clearMessage();
    try {
      const results = await searchMcpServers(query.trim());
      console.log('[handleSearch] results', results);
      setSearchResults(results);
    } catch (e) {
      console.error('[handleSearch] error', e);
      setError(`Search failed: ${e}`);
    } finally {
      setSearching(false);
    }
  }

  async function installPreset(preset: McpPreset) {
    if (preset.authRequired) {
      setCredentialPreset(preset);
      setCredentialFields({});
      return;
    }
    await doInstall(preset.id, preset.name, preset.source, preset.sourceValue, []);
  }

  async function doInstall(
    id: string,
    name: string,
    source: string,
    sourceValue: string,
    secretIds: string[],
  ) {
    try {
      setInstallingPreset(id);
      clearMessage();
      await installMcpServer(id.trim(), name.trim(), source, sourceValue.trim() || undefined, secretIds);
      await refreshServers();
      setInfo(`Installed ${id}`);
    } catch (e) {
      setError(`Install failed: ${e}`);
    } finally {
      setInstallingPreset(null);
    }
  }

  async function handleSaveCredentials() {
    if (!credentialPreset) return;
    const entries = Object.entries(credentialFields).filter(([, v]) => v.trim());
    if (entries.length === 0) {
      setError('Please fill in the required credentials.');
      return;
    }
    setSavingCredentials(true);
    clearMessage();
    try {
      const secretIds: string[] = [];
      for (const [key, value] of entries) {
        const secretName = `${credentialPreset.id}-${key}`;
        await setVaultSecret(secretName, value.trim());
        secretIds.push(secretName);
      }
      await refreshVaultSecrets();
      await doInstall(
        credentialPreset.id,
        credentialPreset.name,
        credentialPreset.source,
        credentialPreset.sourceValue,
        secretIds,
      );
      setCredentialPreset(null);
      setCredentialFields({});
    } catch (e) {
      setError(`Failed to save credentials: ${e}`);
    } finally {
      setSavingCredentials(false);
    }
  }

  async function handleInstallCustom(e: React.FormEvent) {
    e.preventDefault();
    const err = validateForm();
    if (err) {
      setError(err);
      return;
    }
    setLoading(true);
    clearMessage();
    try {
      await installMcpServer(
        form.id.trim(),
        form.name.trim(),
        form.source,
        form.source_value.trim() || undefined,
        form.secret_ids,
      );
      await refreshServers();
      setInfo(`Installed ${form.id.trim()}`);
      setShowAddModal(false);
      setForm({ id: '', name: '', source: 'npm', source_value: '', secret_ids: [] });
      setEditingId(null);
    } catch (err) {
      setError(`Install failed: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function handleUpdateCustom(e: React.FormEvent) {
    e.preventDefault();
    if (!editingId) return;
    if (!form.name.trim()) {
      setError('Display name is required.');
      return;
    }
    setLoading(true);
    clearMessage();
    try {
      await updateMcpServer(editingId, form.name.trim(), form.source_value.trim() || undefined, form.secret_ids);
      await refreshServers();
      setInfo(`Updated ${editingId}`);
      setShowAddModal(false);
      setEditingId(null);
      setForm({ id: '', name: '', source: 'npm', source_value: '', secret_ids: [] });
    } catch (err) {
      setError(`Update failed: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  function validateForm(): string | null {
    if (!form.id.trim() || !/^[a-z0-9_-]+$/.test(form.id.trim())) {
      return 'ID must be a lowercase slug (letters, numbers, underscores, hyphens).';
    }
    if (!form.name.trim()) return 'Display name is required.';
    if (['local', 'url', 'stdio'].includes(form.source) && !form.source_value.trim()) {
      return 'Package / path / URL is required for this source.';
    }
    return null;
  }

  async function handleDelete(id: string) {
    if (!confirm(`Delete MCP server ${id}?`)) return;
    clearMessage();
    try {
      await deleteMcpServer(id);
      removeMcpServer(id);
      if (drawerServer?.id === id) setDrawerServer(null);
      setInfo(`Deleted ${id}`);
    } catch (err) {
      setError(`Delete failed: ${err}`);
    }
  }

  async function handleDeleteFromDrawer(id: string) {
    await handleDelete(id);
  }

  function startEdit(server: McpServerSummary) {
    setEditingId(server.id);
    setForm({
      id: server.id,
      name: server.name,
      source: server.source as typeof form.source,
      source_value: server.source_value || '',
      secret_ids: server.secret_ids || [],
    });
    setShowAddModal(true);
    clearMessage();
  }

  function openAddModal() {
    setEditingId(null);
    setForm({ id: '', name: '', source: 'npm', source_value: '', secret_ids: [] });
    setShowAddModal(true);
    clearMessage();
  }

  async function handleDiscover(server: McpServerSummary) {
    setDiscovering((prev) => ({ ...prev, [server.id]: true }));
    clearMessage();
    try {
      const tools = await discoverMcpTools(server.id);
      await refreshServers();
      setInfo(`Discovered ${tools.length} tools for ${server.id}`);
    } catch (err) {
      setError(`Discover failed for ${server.id}: ${err}`);
    } finally {
      setDiscovering((prev) => ({ ...prev, [server.id]: false }));
    }
  }

  function openDrawer(server: McpServerSummary) {
    setDrawerServer(server);
    setDrawerSecretIds(server.secret_ids || []);
    const enabled = server.enabled_tools.length > 0 ? server.enabled_tools : server.discovered_tools;
    setDrawerEnabledTools(new Set(enabled || []));
  }

  function closeDrawer() {
    setDrawerServer(null);
    setDrawerSecretIds([]);
    setDrawerEnabledTools(new Set());
  }

  function toggleSecret(secretKey: string) {
    setDrawerSecretIds((prev) =>
      prev.includes(secretKey) ? prev.filter((k) => k !== secretKey) : [...prev, secretKey]
    );
  }

  function toggleTool(toolName: string) {
    setDrawerEnabledTools((prev) => {
      const next = new Set(prev);
      if (next.has(toolName)) next.delete(toolName);
      else next.add(toolName);
      return next;
    });
  }

  async function handleDiscoverDrawer() {
    if (!drawerServer) return;
    setDiscoveringDrawer(true);
    clearMessage();
    try {
      const tools = await discoverMcpTools(drawerServer.id);
      await refreshServers();
      setDrawerEnabledTools(new Set(tools.map((t) => t.name)));
      setInfo(`Discovered ${tools.length} tools for ${drawerServer.id}`);
    } catch (err) {
      setError(`Discover failed for ${drawerServer.id}: ${err}`);
    } finally {
      setDiscoveringDrawer(false);
    }
  }

  async function handleSaveMeta() {
    if (!drawerServer) return;
    setSavingMeta(true);
    clearMessage();
    try {
      await updateMcpServerMeta(drawerServer.id, drawerSecretIds, Array.from(drawerEnabledTools));
      await refreshServers();
      setInfo(`Saved settings for ${drawerServer.id}`);
      closeDrawer();
    } catch (err) {
      setError(`Save failed for ${drawerServer.id}: ${err}`);
    } finally {
      setSavingMeta(false);
    }
  }

  const installedIds = new Set(servers.map((s) => s.id));
  const displayPresets = query.trim()
    ? PRESETS.filter(
        (p) =>
          p.name.toLowerCase().includes(query.toLowerCase()) ||
          p.description.toLowerCase().includes(query.toLowerCase())
      )
    : PRESETS;

  return (
    <div className="mcp-page">
      <header className="mcp-header">
        <h1>MCP Servers</h1>
        <p className="mcp-header-description">
          Add MCP servers to extend Goble&apos;s capabilities. MCP servers expose data sources or
          tools to agents through a standardized interface, essentially acting like plugins. Add a
          custom server, or use the presets to get started with popular servers.{' '}
          <a
            href="https://modelcontextprotocol.io/introduction"
            target="_blank"
            rel="noreferrer"
          >
            Learn more
          </a>
          .
        </p>
      </header>

      <div className="mcp-scroll">
        {message && <div className={`mcp-message ${messageType}`}>{message}</div>}

        <div className="mcp-search-row">
          <div className="mcp-search-input">
            <Search size={18} />
            <input
              placeholder="Search MCP Servers"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
              disabled={searching}
              data-testid="mcp-search-input"
            />
          </div>
          <button className="mcp-add-btn" onClick={openAddModal}>
            <Plus size={18} />
            Add
          </button>
          <button
            className="mcp-search-btn"
            onClick={() => handleSearch()}
            disabled={searching}
            data-testid="mcp-search-button"
          >
            <Search size={18} />
            Search
          </button>
        </div>

        {searchResults.length > 0 && (
          <div className="mcp-section">
            <h2 className="mcp-section-title">Search results</h2>
            <div className="mcp-grid">
              {searchResults.map((result) => (
                <div className="mcp-card" key={result.id}>
                  <div className="mcp-card-icon" style={{ background: '#64748b' }}>
                    {result.name.slice(0, 1)}
                  </div>
                  <div className="mcp-card-body">
                    <h3 className="mcp-card-title">{result.name}</h3>
                    <p className="mcp-card-description">{result.description}</p>
                    <div className="mcp-card-actions">
                      <button
                        className="mcp-card-btn primary"
                        onClick={() =>
                          doInstall(result.id, result.name, result.source, result.id, [])
                        }
                        disabled={installingPreset === result.id}
                      >
                        {installingPreset === result.id ? 'Installing…' : 'Install'}
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        <div className="mcp-auto-spawn">
          <div className="mcp-auto-spawn-text">
            <h3>Auto-spawn servers from third-party agents</h3>
            <p>
              Automatically detect and spawn MCP servers from globally-scoped third-party AI agent
              configuration files. Servers detected inside a repository are never spawned
              automatically and must be enabled individually.{' '}
              <a
                href="https://modelcontextprotocol.io/introduction"
                target="_blank"
                rel="noreferrer"
              >
                See supported providers
              </a>
              .
            </p>
          </div>
          <button
            className={`mcp-toggle ${autoSpawn ? 'on' : ''}`}
            onClick={() => setAutoSpawn((v) => !v)}
            aria-label="Toggle auto-spawn"
          >
            <span className="mcp-toggle-knob" />
          </button>
        </div>

        <h2 className="mcp-section-title">Preset servers</h2>
        <div className="mcp-grid">
          {displayPresets.map((preset) => {
            const isInstalled = installedIds.has(preset.id);
            return (
              <div className="mcp-card" key={preset.id}>
                <button
                  className="mcp-card-add"
                  onClick={() => installPreset(preset)}
                  disabled={isInstalled || installingPreset === preset.id}
                  aria-label={isInstalled ? 'Installed' : `Add ${preset.name}`}
                  title={isInstalled ? 'Installed' : `Add ${preset.name}`}
                >
                  {isInstalled ? <Settings size={14} /> : <Plus size={14} />}
                </button>
                <div className="mcp-card-icon" style={{ background: preset.color }}>
                  {preset.icon}
                </div>
                <div className="mcp-card-body">
                  <h3 className="mcp-card-title">
                    {preset.name}
                    {preset.authRequired && (
                      <Key size={12} style={{ marginLeft: 6, verticalAlign: 'middle' }} />
                    )}
                  </h3>
                  <p className="mcp-card-description">{preset.description}</p>
                  <div className="mcp-card-actions">
                    {isInstalled ? (
                      <button
                        className="mcp-card-btn"
                        onClick={() => {
                          const server = servers.find((s) => s.id === preset.id);
                          if (server) openDrawer(server);
                        }}
                      >
                        Manage
                      </button>
                    ) : (
                      <button
                        className="mcp-card-btn primary"
                        onClick={() => installPreset(preset)}
                        disabled={installingPreset === preset.id}
                      >
                        {installingPreset === preset.id ? 'Installing…' : 'Install'}
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
        </div>

        <h2 className="mcp-section-title">Installed MCP servers</h2>
        {servers.length === 0 ? (
          <div className="mcp-empty-state">
            <Puzzle size={32} />
            <h3>No MCP servers installed yet</h3>
            <p>Pick a preset from above or add a custom server to start extending Goble.</p>
          </div>
        ) : (
          <div className="mcp-installed-grid">
            {servers.map((server) => (
              <div className="mcp-installed-card" key={server.id}>
                <div className="mcp-installed-card-header">
                  <div className="mcp-card-icon" style={{ background: '#64748b' }}>
                    {server.name.slice(0, 1)}
                  </div>
                  <div className="mcp-card-body">
                    <h3 className="mcp-card-title">{server.name}</h3>
                    <p className="mcp-card-description">{server.id}</p>
                    <div className="mcp-installed-card-meta">
                      <span className="mcp-tag">{server.source}</span>
                      {server.capabilities.map((c) => (
                        <span className="mcp-tag" key={c}>
                          {c}
                        </span>
                      ))}
                      {server.auth_required && <span className="mcp-tag">auth</span>}
                    </div>
                  </div>
                </div>
                <div className="mcp-installed-card-actions">
                  <button className="mcp-card-btn" onClick={() => openDrawer(server)}>
                    <Settings size={14} /> Manage
                  </button>
                  <button
                    className="mcp-card-btn"
                    onClick={() => handleDiscover(server)}
                    disabled={discovering[server.id]}
                  >
                    <RefreshCw size={14} /> {discovering[server.id] ? 'Discovering…' : 'Discover'}
                  </button>
                  <button className="mcp-card-btn" onClick={() => startEdit(server)}>
                    Edit
                  </button>
                  <button
                    className="mcp-card-btn"
                    style={{ color: '#ef4444', borderColor: 'rgba(239, 68, 68, 0.3)' }}
                    onClick={() => handleDelete(server.id)}
                    data-testid="delete-mcp-button"
                  >
                    <Trash2 size={14} /> Delete
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {showAddModal && (
        <div className="mcp-modal-overlay" onClick={() => setShowAddModal(false)}>
          <div className="mcp-modal" onClick={(e) => e.stopPropagation()}>
            <div className="mcp-modal-header">
              <h3>{editingId ? `Update ${editingId}` : 'Add custom MCP server'}</h3>
              <button
                className="mcp-modal-close"
                onClick={() => setShowAddModal(false)}
                aria-label="Close"
              >
                <X size={18} />
              </button>
            </div>
            <form onSubmit={editingId ? handleUpdateCustom : handleInstallCustom}>
              <div className="mcp-modal-body">
                <div className="mcp-modal-field">
                  <label>ID</label>
                  <input
                    placeholder="e.g. mcp-postgres"
                    value={form.id}
                    onChange={(e) => setForm({ ...form, id: e.target.value })}
                    disabled={!!editingId}
                    required
                  />
                  <p className="mcp-modal-hint">Lowercase slug used as the server identifier.</p>
                </div>
                <div className="mcp-modal-field">
                  <label>Display name</label>
                  <input
                    placeholder="e.g. PostgreSQL"
                    value={form.name}
                    onChange={(e) => setForm({ ...form, name: e.target.value })}
                    required
                  />
                </div>
                <div className="mcp-modal-field">
                  <label>Source</label>
                  <select
                    value={form.source}
                    onChange={(e) => setForm({ ...form, source: e.target.value })}
                  >
                    {SOURCE_OPTIONS.map((s) => (
                      <option key={s} value={s}>
                        {s}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="mcp-modal-field">
                  <label>Package / owner/repo / path / URL</label>
                  <input
                    placeholder="@modelcontextprotocol/server-postgres"
                    value={form.source_value}
                    onChange={(e) => setForm({ ...form, source_value: e.target.value })}
                  />
                  <p className="mcp-modal-hint">
                    Required for local, url and stdio sources; used as the npm package or repo for
                    npm/github.
                  </p>
                </div>
                {vaultSecrets.length > 0 && (
                  <div className="mcp-modal-field">
                    <label>Vault secrets linked to this server</label>
                    <div className="mcp-secret-list">
                      {vaultSecrets.map((secret) => (
                        <label className="mcp-secret-item" key={secret.key}>
                          <input
                            type="checkbox"
                            checked={form.secret_ids.includes(secret.key)}
                            onChange={(e) => {
                              const ids = new Set(form.secret_ids);
                              if (e.target.checked) ids.add(secret.key);
                              else ids.delete(secret.key);
                              setForm({ ...form, secret_ids: Array.from(ids) });
                            }}
                          />
                          {secret.key}
                        </label>
                      ))}
                    </div>
                  </div>
                )}
              </div>
              <div className="mcp-modal-footer">
                <button
                  type="button"
                  className="mcp-modal-btn"
                  onClick={() => setShowAddModal(false)}
                  disabled={loading}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  className="mcp-modal-btn primary"
                  disabled={loading}
                  data-testid="mcp-modal-install"
                >
                  {loading
                    ? editingId
                      ? 'Updating…'
                      : 'Installing…'
                    : editingId
                      ? 'Update'
                      : 'Install'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {credentialPreset && (
        <div className="mcp-modal-overlay" onClick={() => setCredentialPreset(null)}>
          <div className="mcp-modal" onClick={(e) => e.stopPropagation()}>
            <div className="mcp-modal-header">
              <h3>Set credentials for {credentialPreset.name}</h3>
              <button
                className="mcp-modal-close"
                onClick={() => setCredentialPreset(null)}
                aria-label="Close"
              >
                <X size={18} />
              </button>
            </div>
            <div className="mcp-modal-body">
              <p className="mcp-modal-hint">
                {credentialPreset.name} requires credentials. They will be stored in the vault and
                linked to the server.
              </p>
              <div className="mcp-modal-field">
                <label>API key / token</label>
                <input
                  type="password"
                  placeholder="secret value"
                  value={credentialFields.token || ''}
                  onChange={(e) =>
                    setCredentialFields({ ...credentialFields, token: e.target.value })
                  }
                />
              </div>
              <div className="mcp-modal-field">
                <label>Additional value (optional)</label>
                <input
                  placeholder="e.g. base URL, database URL"
                  value={credentialFields.extra || ''}
                  onChange={(e) =>
                    setCredentialFields({ ...credentialFields, extra: e.target.value })
                  }
                />
              </div>
            </div>
            <div className="mcp-modal-footer">
              <button className="mcp-modal-btn" onClick={() => setCredentialPreset(null)}>
                Cancel
              </button>
              <button
                className="mcp-modal-btn primary"
                onClick={handleSaveCredentials}
                disabled={savingCredentials}
              >
                {savingCredentials ? 'Saving…' : 'Install with credentials'}
              </button>
            </div>
          </div>
        </div>
      )}

      {drawerServer && (
        <McpServerDrawer
          server={drawerServer}
          vaultSecrets={vaultSecrets}
          secretIds={drawerSecretIds}
          enabledTools={drawerEnabledTools}
          saving={savingMeta}
          discovering={discoveringDrawer}
          onToggleSecret={toggleSecret}
          onToggleTool={toggleTool}
          onDiscover={handleDiscoverDrawer}
          onSave={handleSaveMeta}
          onClose={closeDrawer}
          onDelete={handleDeleteFromDrawer}
        />
      )}
    </div>
  );
}

function McpServerDrawer({
  server,
  vaultSecrets,
  secretIds,
  enabledTools,
  saving,
  discovering,
  onToggleSecret,
  onToggleTool,
  onDiscover,
  onSave,
  onClose,
  onDelete,
}: {
  server: McpServerSummary;
  vaultSecrets: VaultSecretInfo[];
  secretIds: string[];
  enabledTools: Set<string>;
  saving: boolean;
  discovering: boolean;
  onToggleSecret: (key: string) => void;
  onToggleTool: (name: string) => void;
  onDiscover: () => void;
  onSave: () => void;
  onClose: () => void;
  onDelete: (id: string) => void;
}) {
  const [testTool, setTestTool] = useState('');
  const [testArgs, setTestArgs] = useState('{}');
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  async function runTest(toolName: string) {
    setTesting(true);
    setTestResult(null);
    try {
      const args = JSON.parse(testArgs || '{}');
      const result = await testCallMcpTool(server.id, toolName, args);
      setTestResult(JSON.stringify(result, null, 2));
    } catch (err) {
      setTestResult(`Error: ${err}`);
    } finally {
      setTesting(false);
    }
  }

  const availableTools = Array.from(
    new Set([...server.discovered_tools, ...server.enabled_tools, ...enabledTools]),
  );

  return (
    <div className="mcp-drawer-backdrop" onClick={onClose}>
      <div className="mcp-drawer" onClick={(e) => e.stopPropagation()}>
        <div className="mcp-drawer-header">
          <h3>{server.name}</h3>
          <button onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        <div className="mcp-drawer-body">
          <div className="mcp-drawer-section">
            <div className="mcp-drawer-section-title">Vault secrets</div>
            {vaultSecrets.length === 0 && (
              <p className="mcp-empty">No secrets in vault. Add them in the Vault settings.</p>
            )}
            <div className="mcp-drawer-list">
              {vaultSecrets.map((s) => (
                <div className="mcp-drawer-row" key={s.key}>
                  <label>
                    <input
                      type="checkbox"
                      checked={secretIds.includes(s.key)}
                      onChange={() => onToggleSecret(s.key)}
                    />
                    {s.key}
                  </label>
                </div>
              ))}
            </div>
          </div>

          <div className="mcp-drawer-section">
            <div className="mcp-drawer-section-title" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span>Enabled tools</span>
              <button className="mcp-card-btn" onClick={onDiscover} disabled={discovering} data-testid="mcp-drawer-discover">
                <RefreshCw size={14} />
                {discovering ? 'Discovering…' : 'Discover'}
              </button>
            </div>
            {availableTools.length === 0 && (
              <p className="mcp-empty">No tools discovered yet. Click Discover to fetch them.</p>
            )}
            <div className="mcp-drawer-list">
              {availableTools.map((t) => (
                <div className="mcp-drawer-row" key={t}>
                  <label>
                    <input
                      type="checkbox"
                      checked={enabledTools.has(t)}
                      onChange={() => onToggleTool(t)}
                    />
                    {t}
                  </label>
                  <button className="mcp-card-btn" onClick={() => setTestTool(t)} disabled={testing}>
                    Test
                  </button>
                </div>
              ))}
            </div>
            {testTool && (
              <div className="mcp-test-area">
                <div className="mcp-modal-field">
                  <label>Test {testTool}</label>
                  <textarea
                    rows={3}
                    value={testArgs}
                    onChange={(e) => setTestArgs(e.target.value)}
                    placeholder='Tool arguments as JSON'
                  />
                </div>
                <button className="mcp-card-btn" onClick={() => runTest(testTool)} disabled={testing}>
                  {testing ? 'Running…' : 'Run test'}
                </button>
                {testResult && <pre className="mcp-test-result">{testResult}</pre>}
              </div>
            )}
          </div>
          <div className="mcp-drawer-section mcp-drawer-actions">
            <button
              className="mcp-card-btn"
              onClick={() => { onClose(); onDelete(server.id); }}
              style={{ color: '#ef4444', borderColor: 'rgba(239, 68, 68, 0.3)' }}
              data-testid="delete-mcp-drawer-button"
            >
              <Trash2 size={14} /> Delete server
            </button>
          </div>
        </div>
        <div className="mcp-drawer-footer">
          <button className="primary" onClick={onSave} disabled={saving}>
            {saving ? 'Saving…' : 'Save'}
          </button>
          <button onClick={onClose} disabled={saving}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
