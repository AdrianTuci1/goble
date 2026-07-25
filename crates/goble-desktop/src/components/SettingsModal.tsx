import { useEffect, useState } from 'react';
import { useStore } from '../stores/appStore';
import { workerLogs, getLlmSetting, setLlmSetting, LLM_PROVIDERS } from '../tauri/api';

export default function SettingsModal() {
  const isOpen = useStore((s) => s.isSettingsOpen);
  const setOpen = useStore((s) => s.setSettingsOpen);
  const logs = useStore((s) => s.logs);

  const [provider, setProvider] = useState('openai');
  const [apiKey, setApiKey] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [model, setModel] = useState('gpt-4o-mini');
  const [temperature, setTemperature] = useState('0.7');

  useEffect(() => {
    if (!isOpen) return;
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
    });
  }, [isOpen, provider]);

  if (!isOpen) return null;

  return (
    <div className="modal-overlay" onClick={() => setOpen(false)}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h3>Settings</h3>
          <button onClick={() => setOpen(false)}>Close</button>
        </div>
        <div className="modal-body">
          <div className="settings-section">
            <h4>LLM Provider</h4>
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
            <button
              onClick={() =>
                setLlmSetting(
                  provider,
                  apiKey,
                  model,
                  baseUrl || undefined,
                  parseFloat(temperature)
                ).then(() => setOpen(false))
              }
            >
              Save LLM Settings
            </button>
          </div>
          <div className="settings-section">
            <h4>Logs</h4>
            <button onClick={() => workerLogs().then((l) => useStore.getState().setLogs(l))}>
              Refresh
            </button>
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
      </div>
    </div>
  );
}
