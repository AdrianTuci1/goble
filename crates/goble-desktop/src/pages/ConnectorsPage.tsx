import { useState } from 'react';
import { useStore } from '../stores/appStore';
import {
  searchMcpServers,
  installMcpServer,
  updateMcpServer,
  deleteMcpServer,
  discoverMcpTools,
  testCallMcpTool,
  listMcpServers,
  updateMcpServerMeta,
} from '../tauri/api';
import type { McpSearchResult, McpServerSummary, VaultSecretInfo } from '../tauri/api';
import { Search, Settings, RefreshCw, Trash2, Puzzle } from 'lucide-react';
import './ConnectorsPage.css';

const SOURCE_OPTIONS = ['npm', 'github', 'local', 'url', 'stdio'];

export default function ConnectorsPage() {
  const servers = useStore((s) => s.mcpServers);
  const vaultSecrets = useStore((s) => s.vaultSecrets);
  const removeMcpServer = useStore((s) => s.removeMcpServer);
  const setMcpServers = useStore((s) => s.setMcpServers);

  const [query, setQuery] = useState('');
  const [searchResults, setSearchResults] = useState<McpSearchResult[]>([]);
  const [searching, setSearching] = useState(false);

  const [form, setForm] = useState({
    id: '',
    name: '',
    source: 'npm',
    source_value: '',
    secret_ids: [] as string[],
  });
  const [editingId, setEditingId] = useState<string | null>(null);
  const [message, setMessage] = useState('');
  const [messageType, setMessageType] = useState<'info' | 'error'>('info');
  const [loading, setLoading] = useState(false);
  const [discovering, setDiscovering] = useState<Record<string, boolean>>({});
  const [drawerServer, setDrawerServer] = useState<McpServerSummary | null>(null);
  const [drawerSecretIds, setDrawerSecretIds] = useState<string[]>([]);
  const [drawerEnabledTools, setDrawerEnabledTools] = useState<Set<string>>(new Set());
  const [savingMeta, setSavingMeta] = useState(false);
  const [discoveringDrawer, setDiscoveringDrawer] = useState(false);

  async function handleSearch() {
    if (!query.trim()) return;
    setSearching(true);
    clearMessage();
    try {
      const results = await searchMcpServers(query.trim());
      setSearchResults(results);
    } catch (e) {
      setError(`Search failed: ${e}`);
    } finally {
      setSearching(false);
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

  function selectSearchResult(result: McpSearchResult) {
    setForm({
      id: result.id,
      name: result.name,
      source: result.source === 'github' ? 'github' : 'npm',
      source_value: result.source === 'github' ? result.name : result.id,
      secret_ids: [],
    });
    setEditingId(null);
    setMessage('');
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

  async function handleInstall(e: React.FormEvent) {
    e.preventDefault();
    const err = validateForm();
    if (err) {
      setError(err);
      return;
    }
    setLoading(true);
    clearMessage();
    try {
      await installMcpServer(form.id.trim(), form.name.trim(), form.source, form.source_value.trim() || undefined, form.secret_ids);
      const updated = await listMcpServers();
      setMcpServers(updated);
      setInfo(`Installed ${form.id.trim()}`);
      setForm({ id: '', name: '', source: 'npm', source_value: '', secret_ids: [] });
    } catch (err) {
      setError(`Install failed: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function handleUpdate(e: React.FormEvent) {
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
      const updated = await listMcpServers();
      setMcpServers(updated);
      setInfo(`Updated ${editingId}`);
      setEditingId(null);
      setForm({ id: '', name: '', source: 'npm', source_value: '', secret_ids: [] });
    } catch (err) {
      setError(`Update failed: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function handleDelete(id: string) {
    if (!confirm(`Delete MCP server ${id}?`)) return;
    clearMessage();
    try {
      await deleteMcpServer(id);
      removeMcpServer(id);
      setInfo(`Deleted ${id}`);
    } catch (err) {
      setError(`Delete failed: ${err}`);
    }
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
    clearMessage();
  }

  function cancelEdit() {
    setEditingId(null);
    setForm({ id: '', name: '', source: 'npm', source_value: '', secret_ids: [] });
    clearMessage();
  }

  async function handleDiscover(server: McpServerSummary) {
    setDiscovering((prev) => ({ ...prev, [server.id]: true }));
    clearMessage();
    try {
      const tools = await discoverMcpTools(server.id);
      const updated = await listMcpServers();
      setMcpServers(updated);
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
    setDrawerEnabledTools(new Set(server.enabled_tools || server.discovered_tools || []));
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
      const updated = await listMcpServers();
      setMcpServers(updated);
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
      const updated = await listMcpServers();
      setMcpServers(updated);
      setInfo(`Saved settings for ${drawerServer.id}`);
      closeDrawer();
    } catch (err) {
      setError(`Save failed for ${drawerServer.id}: ${err}`);
    } finally {
      setSavingMeta(false);
    }
  }

  return (
    <div className="mcp-page">
      <div className="mcp-header">
        <h1>MCP Connectors</h1>
        <p className="mcp-header-description">
          Extend Goble with Model Context Protocol servers. Search the registry or add a custom
          server below.
        </p>
      </div>

      <div className="mcp-scroll">
        {message && <div className={`mcp-message ${messageType}`}>{message}</div>}

        <div className="mcp-search-row">
          <div className="mcp-search-input">
            <Search size={16} />
            <input
              placeholder="postgres, filesystem, slack..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
            />
          </div>
          <button
            className="mcp-add-btn"
            onClick={handleSearch}
            disabled={searching || !query.trim()}
          >
            {searching ? 'Searching…' : 'Search'}
          </button>
        </div>

        {searchResults.length > 0 && (
          <>
            <h2 className="mcp-section-title">Search results</h2>
            <div className="mcp-grid">
              {searchResults.map((result) => (
                <div className="mcp-card" key={result.id}>
                  <div className="mcp-card-icon">
                    {result.name.slice(0, 1).toUpperCase()}
                  </div>
                  <div className="mcp-card-body">
                    <h3 className="mcp-card-title">{result.name}</h3>
                    <p className="mcp-card-description">{result.description}</p>
                    <div className="mcp-card-actions">
                      <button
                        className="mcp-card-btn primary"
                        onClick={() => selectSearchResult(result)}
                      >
                        Use
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </>
        )}

        <div className="mcp-card">
          <div className="mcp-card-body">
            <h3 className="mcp-card-title">
              {editingId ? `Update ${editingId}` : 'Install MCP server'}
            </h3>
            <form onSubmit={editingId ? handleUpdate : handleInstall}>
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
              <div className="mcp-card-actions" style={{ marginTop: 8 }}>
                <button
                  type="submit"
                  className="mcp-card-btn primary"
                  disabled={loading}
                >
                  {loading
                    ? editingId
                      ? 'Updating…'
                      : 'Installing…'
                    : editingId
                      ? 'Update'
                      : 'Install'}
                </button>
                {editingId && (
                  <button
                    type="button"
                    className="mcp-card-btn"
                    onClick={cancelEdit}
                    disabled={loading}
                  >
                    Cancel
                  </button>
                )}
              </div>
            </form>
          </div>
        </div>

        <h2 className="mcp-section-title">Installed MCP servers</h2>
        {servers.length === 0 ? (
          <div className="mcp-empty-state">
            <Puzzle size={32} />
            <h3>No MCP servers installed yet</h3>
            <p>Use the form above to add a server.</p>
          </div>
        ) : (
          <div className="mcp-installed-grid">
            {servers.map((server) => (
              <div className="mcp-installed-card" key={server.id}>
                <div className="mcp-installed-card-header">
                  <div className="mcp-card-icon">
                    {server.name.slice(0, 1).toUpperCase()}
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
                    disabled={!!discovering[server.id]}
                  >
                    <RefreshCw size={14} />{' '}
                    {discovering[server.id] ? 'Discovering…' : 'Discover'}
                  </button>
                  <button className="mcp-card-btn" onClick={() => startEdit(server)}>
                    Edit
                  </button>
                  <button
                    className="mcp-card-btn"
                    onClick={() => handleDelete(server.id)}
                  >
                    <Trash2 size={14} /> Delete
                  </button>
                </div>
              </div>
            ))}
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
          />
        )}
      </div>
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
    new Set([...server.discovered_tools, ...server.enabled_tools, ...enabledTools])
  );

  return (
    <div className="mcp-drawer-backdrop" onClick={onClose}>
      <div className="mcp-drawer" onClick={(e) => e.stopPropagation()}>
        <div className="mcp-drawer-header">
          <h3>{server.name}</h3>
          <button onClick={onClose} aria-label="Close">
            ×
          </button>
        </div>
        <div className="mcp-drawer-body">
          <div className="mcp-drawer-section">
            <div className="mcp-drawer-section-title">Vault secrets</div>
            {vaultSecrets.length === 0 && (
              <p className="mcp-empty">No secrets in vault. Add them in the Vault page.</p>
            )}
            <div className="mcp-drawer-list">
              {vaultSecrets.map((s) => (
                <label key={s.key} className="mcp-drawer-row">
                  <input
                    type="checkbox"
                    checked={secretIds.includes(s.key)}
                    onChange={() => onToggleSecret(s.key)}
                  />
                  <span>{s.key}</span>
                </label>
              ))}
            </div>
          </div>

          <div className="mcp-drawer-section">
            <div
              className="mcp-drawer-section-title"
              style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}
            >
              <span>Enabled tools</span>
              <button
                className="mcp-card-btn"
                onClick={onDiscover}
                disabled={discovering}
              >
                {discovering ? 'Discovering…' : 'Discover'}
              </button>
            </div>
            {availableTools.length === 0 && (
              <p className="mcp-empty">No tools discovered yet. Click Discover to fetch them.</p>
            )}
            <div className="mcp-drawer-list">
              {availableTools.map((t) => (
                <div key={t} className="mcp-drawer-row">
                  <label>
                    <input
                      type="checkbox"
                      checked={enabledTools.has(t)}
                      onChange={() => onToggleTool(t)}
                    />
                    <span>{t}</span>
                  </label>
                  <button
                    className="mcp-card-btn"
                    onClick={() => setTestTool(t)}
                    disabled={testing}
                  >
                    Test
                  </button>
                </div>
              ))}
            </div>
            {testTool && (
              <div className="mcp-test-area">
                <div>
                  Test tool: <strong>{testTool}</strong>
                </div>
                <textarea
                  rows={3}
                  value={testArgs}
                  onChange={(e) => setTestArgs(e.target.value)}
                  placeholder="Tool arguments as JSON"
                />
                <button
                  className="mcp-card-btn"
                  onClick={() => runTest(testTool)}
                  disabled={testing}
                >
                  {testing ? 'Running…' : 'Run test'}
                </button>
                {testResult && <pre className="mcp-test-result">{testResult}</pre>}
              </div>
            )}
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
