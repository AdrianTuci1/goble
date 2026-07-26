import { useState } from 'react';
import { useStore } from '../stores/appStore';
import {
  searchMcpServers,
  installMcpServer,
  updateMcpServer,
  deleteMcpServer,
  discoverMcpTools,
  listMcpServers,
  updateMcpServerMeta,
} from '../tauri/api';
import type { McpSearchResult, McpServerSummary, VaultSecretInfo } from '../tauri/api';

const SOURCE_OPTIONS = ['npm', 'github', 'local', 'url', 'stdio'];

export default function ConnectorsPage() {
  const servers = useStore((s) => s.mcpServers);
  const vaultSecrets = useStore((s) => s.vaultSecrets);
  const removeMcpServer = useStore((s) => s.removeMcpServer);
  const setMcpServers = useStore((s) => s.setMcpServers);

  const [query, setQuery] = useState('');
  const [searchResults, setSearchResults] = useState<McpSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [favorites, setFavorites] = useState<Set<string>>(new Set());

  const [form, setForm] = useState({
    id: '',
    name: '',
    source: 'npm',
    source_value: '',
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
      await installMcpServer(form.id.trim(), form.name.trim(), form.source, form.source_value.trim() || undefined);
      const updated = await listMcpServers();
      setMcpServers(updated);
      setInfo(`Installed ${form.id.trim()}`);
      setForm({ id: '', name: '', source: 'npm', source_value: '' });
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
      await updateMcpServer(editingId, form.name.trim(), form.source_value.trim() || undefined);
      const updated = await listMcpServers();
      setMcpServers(updated);
      setInfo(`Updated ${editingId}`);
      setEditingId(null);
      setForm({ id: '', name: '', source: 'npm', source_value: '' });
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
    });
    clearMessage();
  }

  function cancelEdit() {
    setEditingId(null);
    setForm({ id: '', name: '', source: 'npm', source_value: '' });
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

  function toggleFavorite(id: string) {
    setFavorites((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  const favoriteServers = servers.filter((s) => favorites.has(s.id));
  const otherServers = servers.filter((s) => !favorites.has(s.id));

  return (
    <div className="page">
      <div className="page-header">
        <h2>MCP Connectors</h2>
      </div>

      <div className="page-content">
        {message && <div className={`card ${messageType === 'error' ? 'card-error' : 'card-info'}`} style={{ marginBottom: 12 }}>{message}</div>}

        <div className="card" style={{ marginBottom: 16 }}>
          <div className="card-title">Search registry</div>
          <div className="card-row" style={{ display: 'flex', gap: 8 }}>
            <input
              placeholder="postgres, filesystem, slack..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
              style={{ flex: 1 }}
            />
            <button onClick={handleSearch} disabled={searching || !query.trim()}>
              {searching ? 'Searching...' : 'Search'}
            </button>
          </div>

          {searchResults.length === 0 && !searching && query.trim() && (
            <p className="card-row">No results found.</p>
          )}
          {searchResults.length > 0 && (
            <div style={{ marginTop: 12 }}>
              <div className="card-title">Results</div>
              {searchResults.map((result) => (
                <div
                  key={result.id}
                  className="card-row"
                  style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}
                >
                  <div>
                    <strong>{result.name}</strong> <span className="card-row">({result.source})</span>
                    <div className="card-row">{result.description}</div>
                  </div>
                  <button onClick={() => selectSearchResult(result)}>Use</button>
                </div>
              ))}
              <button
                onClick={() => setSearchResults([])}
                style={{ marginTop: 8 }}
              >
                Clear results
              </button>
            </div>
          )}
        </div>

        <div className="card" style={{ marginBottom: 16 }}>
          <div className="card-title">{editingId ? `Update ${editingId}` : 'Install MCP server'}</div>
          <form
            onSubmit={editingId ? handleUpdate : handleInstall}
            style={{ display: 'flex', flexDirection: 'column', gap: 8 }}
          >
            <input
              placeholder="ID (e.g. mcp-postgres)"
              value={form.id}
              onChange={(e) => setForm({ ...form, id: e.target.value })}
              disabled={!!editingId}
              required
            />
            <input
              placeholder="Display name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              required
            />
            <select
              value={form.source}
              onChange={(e) => setForm({ ...form, source: e.target.value })}
            >
              {SOURCE_OPTIONS.map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
            </select>
            <input
              placeholder="Package / owner/repo / path / url"
              value={form.source_value}
              onChange={(e) => setForm({ ...form, source_value: e.target.value })}
            />
            <div style={{ display: 'flex', gap: 8 }}>
              <button type="submit" disabled={loading}>
                {loading ? (editingId ? 'Updating...' : 'Installing...') : (editingId ? 'Update' : 'Install')}
              </button>
              {editingId && (
                <button type="button" onClick={cancelEdit} disabled={loading}>
                  Cancel
                </button>
              )}
            </div>
          </form>
        </div>

        {servers.length > 0 && favoriteServers.length === 0 && (
        <p className="empty-state">No favorites. Click the star on a server to add it here.</p>
        )}
        {favoriteServers.length > 0 && (
        <div style={{ marginBottom: 16 }}>
          <h3>Favorites</h3>
            {favoriteServers.map((server) => (
              <ServerCard
                key={server.id}
                server={server}
                isFavorite
                discovering={!!discovering[server.id]}
                onToggleFavorite={() => toggleFavorite(server.id)}
                onEdit={() => startEdit(server)}
                onDelete={() => handleDelete(server.id)}
                onDiscover={() => handleDiscover(server)}
                onOpen={() => openDrawer(server)}
              />
            ))}
          </div>
        )}

        <div>
          <h3>Installed MCP servers</h3>
          {servers.length === 0 && <p className="empty-state">No MCP servers installed yet. Use the form above to add one.</p>}
          {otherServers.map((server) => (
            <ServerCard
              key={server.id}
              server={server}
              discovering={!!discovering[server.id]}
              onToggleFavorite={() => toggleFavorite(server.id)}
              onEdit={() => startEdit(server)}
              onDelete={() => handleDelete(server.id)}
              onDiscover={() => handleDiscover(server)}
              onOpen={() => openDrawer(server)}
            />
          ))}
        </div>

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

function ServerCard({
  server,
  isFavorite,
  discovering,
  onToggleFavorite,
  onEdit,
  onDelete,
  onDiscover,
  onOpen,
}: {
  server: McpServerSummary;
  isFavorite?: boolean;
  discovering: boolean;
  onToggleFavorite: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onDiscover: () => void;
  onOpen: () => void;
}) {
  return (
    <div className="card" style={{ marginBottom: 12 }} onClick={onOpen} role="button" tabIndex={0}>
      <div className="card-title" style={{ display: 'flex', justifyContent: 'space-between' }} onClick={(e) => e.stopPropagation()}>
        <span>{server.name} <small>({server.id})</small></span>
        <button onClick={onToggleFavorite}>{isFavorite ? '★' : '☆'}</button>
      </div>
      <div className="card-row" onClick={(e) => e.stopPropagation()}>Source: {server.source}{server.source_value ? ` / ${server.source_value}` : ''}</div>
      <div className="card-row" onClick={(e) => e.stopPropagation()}>Capabilities: {server.capabilities.join(', ') || 'none'}</div>
      <div className="card-row" onClick={(e) => e.stopPropagation()}>Auth required: {server.auth_required ? 'yes' : 'no'}</div>
      <div className="card-row" onClick={(e) => e.stopPropagation()}>Discovered tools: {server.discovered_tools.length}</div>
      <div className="card-row" onClick={(e) => e.stopPropagation()}>Enabled tools: {server.enabled_tools.length || server.discovered_tools.length}</div>
      {server.discovered_tools.length > 0 && (
        <ul className="card-row" onClick={(e) => e.stopPropagation()}>
          {server.discovered_tools.map((t) => (
            <li key={t}>{t} {server.enabled_tools.includes(t) || server.enabled_tools.length === 0 ? '✓' : '✗'}</li>
          ))}
        </ul>
      )}
      <div style={{ display: 'flex', gap: 8, marginTop: 8 }} onClick={(e) => e.stopPropagation()}>
        <button onClick={onEdit}>Edit</button>
        <button onClick={onDiscover} disabled={discovering}>
          {discovering ? 'Discovering...' : 'Discover tools'}
        </button>
        <button onClick={onDelete}>Delete</button>
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
  const availableTools = Array.from(new Set([
    ...server.discovered_tools,
    ...server.enabled_tools,
    ...enabledTools,
  ]));

  return (
    <div className="drawer-backdrop" onClick={onClose}>
      <div className="drawer" onClick={(e) => e.stopPropagation()}>
        <div className="drawer-header">
          <h3>{server.name}</h3>
          <button onClick={onClose} aria-label="Close">×</button>
        </div>
        <div className="drawer-body">
          <div className="drawer-section">
            <div className="drawer-section-title">Vault secrets</div>
            {vaultSecrets.length === 0 && <p className="drawer-empty">No secrets in vault. Add them in the Vault page.</p>}
            <div className="drawer-list">
              {vaultSecrets.map((s) => (
                <label key={s.key} className="drawer-row">
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

          <div className="drawer-section">
            <div className="drawer-section-title" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span>Enabled tools</span>
              <button onClick={onDiscover} disabled={discovering}>
                {discovering ? 'Discovering...' : 'Discover'}
              </button>
            </div>
            {availableTools.length === 0 && <p className="drawer-empty">No tools discovered yet. Click Discover to fetch them.</p>}
            <div className="drawer-list">
              {availableTools.map((t) => (
                <label key={t} className="drawer-row">
                  <input
                    type="checkbox"
                    checked={enabledTools.has(t)}
                    onChange={() => onToggleTool(t)}
                  />
                  <span>{t}</span>
                </label>
              ))}
            </div>
          </div>
        </div>
        <div className="drawer-footer">
          <button onClick={onSave} disabled={saving}>{saving ? 'Saving...' : 'Save'}</button>
          <button onClick={onClose} disabled={saving}>Cancel</button>
        </div>
      </div>
    </div>
  );
}
