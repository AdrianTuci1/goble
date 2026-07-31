import { useState } from 'react';
import { useStore } from '../stores/appStore';
import { setVaultSecret, unlockVault } from '../tauri/api';
import './Pages.css';

export default function VaultPage() {
  const secrets = useStore((s) => s.vaultSecrets);
  const [passphrase, setPassphrase] = useState('');
  const [key, setKey] = useState('');
  const [value, setValue] = useState('');
  const [unlocked, setUnlocked] = useState(false);

  async function handleUnlock() {
    if (!passphrase) return;
    await unlockVault(passphrase);
    setUnlocked(true);
  }

  async function handleAddSecret() {
    if (!key || !value) return;
    await setVaultSecret(key, value);
    setKey('');
    setValue('');
  }

  return (
    <div className="page">
      <div className="page-header">
        <h2>Vault</h2>
      </div>
      <div className="page-content">
        {!unlocked && (
          <div className="vault-unlock">
            <input
              type="password"
              placeholder="Vault passphrase"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
            />
            <button onClick={handleUnlock}>Unlock vault</button>
          </div>
        )}
        {unlocked && (
          <div className="vault-form">
            <input
              placeholder="Secret key"
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
            <input
              type="password"
              placeholder="Secret value"
              value={value}
              onChange={(e) => setValue(e.target.value)}
            />
            <button onClick={handleAddSecret}>Add secret</button>
          </div>
        )}
        <div className="secret-list">
          {secrets.map((s) => (
            <div key={s.key} className="card">
              <div className="card-title">{s.key}</div>
              <div className="card-row">Updated: {s.updated_at}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
