import { useEffect, useState, useCallback } from 'react';
import { Users, LogOut, KeyRound, AlertTriangle, Crown, UserCheck, Plus, X, Copy, Check, Monitor, Server, Network } from 'lucide-react';
import {
  listClusters,
  leaveCluster,
  exportClusterKey,
  generateDeviceIdentity,
  joinClusterWithInvite,
  generateClusterInvite,
  type ClusterMembership,
  type DeploymentConfig,
  type DeploymentMode,
  onDeviceIdentitiesUpdated,
} from '../../../../shared/tauri/api';
import './WorkerGroupsSection.css';

type ModalMode = 'create' | 'join';
type SubStep = 'mode' | 'config';

const DEFAULT_LOCAL_PORT = 8787;

function useClusters() {
  const [clusters, setClusters] = useState<ClusterMembership[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      setLoading(true);
      setError(null);
      const result = await listClusters();
      setClusters(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load clusters');
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    let unsubscribe: (() => void) | undefined;
    onDeviceIdentitiesUpdated(() => refresh()).then((u) => {
      unsubscribe = u;
    });
    return () => {
      if (unsubscribe) unsubscribe();
    };
  }, []);

  return { clusters, loading, error, refresh };
}

function getDeploymentLabel(mode: string) {
  switch (mode) {
    case 'remote_server':
      return 'Remote server';
    case 'mesh_vpn':
      return 'Mesh VPN';
    case 'local':
    default:
      return 'Local';
  }
}

function getDeploymentIcon(mode: string) {
  switch (mode) {
    case 'remote_server':
      return <Server size={14} />;
    case 'mesh_vpn':
      return <Network size={14} />;
    case 'local':
    default:
      return <Monitor size={14} />;
  }
}

export default function WorkerGroupsSection() {
  const { clusters, loading, error, refresh } = useClusters();
  const [exportError, setExportError] = useState<string | null>(null);

  const [isModalOpen, setIsModalOpen] = useState(false);
  const [modalMode, setModalMode] = useState<ModalMode>('create');
  const [subStep, setSubStep] = useState<SubStep>('mode');
  const [clusterName, setClusterName] = useState('');
  const [selectedDeploymentMode, setSelectedDeploymentMode] = useState<DeploymentMode['mode']>('local');
  const [deploymentConfig, setDeploymentConfig] = useState<DeploymentConfig>({
    mode: { mode: 'local', advertise_upnp: false, local_port: DEFAULT_LOCAL_PORT },
  });
  const [inviteCode, setInviteCode] = useState('');
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionLoading, setActionLoading] = useState(false);

  const [inviteModalOpen, setInviteModalOpen] = useState(false);
  const [generatedInvite, setGeneratedInvite] = useState<{ code: string; pem_bundle: string } | null>(null);
  const [copied, setCopied] = useState<'code' | 'pem' | null>(null);

  const openModal = useCallback(() => {
    setModalMode('create');
    setSubStep('mode');
    setClusterName('');
    setSelectedDeploymentMode('local');
    setDeploymentConfig({
      mode: { mode: 'local', advertise_upnp: false, local_port: DEFAULT_LOCAL_PORT },
    });
    setInviteCode('');
    setActionError(null);
    setActionLoading(false);
    setIsModalOpen(true);
  }, []);

  const closeModal = useCallback(() => {
    setIsModalOpen(false);
    setActionError(null);
  }, []);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        if (inviteModalOpen) {
          setInviteModalOpen(false);
        } else if (isModalOpen) {
          closeModal();
        }
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isModalOpen, closeModal, inviteModalOpen]);

  function updateDeploymentMode(mode: DeploymentMode['mode']) {
    setSelectedDeploymentMode(mode);
    let next: DeploymentConfig;
    if (mode === 'local') {
      next = { mode: { mode: 'local', advertise_upnp: false, local_port: DEFAULT_LOCAL_PORT } };
    } else if (mode === 'remote_server') {
      next = { mode: { mode: 'remote_server', host: '', user: 'root', port: 22, private_key: '', endpoint: '' } };
    } else {
      next = { mode: { mode: 'mesh_vpn', provider: 'tailscale', auth_key: '', headscale_url: '', hostname: '' } };
    }
    setDeploymentConfig(next);
  }

  function updateConfigField<K extends keyof DeploymentMode>(field: K, value: DeploymentMode[K]) {
    setDeploymentConfig((prev) => ({
      mode: { ...prev.mode, [field]: value } as DeploymentMode,
    }));
  }

  async function handleCreate() {
    if (!clusterName.trim()) {
      setActionError('Cluster name is required.');
      return;
    }
    if (deploymentConfig.mode.mode === 'remote_server') {
      const cfg = deploymentConfig.mode;
      if (!cfg.host?.trim() || !cfg.user?.trim() || !cfg.endpoint?.trim()) {
        setActionError('Host, user, and endpoint are required.');
        return;
      }
    }
    if (deploymentConfig.mode.mode === 'mesh_vpn') {
      const cfg = deploymentConfig.mode;
      if (!cfg.auth_key?.trim() || !cfg.hostname?.trim()) {
        setActionError('Auth key and hostname are required.');
        return;
      }
    }
    try {
      setActionLoading(true);
      setActionError(null);
      await generateDeviceIdentity(clusterName.trim(), deploymentConfig);
      await refresh();
      closeModal();
    } catch (e) {
      setActionError(e instanceof Error ? e.message : 'Failed to create cluster');
    } finally {
      setActionLoading(false);
    }
  }

  async function handleJoin() {
    if (!inviteCode.trim()) {
      setActionError('Invite code or PEM is required.');
      return;
    }
    try {
      setActionLoading(true);
      setActionError(null);
      await joinClusterWithInvite(inviteCode.trim());
      await refresh();
      closeModal();
    } catch (e) {
      setActionError(e instanceof Error ? e.message : 'Failed to join cluster');
    } finally {
      setActionLoading(false);
    }
  }

  async function handleLeave(id: string) {
    try {
      await leaveCluster(id);
      await refresh();
    } catch (e) {
      setExportError(e instanceof Error ? e.message : 'Failed to leave cluster');
    }
  }

  async function handleExportClusterKey() {
    try {
      setExportError(null);
      const key = await exportClusterKey();
      const blob = new Blob([key], { type: 'text/plain' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'goble-cluster-key.txt';
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (e) {
      setExportError(e instanceof Error ? e.message : 'Failed to export cluster key');
    }
  }

  async function handleInviteMember(clusterName: string) {
    try {
      setActionError(null);
      const invite = await generateClusterInvite(clusterName, 'Operator');
      setGeneratedInvite(invite);
      setInviteModalOpen(true);
    } catch (e) {
      setExportError(e instanceof Error ? e.message : 'Failed to generate invite');
    }
  }

  function copyToClipboard(type: 'code' | 'pem', value: string) {
    navigator.clipboard.writeText(value).then(() => {
      setCopied(type);
      setTimeout(() => setCopied(null), 2000);
    });
  }

  const ownerCluster = clusters.find((c) => c.is_owner);
  const memberClusters = clusters.filter((c) => !c.is_owner);

  return (
    <div className="worker-groups-section">
      <div className="worker-groups-header">
        <div>
          <h2 className="worker-groups-title">Worker Groups</h2>
          <p className="worker-groups-subtitle">
            A worker group is a cluster of devices that share the same identity key. Create a group
            and invite others, or join an existing group with an invite.
          </p>
        </div>
        <button className="worker-groups-add-btn" onClick={openModal}>
          <Plus size={16} />
          Add group
        </button>
      </div>

      {loading && <div className="worker-groups-empty">Loading clusters…</div>}

      {!loading && clusters.length === 0 && (
        <div className="worker-groups-card">
          <div className="worker-groups-empty">
            <Users size={32} />
            <p>You are not part of any worker group yet.</p>
            <p className="worker-groups-empty-hint">
              Create a new group or use an invite to join one.
            </p>
          </div>
        </div>
      )}

      {ownerCluster && (
        <div className="worker-groups-card owner">
          <div className="worker-groups-card-header">
            <Crown size={18} className="worker-groups-owner-icon" />
            <div className="worker-groups-card-title-row">
              <h3 className="worker-groups-card-title">{ownerCluster.cluster_name}</h3>
              <span className="worker-groups-badge owner">Owner</span>
              <span className={`worker-groups-deployment-badge ${ownerCluster.deployment_mode}`}>
                {getDeploymentIcon(ownerCluster.deployment_mode)}
                {getDeploymentLabel(ownerCluster.deployment_mode)}
              </span>
            </div>
          </div>
          <div className="worker-groups-deployment-status">
            {ownerCluster.deployment_status?.local_endpoint && (
              <span>Local: {ownerCluster.deployment_status.local_endpoint}</span>
            )}
            {ownerCluster.deployment_status?.public_endpoint && (
              <span>Public: {ownerCluster.deployment_status.public_endpoint}</span>
            )}
            {ownerCluster.deployment_status?.mesh_hostname && (
              <span>Mesh: {ownerCluster.deployment_status.mesh_hostname}</span>
            )}
          </div>
          <p className="worker-groups-card-hint">
            This cluster is bound to your device identity. It cannot be left; if you regenerate your
            identity, the cluster key is lost unless exported.
          </p>
          <div className="worker-groups-card-actions">
            <button className="worker-groups-action-button" onClick={handleExportClusterKey}>
              <KeyRound size={14} /> Export cluster key
            </button>
            <button className="worker-groups-action-button primary" onClick={() => handleInviteMember(ownerCluster.cluster_name)}>
              <Users size={14} /> Invite member
            </button>
          </div>
        </div>
      )}

      {memberClusters.length > 0 && (
        <div className="worker-groups-group">
          <h3 className="worker-groups-group-title">
            <UserCheck size={14} /> Member clusters
          </h3>
          <div className="worker-groups-list">
            {memberClusters.map((cluster) => (
              <div className="worker-groups-card" key={cluster.id}>
                <div className="worker-groups-card-header">
                  <Users size={18} />
                  <div className="worker-groups-card-title-row">
                    <h3 className="worker-groups-card-title">{cluster.cluster_name}</h3>
                    <span className="worker-groups-badge member">{cluster.role}</span>
                    <span className={`worker-groups-deployment-badge ${cluster.deployment_mode}`}>
                      {getDeploymentIcon(cluster.deployment_mode)}
                      {getDeploymentLabel(cluster.deployment_mode)}
                    </span>
                  </div>
                </div>
                <div className="worker-groups-deployment-status">
                  {cluster.deployment_status?.local_endpoint && (
                    <span>Local: {cluster.deployment_status.local_endpoint}</span>
                  )}
                  {cluster.deployment_status?.public_endpoint && (
                    <span>Public: {cluster.deployment_status.public_endpoint}</span>
                  )}
                  {cluster.deployment_status?.mesh_hostname && (
                    <span>Mesh: {cluster.deployment_status.mesh_hostname}</span>
                  )}
                </div>
                <div className="worker-groups-card-actions">
                  <button
                    className="worker-groups-action-button danger"
                    onClick={() => handleLeave(cluster.id)}
                  >
                    <LogOut size={14} /> Leave
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {(error || exportError) && (
        <div className="worker-groups-error">
          <AlertTriangle size={14} /> {error || exportError}
        </div>
      )}

      {isModalOpen && (
        <div className="modal-overlay" onClick={closeModal}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h3>Add Worker Group</h3>
              <div className="modal-close-group">
                <button className="modal-close" onClick={closeModal} aria-label="Close">
                  <X size={18} />
                </button>
                <span className="modal-esc">ESC</span>
              </div>
            </div>

            <p className="modal-description">
              Create a new cluster and choose how it is reachable, or join an existing cluster with
              an invite from the admin.
            </p>

            {actionError && <div className="modal-error">{actionError}</div>}

            <div className="modal-body">
              <div className="modal-field">
                <label>Action</label>
                <div className="modal-radio-group">
                  <button
                    className={`modal-radio ${modalMode === 'create' ? 'selected' : ''}`}
                    onClick={() => {
                      setModalMode('create');
                      setSubStep('mode');
                    }}
                  >
                    Create cluster
                  </button>
                  <button
                    className={`modal-radio ${modalMode === 'join' ? 'selected' : ''}`}
                    onClick={() => setModalMode('join')}
                  >
                    Join with invite
                  </button>
                </div>
              </div>

              {modalMode === 'create' ? (
                <>
                  {subStep === 'mode' ? (
                    <>
                      <div className="modal-field">
                        <label>Cluster name</label>
                        <input
                          value={clusterName}
                          onChange={(e) => setClusterName(e.target.value)}
                          placeholder="e.g., home-lab"
                        />
                      </div>
                      <div className="modal-field">
                        <label>Deployment mode</label>
                        <div className="modal-radio-group mode-select">
                          <button
                            className={`modal-radio mode-card ${selectedDeploymentMode === 'local' ? 'selected' : ''}`}
                            onClick={() => updateDeploymentMode('local')}
                          >
                            <span className="mode-card-title">
                              <Monitor size={16} /> Local
                            </span>
                            <span className="mode-card-desc">
                              This desktop is the worker. Use LAN, UPnP, or port forwarding.
                            </span>
                          </button>
                          <button
                            className={`modal-radio mode-card ${selectedDeploymentMode === 'remote_server' ? 'selected' : ''}`}
                            onClick={() => updateDeploymentMode('remote_server')}
                          >
                            <span className="mode-card-title">
                              <Server size={16} /> Remote server
                            </span>
                            <span className="mode-card-desc">
                              Deploy a worker on a VPS or server with a public endpoint.
                            </span>
                          </button>
                          <button
                            className={`modal-radio mode-card ${selectedDeploymentMode === 'mesh_vpn' ? 'selected' : ''}`}
                            onClick={() => updateDeploymentMode('mesh_vpn')}
                          >
                            <span className="mode-card-title">
                              <Network size={16} /> Mesh VPN
                            </span>
                            <span className="mode-card-desc">
                              Reach this device through Tailscale or Headscale.
                            </span>
                          </button>
                        </div>
                      </div>
                    </>
                  ) : (
                    <>
                      {selectedDeploymentMode === 'local' && (
                        <>
                          <div className="modal-field">
                            <label>Local port</label>
                            <input
                              type="number"
                              value={deploymentConfig.mode.local_port}
                              onChange={(e) =>
                                updateConfigField('local_port', parseInt(e.target.value, 10) || DEFAULT_LOCAL_PORT)
                              }
                            />
                          </div>
                          <div className="modal-field">
                            <label className="modal-checkbox">
                              <input
                                type="checkbox"
                                checked={deploymentConfig.mode.advertise_upnp}
                                onChange={(e) => updateConfigField('advertise_upnp', e.target.checked)}
                              />
                              Try UPnP / NAT-PMP auto port mapping
                            </label>
                          </div>
                          <p className="modal-hint">
                            Other devices on the same LAN can reach this desktop directly. For
                            external access, enable UPnP or forward the port manually.
                          </p>
                        </>
                      )}

                      {selectedDeploymentMode === 'remote_server' && (
                        <>
                          <div className="modal-field">
                            <label>SSH host</label>
                            <input
                              value={deploymentConfig.mode.host}
                              onChange={(e) => updateConfigField('host', e.target.value)}
                              placeholder="203.0.113.10 or vps.example.com"
                            />
                          </div>
                          <div className="modal-field">
                            <label>SSH user</label>
                            <input
                              value={deploymentConfig.mode.user}
                              onChange={(e) => updateConfigField('user', e.target.value)}
                              placeholder="root"
                            />
                          </div>
                          <div className="modal-field">
                            <label>SSH port</label>
                            <input
                              type="number"
                              value={deploymentConfig.mode.port}
                              onChange={(e) => updateConfigField('port', parseInt(e.target.value, 10) || 22)}
                            />
                          </div>
                          <div className="modal-field">
                            <label>Private key</label>
                            <textarea
                              rows={4}
                              value={deploymentConfig.mode.private_key}
                              onChange={(e) => updateConfigField('private_key', e.target.value)}
                              placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
                            />
                          </div>
                          <div className="modal-field">
                            <label>Public endpoint</label>
                            <input
                              value={deploymentConfig.mode.endpoint}
                              onChange={(e) => updateConfigField('endpoint', e.target.value)}
                              placeholder="https://vps.example.com:8787"
                            />
                          </div>
                          <p className="modal-hint">
                            The worker binary will be deployed over SSH. The public endpoint is
                            shown to peers who join the cluster.
                          </p>
                        </>
                      )}

                      {selectedDeploymentMode === 'mesh_vpn' && (
                        <>
                          <div className="modal-field">
                            <label>Provider</label>
                            <div className="modal-radio-group">
                              <button
                                className={`modal-radio ${deploymentConfig.mode.provider === 'tailscale' ? 'selected' : ''}`}
                                onClick={() => updateConfigField('provider', 'tailscale')}
                              >
                                Tailscale
                              </button>
                              <button
                                className={`modal-radio ${deploymentConfig.mode.provider === 'headscale' ? 'selected' : ''}`}
                                onClick={() => updateConfigField('provider', 'headscale')}
                              >
                                Headscale
                              </button>
                            </div>
                          </div>
                          {deploymentConfig.mode.provider === 'headscale' && (
                            <div className="modal-field">
                              <label>Headscale URL</label>
                              <input
                                value={deploymentConfig.mode.headscale_url ?? ''}
                                onChange={(e) => updateConfigField('headscale_url', e.target.value || null)}
                                placeholder="https://headscale.example.com"
                              />
                            </div>
                          )}
                          <div className="modal-field">
                            <label>Auth key</label>
                            <input
                              type="password"
                              value={deploymentConfig.mode.auth_key}
                              onChange={(e) => updateConfigField('auth_key', e.target.value)}
                              placeholder="tskey-auth-..."
                            />
                          </div>
                          <div className="modal-field">
                            <label>Hostname</label>
                            <input
                              value={deploymentConfig.mode.hostname}
                              onChange={(e) => updateConfigField('hostname', e.target.value)}
                              placeholder="goble-desktop"
                            />
                          </div>
                          <p className="modal-hint">
                            The mesh hostname is shared with invited members so they can reach this
                            device without a public IP.
                          </p>
                        </>
                      )}
                    </>
                  )}
                </>
              ) : (
                <div className="modal-field">
                  <label>Invite code or PEM</label>
                  <textarea
                    rows={8}
                    value={inviteCode}
                    onChange={(e) => setInviteCode(e.target.value)}
                    placeholder="Paste the invite code or the full PEM bundle you received from the cluster admin."
                  />
                </div>
              )}
            </div>

            <div className="modal-footer">
              <div className="modal-footer-actions">
                <button className="modal-btn secondary" onClick={closeModal}>
                  Cancel
                </button>
                {modalMode === 'create' && subStep === 'mode' && (
                  <button
                    className="modal-btn primary"
                    onClick={() => setSubStep('config')}
                    disabled={!clusterName.trim() || actionLoading}
                    data-testid="wg-modal-next"
                  >
                    Next
                  </button>
                )}
                {modalMode === 'create' && subStep === 'config' && (
                  <>
                    <button className="modal-btn secondary" onClick={() => setSubStep('mode')} data-testid="wg-modal-back">
                      Back
                    </button>
                    <button
                      className="modal-btn primary"
                      onClick={handleCreate}
                      disabled={actionLoading}
                      data-testid="wg-modal-create"
                    >
                      {actionLoading ? 'Creating…' : 'Create cluster'}
                    </button>
                  </>
                )}
                {modalMode === 'join' && (
                  <button
                    className="modal-btn primary"
                    onClick={handleJoin}
                    disabled={actionLoading}
                    data-testid="wg-modal-join"
                  >
                    {actionLoading ? 'Joining…' : 'Join cluster'}
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {inviteModalOpen && generatedInvite && (
        <div className="modal-overlay" onClick={() => setInviteModalOpen(false)}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h3>Invite member</h3>
              <div className="modal-close-group">
                <button className="modal-close" onClick={() => setInviteModalOpen(false)} aria-label="Close">
                  <X size={18} />
                </button>
                <span className="modal-esc">ESC</span>
              </div>
            </div>

            <div className="modal-body">
              <div className="modal-invite-display">
                <div className="modal-field">
                  <label>Invite code</label>
                  <div className="modal-invite-code">{generatedInvite.code}</div>
                  <button
                    className="worker-groups-action-button"
                    onClick={() => copyToClipboard('code', generatedInvite.code)}
                  >
                    {copied === 'code' ? <Check size={14} /> : <Copy size={14} />}
                    {copied === 'code' ? 'Copied' : 'Copy code'}
                  </button>
                </div>

                <div className="modal-field">
                  <label>Or share the full PEM bundle</label>
                  <textarea
                    className="modal-invite-textarea"
                    readOnly
                    rows={8}
                    value={generatedInvite.pem_bundle}
                  />
                  <button
                    className="worker-groups-action-button"
                    onClick={() => copyToClipboard('pem', generatedInvite.pem_bundle)}
                  >
                    {copied === 'pem' ? <Check size={14} /> : <Copy size={14} />}
                    {copied === 'pem' ? 'Copied' : 'Copy PEM'}
                  </button>
                </div>

                <p className="modal-hint">
                  Anyone with this code or PEM can join the cluster. Share it securely. You can
                  revoke invites from cluster settings later.
                </p>
              </div>
            </div>

            <div className="modal-footer">
              <div className="modal-footer-actions">
                <button className="modal-btn secondary" onClick={() => setInviteModalOpen(false)}>
                  Close
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
