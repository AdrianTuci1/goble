import { useEffect, useMemo, useState } from 'react';
import { User, Mail, Download, QrCode, RefreshCw, Upload, Shield, AlertTriangle } from 'lucide-react';
import {
  useGeneralStore,
  generateRandomName,
} from '../../store/generalStore';
import {
  getDeviceIdentity,
  generateDeviceIdentity,
  importDeviceIdentity,
  exportDeviceIdentity,
  type DeviceIdentity,
  onDeviceIdentitiesUpdated,
} from '../../../../shared/tauri/api';
import IdentityQrCode from './IdentityQrCode';
import './GeneralSection.css';

function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return '?';
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

function stringToColor(str: string): string {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = str.charCodeAt(i) + ((hash << 5) - hash);
  }
  const h = Math.abs(hash) % 360;
  return `hsl(${h} 70% 45%)`;
}

function Avatar({ name }: { name: string }) {
  const label = initials(name);
  const bg = stringToColor(name || 'goble');
  return (
    <div className="general-avatar" style={{ background: bg }} aria-label={`Avatar for ${name}`}>
      <span className="general-avatar-initials">{label}</span>
    </div>
  );
}

export default function GeneralSection() {
  const displayName = useGeneralStore((s) => s.displayName);
  const email = useGeneralStore((s) => s.email);
  const avatarSeed = useGeneralStore((s) => s.avatarSeed);
  const setDisplayName = useGeneralStore((s) => s.setDisplayName);
  const setEmail = useGeneralStore((s) => s.setEmail);
  const setAvatarSeed = useGeneralStore((s) => s.setAvatarSeed);

  const effectiveName = useMemo(
    () => displayName.trim() || avatarSeed,
    [displayName, avatarSeed]
  );

  const [identity, setIdentity] = useState<DeviceIdentity | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showQr, setShowQr] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importValue, setImportValue] = useState('');
  const [regenerateConfirm, setRegenerateConfirm] = useState(false);

  async function refreshIdentity() {
    try {
      setLoading(true);
      setError(null);
      const result = await getDeviceIdentity();
      setIdentity(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load identity');
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refreshIdentity();
    let unsubscribe: (() => void) | undefined;
    onDeviceIdentitiesUpdated(() => refreshIdentity()).then((u) => {
      unsubscribe = u;
    });
    return () => {
      if (unsubscribe) unsubscribe();
    };
  }, []);

  async function handleGenerate() {
    try {
      setLoading(true);
      setError(null);
      const result = await generateDeviceIdentity('personal');
      setIdentity(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to generate identity');
    } finally {
      setLoading(false);
    }
  }

  async function handleImport() {
    if (!importValue.trim()) return;
    try {
      setLoading(true);
      setError(null);
      const result = await importDeviceIdentity(importValue.trim());
      setIdentity(result);
      setImportValue('');
      setImportOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to import identity');
    } finally {
      setLoading(false);
    }
  }

  async function handleDownload() {
    try {
      setError(null);
      const pem = await exportDeviceIdentity();
      const blob = new Blob([pem], { type: 'application/x-pem-file' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `goble-identity-${identity?.cluster_name || 'personal'}.pem`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to export identity');
    }
  }

  function handleRegenerateName() {
    setAvatarSeed(generateRandomName());
  }

  function handleRegenerateIdentity() {
    if (!regenerateConfirm) {
      setRegenerateConfirm(true);
      return;
    }
    setRegenerateConfirm(false);
    handleGenerate();
  }

  return (
    <div className="general-section">
      <div className="general-section-header">
        <h2 className="general-section-title">General</h2>
        <p className="general-section-subtitle">
          Manage your profile, contact details, and device identity. Your identity key is portable:
          you can use the same PEM on multiple devices or share it by scanning the QR code.
        </p>
      </div>

      <div className="general-card">
        <div className="general-avatar-row">
          <Avatar name={effectiveName} />
          <div className="general-avatar-info">
            <div className="general-avatar-name">{effectiveName}</div>
            <div className="general-avatar-hint">
              {displayName.trim() ? 'Custom display name' : 'Randomly generated name'}
            </div>
          </div>
        </div>

        <div className="general-field">
          <label className="general-label">
            <User size={14} /> Display name
          </label>
          <input
            className="general-input"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            placeholder={avatarSeed}
          />
          <p className="general-hint">
            Leave empty to use the generated name. Open-source users who do not add a name receive a
            random name like “honeycomb204”.
          </p>
        </div>

        <div className="general-field">
          <label className="general-label">
            <Mail size={14} /> Email
          </label>
          <input
            className="general-input"
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="you@example.com"
          />
        </div>

        <div className="general-field">
          <button className="general-link-button" onClick={handleRegenerateName}>
            <RefreshCw size={14} /> Regenerate random name
          </button>
        </div>
      </div>

      <div className="general-card">
        <div className="general-card-header">
          <Shield size={18} />
          <h3 className="general-card-title">Device Identity</h3>
        </div>

        {!identity && !loading && (
          <div className="general-empty-state">
            <p>No device identity configured yet.</p>
            <button className="general-primary-button" onClick={handleGenerate}>
              <Shield size={14} /> Generate identity
            </button>
          </div>
        )}

        {identity && (
          <div className="general-identity-details">
            <div className="general-identity-row">
              <span className="general-identity-label">Cluster</span>
              <span className="general-identity-value">{identity.cluster_name}</span>
            </div>
            <div className="general-identity-row">
              <span className="general-identity-label">Role</span>
              <span className={`general-identity-badge ${identity.is_owner ? 'owner' : 'member'}`}>
                {identity.is_owner ? 'Owner' : identity.role}
              </span>
            </div>
            <div className="general-identity-row">
              <span className="general-identity-label">Serial</span>
              <span className="general-identity-value mono">{identity.id.slice(0, 12)}</span>
            </div>

            <div className="general-identity-actions">
              <button className="general-action-button" onClick={handleDownload}>
                <Download size={14} /> Download .pem
              </button>
              <button className="general-action-button" onClick={() => setShowQr((s) => !s)}>
                <QrCode size={14} /> {showQr ? 'Hide QR' : 'Show QR'}
              </button>
              <button className="general-action-button danger" onClick={handleRegenerateIdentity}>
                <RefreshCw size={14} />
                {regenerateConfirm ? 'Confirm regenerate' : 'Regenerate'}
              </button>
            </div>

            {showQr && (
              <div className="general-identity-qr">
                <IdentityQrCode value={`${identity.cert_pem}\n${identity.key_pem}`} />
                <p className="general-qr-hint">Scan this QR code to import the identity on another device.</p>
              </div>
            )}

            {regenerateConfirm && (
              <div className="general-warning">
                <AlertTriangle size={14} />
                Regenerating creates a new identity and cluster. Existing memberships will be lost unless you
                export and re-import the current identity first.
              </div>
            )}
          </div>
        )}

        <div className="general-import-section">
          <button className="general-link-button" onClick={() => setImportOpen((s) => !s)}>
            <Upload size={14} /> {importOpen ? 'Cancel import' : 'Import identity PEM'}
          </button>
          {importOpen && (
            <div className="general-import-form">
              <textarea
                className="general-textarea"
                rows={6}
                value={importValue}
                onChange={(e) => setImportValue(e.target.value)}
                placeholder="Paste the PEM bundle here (certificate + private key)..."
              />
              <button className="general-primary-button" onClick={handleImport} disabled={!importValue.trim() || loading}>
                <Upload size={14} /> Import PEM
              </button>
            </div>
          )}
        </div>

        {error && (
          <div className="general-error">
            <AlertTriangle size={14} /> {error}
          </div>
        )}
      </div>
    </div>
  );
}
