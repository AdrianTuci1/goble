import { useState } from 'react';
import { clusterHelmInstall } from '../tauri/api';

const PROVIDERS = ['local', 's3', 'r2', 'b2', 'minio'];

export default function ClusterInstallCard() {
  const [name, setName] = useState('goble-cluster');
  const [namespace, setNamespace] = useState('goble');
  const [replicas, setReplicas] = useState(3);
  const [storageClass, setStorageClass] = useState('');
  const [persistenceSize, setPersistenceSize] = useState('10Gi');
  const [provider, setProvider] = useState('s3');
  const [endpoint, setEndpoint] = useState('');
  const [bucket, setBucket] = useState('');
  const [accessKeyId, setAccessKeyId] = useState('');
  const [secretAccessKey, setSecretAccessKey] = useState('');
  const [region, setRegion] = useState('');
  const [intervalSeconds, setIntervalSeconds] = useState(3600);
  const [localChart, setLocalChart] = useState('');
  const [command, setCommand] = useState('');
  const [error, setError] = useState('');
  const [copied, setCopied] = useState(false);

  async function generate() {
    setError('');
    setCopied(false);
    try {
      const cmd = await clusterHelmInstall({
        name,
        namespace,
        replicas,
        storageClass: storageClass || undefined,
        persistenceSize,
        provider,
        endpoint: endpoint || undefined,
        bucket: bucket || undefined,
        accessKeyId: accessKeyId || undefined,
        secretAccessKey: secretAccessKey || undefined,
        region: region || undefined,
        intervalSeconds,
        localChart: localChart || undefined,
      });
      setCommand(cmd);
    } catch (e) {
      setError(String(e));
    }
  }

  function copy() {
    navigator.clipboard.writeText(command).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }

  const needsObjectStorage = provider !== 'local';

  return (
    <div className="cluster-install-card">
      <h3>Install Goblin cluster</h3>
      <p className="hint">
        Generate a Helm command that installs a StatefulSet of Goblin workers with
        snapshot disaster recovery. Paste the command in a cluster with Helm configured.
      </p>

      <div className="form-row">
        <label>Release name</label>
        <input value={name} onChange={(e) => setName(e.target.value)} />
      </div>
      <div className="form-row">
        <label>Namespace</label>
        <input value={namespace} onChange={(e) => setNamespace(e.target.value)} />
      </div>
      <div className="form-row">
        <label>Replicas</label>
        <input type="number" min={1} value={replicas} onChange={(e) => setReplicas(Number(e.target.value))} />
      </div>
      <div className="form-row">
        <label>Storage class</label>
        <input value={storageClass} onChange={(e) => setStorageClass(e.target.value)} placeholder="empty = default" />
      </div>
      <div className="form-row">
        <label>Persistence size</label>
        <input value={persistenceSize} onChange={(e) => setPersistenceSize(e.target.value)} />
      </div>
      <div className="form-row">
        <label>Snapshot provider</label>
        <select value={provider} onChange={(e) => setProvider(e.target.value)}>
          {PROVIDERS.map((p) => <option key={p} value={p}>{p}</option>)}
        </select>
      </div>
      {needsObjectStorage && (
        <>
          <div className="form-row">
            <label>Endpoint</label>
            <input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://..." />
          </div>
          <div className="form-row">
            <label>Bucket</label>
            <input value={bucket} onChange={(e) => setBucket(e.target.value)} />
          </div>
          <div className="form-row">
            <label>Access key ID</label>
            <input value={accessKeyId} onChange={(e) => setAccessKeyId(e.target.value)} />
          </div>
          <div className="form-row">
            <label>Secret access key</label>
            <input type="password" value={secretAccessKey} onChange={(e) => setSecretAccessKey(e.target.value)} />
          </div>
          <div className="form-row">
            <label>Region</label>
            <input value={region} onChange={(e) => setRegion(e.target.value)} />
          </div>
        </>
      )}
      <div className="form-row">
        <label>Snapshot interval (seconds)</label>
        <input type="number" min={60} value={intervalSeconds} onChange={(e) => setIntervalSeconds(Number(e.target.value))} />
      </div>
      <div className="form-row">
        <label>Local chart path (optional)</label>
        <input value={localChart} onChange={(e) => setLocalChart(e.target.value)} placeholder="e.g. deploy/goblin/charts/goblin-cluster" />
      </div>

      <button onClick={generate}>Generate helm install command</button>

      {error && <p className="error">{error}</p>}
      {command && (
        <div className="command-output">
          <pre>{command}</pre>
          <button onClick={copy}>{copied ? 'Copied!' : 'Copy'}</button>
        </div>
      )}
    </div>
  );
}
