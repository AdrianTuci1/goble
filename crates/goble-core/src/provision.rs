use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::identity::{ClusterCa, Identity};
use crate::tls::PairingBundle;
use crate::ClusterIdentity;
use anyhow::{Context, Result};

/// Self-contained certificate bundle used to start a worker with mTLS.
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkerBundle {
    pub worker_id: String,
    pub cert_pem: String,
    pub key_pem: String,
    pub ca_cert_pem: String,
    pub cluster_name: String,
}

impl WorkerBundle {
    /// Generate a fresh worker bundle from an ephemeral CA. Useful for tests and
    /// for CLI provisioning when no persistent cluster CA is available.
    pub fn generate(worker_id: &str, cluster_name: &str, _san_dns: &str) -> Result<Self> {
        let ca = ClusterCa::generate_new(cluster_name)?;
        ca.sign_worker_bundle(worker_id, cluster_name, 365)
    }

    /// Build a rustls server config from this bundle. The server will require a client
    /// certificate signed by the bundled CA and carrying an operator role.
    pub fn server_config(&self) -> Result<rustls::ServerConfig> {
        let worker = Identity::from_pem(self.cert_pem.clone(), self.key_pem.clone())?;
        crate::tls::mtls_server_config(
            &worker,
            &Identity::from_ca_pem(self.ca_cert_pem.clone(), String::new())?,
        )
    }

    /// Build a rustls client config from this bundle that verifies the server has the
    /// Worker role and presents the CA as a trusted root.
    pub fn client_config(&self, desktop_identity: &Identity) -> Result<rustls::ClientConfig> {
        crate::tls::mtls_client_config(
            desktop_identity,
            &Identity::from_ca_pem(self.ca_cert_pem.clone(), String::new())?,
        )
    }

    /// Convert the legacy pairing bundle into a WorkerBundle. Returns None if the
    /// legacy bundle does not contain a worker certificate.
    pub fn from_pairing_bundle(
        bundle: &PairingBundle,
        worker_id: &str,
        cluster_name: &str,
    ) -> Self {
        Self {
            worker_id: worker_id.to_string(),
            cert_pem: bundle.worker_cert_pem.clone(),
            key_pem: bundle.worker_key_pem.clone(),
            ca_cert_pem: bundle.ca_cert_pem.clone(),
            cluster_name: cluster_name.to_string(),
        }
    }
}

/// Transport used to copy files and run commands on a remote host.
pub trait ProvisionTransport: Send + Sync {
    /// Copy a local file to a remote path.
    fn copy_file(&self, local: &Path, remote: &str) -> Result<()>;
    /// Run a shell command on the remote host and return stdout.
    fn run_command(&self, command: &str) -> Result<String>;
}

/// SSH-based provisioning transport.
pub struct SshTransport {
    pub host: String,
    pub username: String,
    pub ssh_key: Option<PathBuf>,
}

impl SshTransport {
    pub fn new(
        host: impl Into<String>,
        username: impl Into<String>,
        ssh_key: Option<PathBuf>,
    ) -> Self {
        Self {
            host: host.into(),
            username: username.into(),
            ssh_key,
        }
    }

    fn ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
        ];
        if let Some(key) = &self.ssh_key {
            args.push("-i".to_string());
            args.push(key.display().to_string());
        }
        args.push(format!("{}@{}", self.username, self.host));
        args
    }
}

impl ProvisionTransport for SshTransport {
    fn copy_file(&self, local: &Path, remote: &str) -> Result<()> {
        let mut cmd = std::process::Command::new("scp");
        cmd.arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("ConnectTimeout=10");
        if let Some(key) = &self.ssh_key {
            cmd.arg("-i").arg(key);
        }
        cmd.arg(local)
            .arg(format!("{}@{}:{}", self.username, self.host, remote));
        let output = cmd.output().context("scp command failed to start")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("scp failed: {}", stderr);
        }
        Ok(())
    }

    fn run_command(&self, command: &str) -> Result<String> {
        let mut ssh = std::process::Command::new("ssh");
        ssh.args(self.ssh_args())
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = ssh.output().context("ssh command failed to start")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ssh command failed: {}", stderr);
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Local transport for testing provisioning on the same machine.
pub struct LocalTransport {
    pub root: PathBuf,
}

impl LocalTransport {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl ProvisionTransport for LocalTransport {
    fn copy_file(&self, local: &Path, remote: &str) -> Result<()> {
        let dest = self.root.join(remote.trim_start_matches('/'));
        std::fs::create_dir_all(dest.parent().unwrap_or(&self.root))?;
        std::fs::copy(local, &dest).with_context(|| format!("failed to copy to {:?}", dest))?;
        Ok(())
    }

    fn run_command(&self, command: &str) -> Result<String> {
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(&self.root)
            .env(
                "GOBLE_LOCAL_PROVISION_ROOT",
                self.root.display().to_string(),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("local bash command failed")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("local command failed: {}", stderr);
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// The data root the install uses when nothing else is asked for: the worker's
/// task store and vault live here, above the workspace root the CLI defaults to.
pub const DEFAULT_DATA_ROOT: &str = "/var/goblin";

/// Normalises a path textually: repeated separators collapse, `.` components
/// drop out, and `..` components cancel the component before them and are
/// clamped at the root, exactly as the shell resolving them would. Nothing here
/// touches the filesystem — these paths are on the host being provisioned and
/// need not exist on this one — so an existence check is not available and not
/// wanted: `/srv//`, `//srv` and `/srv/../srv` all have to be seen as the `/srv`
/// they resolve to.
fn normalise_path(path: &str) -> String {
    let mut normalised = String::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => normalised.truncate(normalised.rfind('/').unwrap_or(0)),
            component => {
                normalised.push('/');
                normalised.push_str(component);
            }
        }
    }
    normalised
}

/// Every directory the install hands to `chown -R` is checked here first: it has
/// to be absolute and has to resolve to a directory of its own, never to `/` and
/// never to a bare top level such as `/srv`. The check is over the *normalised*
/// path, so a value that only looks different from `/srv` — a doubled
/// separator, a `.` or a `..` component — is refused like `/srv` itself. The
/// generated script repeats the check after normalising the installed values, so
/// this is the first line and the script's own guard is the second.
fn validate_install_path(field: &str, path: &str) -> Result<()> {
    let normalised = normalise_path(path);
    let depth = normalised.split('/').filter(|c| !c.is_empty()).count();
    if !path.starts_with('/') || depth < 2 {
        anyhow::bail!(
            "refusing to install with {field} {path:?}: it must be an absolute path \
             with a directory level of its own (for example /var/goblin)"
        );
    }
    Ok(())
}

/// The data root is a directory of the install's own, never a system directory:
/// the install runs a recursive `chown` on it, and its value is a public config
/// field. Empty, `/`, a bare top level (`/srv`), a relative path and anything
/// that merely resolves to one of those are all refused, loudly.
pub fn validate_data_root(data_root: &str) -> Result<()> {
    validate_install_path("data_root", data_root)
}

/// Configuration for a worker installation.
#[derive(Debug, Clone)]
pub struct ProvisionConfig {
    pub worker_id: String,
    #[allow(dead_code)]
    pub name: String,
    pub install_path: String,
    pub workspace_root: String,
    /// The directory the worker's task store and vault are installed in. It is an
    /// installed value, not a `dirname` of `workspace_root`: the unit is given the
    /// store and vault as `--task-store`/`--vault-path`, so this is the directory
    /// the worker really writes to.
    pub data_root: String,
    pub pairing_code_hash: String,
    /// Install the remote desktop that a computer-use session streams from. It is
    /// the only heavy piece of the install, and only computer use needs it, so it
    /// downloads in the background once the worker is already serving.
    pub install_remote_desktop: bool,
    pub goblin_binary: PathBuf,
    pub worker_bundle: WorkerBundle,
}

impl ProvisionConfig {
    /// Build a provisioning configuration for a worker from the active cluster identity.
    pub fn from_cluster_identity(
        cluster: &ClusterIdentity,
        worker_id: impl Into<String>,
        name: impl Into<String>,
        _host: impl Into<String>,
        install_path: impl Into<String>,
        pairing_code_hash: impl Into<String>,
        goblin_binary: PathBuf,
    ) -> Result<Self> {
        let worker_id = worker_id.into();
        let name = name.into();
        let worker_bundle =
            cluster
                .ca
                .sign_worker_bundle(&worker_id, &cluster.cluster_name, 365)?;
        Ok(Self {
            worker_id: worker_id.clone(),
            name: name.clone(),
            install_path: install_path.into(),
            workspace_root: "/var/goblin/workspaces".to_string(),
            data_root: DEFAULT_DATA_ROOT.to_string(),
            pairing_code_hash: pairing_code_hash.into(),
            install_remote_desktop: false,
            goblin_binary,
            worker_bundle,
        })
    }
}

/// The remote desktop a computer-use session streams from. It is written to the
/// host and started in the background *after* the worker service is up, because
/// the download is the slow part of a provisioning run and a workspace that never
/// uses computer use never needs it.
const REMOTE_DESKTOP_STEP: &str = r#"
cat > "$INSTALL_PATH/goblin-remote-desktop.sh" <<'GOBLIN_REMOTE_DESKTOP'
#!/bin/bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends xrdp xorgxrdp xfce4 xfce4-terminal dbus-x11
# xrdp runs /etc/xrdp/startwm.sh; pin the session instead of inheriting whatever
# the distribution's Xsession would pick.
cat > /etc/xrdp/startwm.sh <<'STARTWM'
#!/bin/sh
[ -r /etc/profile ] && . /etc/profile
exec startxfce4
STARTWM
chmod +x /etc/xrdp/startwm.sh
adduser xrdp ssl-cert 2>/dev/null || true
systemctl enable xrdp
systemctl restart xrdp
echo "remote desktop listening on 3389"
GOBLIN_REMOTE_DESKTOP
chmod +x "$INSTALL_PATH/goblin-remote-desktop.sh"
nohup "$INSTALL_PATH/goblin-remote-desktop.sh" > /var/log/goblin-remote-desktop.log 2>&1 &
echo "remote desktop installing in the background: /var/log/goblin-remote-desktop.log"
"#;

/// The install hands its own directories to the worker user with `chown -R`, and
/// some of those directories come from public config fields, so the chown
/// refuses a path that does not resolve to a directory of its own: `/`, an empty
/// path, a bare top level such as `/srv`, a relative path, and — because the
/// comparison is over the path after normalisation — anything that merely looks
/// different from one of those, such as `/srv//`, `//srv` or `/srv/../srv`.
/// `chown -R` follows those for real, so they have to be seen as `/srv` here.
/// It fails loudly instead of skipping — a skipped chown leaves the worker
/// unable to open its store, which is the flapping service this guard exists to
/// prevent. `chown` is given the normalised target, the same path the decision
/// was made about. The normaliser is textual and never touches the filesystem:
/// the path is on this host but need not exist yet.
const INSTALL_PATH_GUARD: &str = r#"
chown_guarded() {
    local original="${1-}" target="" rest="${1-}" component
    while [ -n "$rest" ]; do
        case "$rest" in
            */*) component="${rest%%/*}"; rest="${rest#*/}" ;;
            *) component="$rest"; rest="" ;;
        esac
        case "$component" in
            "" | .) ;;
            ..) target="${target%/*}" ;;
            *) target="$target/$component" ;;
        esac
    done
    case "$target" in
        /*/*) chown -R goblin:goblin "$target" ;;
        "") echo "refusing to chown \"$original\": it is empty or is / itself" >&2; exit 1 ;;
        *) echo "refusing to chown \"$original\": it needs a directory level of its own" >&2; exit 1 ;;
    esac
}
"#;

/// Generates the shell script that installs the worker on the target host.
///
/// The worker is one self-contained binary that carries the agent harness, the
/// workflow engine and the embedded daemon; the host needs nothing else to run a
/// session. Secrets are not part of the install: they live in the client's
/// `config.toml` and are pushed to the worker over the paired connection.
pub fn generate_install_script(config: &ProvisionConfig) -> String {
    let remote_desktop = if config.install_remote_desktop {
        REMOTE_DESKTOP_STEP
    } else {
        ""
    };

    let bundle_json = serde_json::to_string(&config.worker_bundle).unwrap_or_default();

    format!(
        r#"#!/bin/bash
set -euo pipefail

INSTALL_PATH={install_path}
WORKSPACE_ROOT={workspace_root}
# The worker's task store and vault are installed in the data root, an explicit
# installed value rather than a `dirname` of the workspace root — that root is a
# public config field and may be shallow. The unit passes both below, so this is
# the directory the worker really writes to.
DATA_ROOT={data_root}
WORKER_ID={worker_id}
PAIRING_HASH={pairing_hash}
TLS_DIR="$INSTALL_PATH/tls"
BUNDLE_FILE="$TLS_DIR/worker-bundle.json"
{install_path_guard}
# The host's own tooling: the desktop service reads the worker's /platform over
# SSH with curl.
command -v curl >/dev/null 2>&1 || (apt-get update && apt-get install -y curl)

mkdir -p "$INSTALL_PATH" "$WORKSPACE_ROOT" "$TLS_DIR" "$DATA_ROOT"
groupadd -f goblin
id -u goblin >/dev/null 2>&1 || useradd -m -g goblin -s /bin/bash goblin
chown_guarded "$INSTALL_PATH"
chown_guarded "$WORKSPACE_ROOT"
chown_guarded "$DATA_ROOT"

cat > "$BUNDLE_FILE" <<'EOF'
{bundle_json}
EOF
chmod 600 "$BUNDLE_FILE"
chown goblin:goblin "$BUNDLE_FILE"

# Store the CA key on the VPS for disaster recovery using the VPS credentials.
CA_KEY_FILE="$TLS_DIR/ca-key.pem"
if [ -n "{ca_key_pem}" ]; then
    cat > "$CA_KEY_FILE" <<'EOF'
{ca_key_pem}
EOF
    chmod 600 "$CA_KEY_FILE"
    chown goblin:goblin "$CA_KEY_FILE"
fi

mv "$INSTALL_PATH/goblin.new" "$INSTALL_PATH/goblin"
chmod +x "$INSTALL_PATH/goblin"

cat > "$INSTALL_PATH/goblin.env" <<EOF
GOBLIN_WORKER_ID=$WORKER_ID
GOBLIN_WORKSPACE_ROOT=$WORKSPACE_ROOT
GOBLIN_TASK_STORE=$DATA_ROOT/tasks.db
GOBLIN_VAULT_PATH=$DATA_ROOT/vault.json
GOBLIN_PAIRING_HASH=$PAIRING_HASH
GOBLIN_TLS_BUNDLE=$BUNDLE_FILE
EOF

cat > /etc/systemd/system/goblin.service <<EOF
[Unit]
Description=Goblin Worker
After=network.target

[Service]
Type=simple
User=goblin
Group=goblin
WorkingDirectory=$INSTALL_PATH
EnvironmentFile=$INSTALL_PATH/goblin.env
ExecStart=$INSTALL_PATH/goblin --bind 0.0.0.0:8787 --workspace-root $WORKSPACE_ROOT --worker-id $WORKER_ID --tls-bundle $BUNDLE_FILE --task-store $DATA_ROOT/tasks.db --vault-path $DATA_ROOT/vault.json
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable goblin.service
systemctl restart goblin.service || echo "goblin service start requested; verify with systemctl status goblin"
{remote_desktop}
echo "Goblin worker $WORKER_ID provisioned at $INSTALL_PATH"
"#,
        install_path = config.install_path,
        workspace_root = config.workspace_root,
        data_root = config.data_root,
        install_path_guard = INSTALL_PATH_GUARD,
        worker_id = config.worker_id,
        pairing_hash = config.pairing_code_hash,
        bundle_json = bundle_json,
        ca_key_pem = config.worker_bundle.ca_cert_pem.as_str(),
        remote_desktop = remote_desktop,
    )
}

/// Provisions a worker on the target host using the given transport.
pub fn provision_worker(
    transport: &dyn ProvisionTransport,
    config: &ProvisionConfig,
) -> Result<()> {
    // Before anything is copied to the host: a directory the install will
    // recursively chown that is not a directory of its own is a configuration
    // error, not a host-side surprise. The generated script's own guard is the
    // second line, for the installed values it re-reads on the host.
    validate_install_path("install_path", &config.install_path)?;
    validate_install_path("workspace_root", &config.workspace_root)?;
    validate_data_root(&config.data_root)?;
    let script = generate_install_script(config);
    let script_path = PathBuf::from("/tmp/goblin-install.sh");
    std::fs::write(&script_path, script).context("failed to write install script")?;

    let bundle_path = PathBuf::from("/tmp/goblin-worker-bundle.json");
    std::fs::write(&bundle_path, serde_json::to_string(&config.worker_bundle)?)
        .context("failed to write worker bundle")?;

    transport.copy_file(
        &config.goblin_binary,
        &format!("{}/goblin.new", config.install_path),
    )?;
    transport.copy_file(
        &bundle_path,
        &format!("{}/tls/worker-bundle.json", config.install_path),
    )?;
    transport.copy_file(&script_path, "/tmp/goblin-install.sh")?;
    transport.run_command("chmod +x /tmp/goblin-install.sh && sudo bash /tmp/goblin-install.sh")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_generate_install_script_contains_worker_id() {
        let config = test_config(false);
        let script = generate_install_script(&config);
        assert!(script.contains("$INSTALL_PATH/goblin --bind"));
        assert!(script.contains("--tls-bundle $BUNDLE_FILE"));
        assert!(script.contains("worker-123"));
        assert!(script.contains("deadbeef"));
        assert!(script.contains("worker-bundle.json"));
    }

    /// The worker binary carries our own harness, and the secrets stay in the
    /// client's `config.toml`; the host is not given a third-party agent runtime,
    /// a container runtime, or an interpreter to run one.
    #[test]
    fn test_the_install_carries_no_foreign_runtime() {
        let script = generate_install_script(&test_config(false));
        for foreign in [
            "docker",
            "Docker",
            "crewai",
            "hermes",
            "Hermes",
            "python3",
            // Not "pip": the script's own `set -euo pipefail` contains it.
            "pip install",
        ] {
            assert!(
                !script.contains(foreign),
                "the install script still carries {foreign}"
            );
        }
    }

    /// The data root is an installed value, not a `dirname` of the public
    /// `workspace_root`, and the unit starts the worker against it: the worker's
    /// store and vault defaults are the fixed absolutes `/var/goblin/tasks.db` and
    /// `/var/goblin/vault.json`, so with a non-default `workspace_root` a worker
    /// that is not told where to write cannot open its store and flaps in
    /// `activating` on a real host: found by `tests/install_systemd_e2e.rs`, which
    /// runs this script in a systemd container.
    #[test]
    fn test_the_install_gives_the_worker_the_data_root_it_is_told_to_use() {
        let script = generate_install_script(&test_config(false));
        assert!(script.contains("DATA_ROOT=/var/goblin\n"));
        assert!(!script.contains("$(dirname"));
        assert!(script.contains(r#""$INSTALL_PATH" "$WORKSPACE_ROOT" "$TLS_DIR" "$DATA_ROOT""#));
        assert!(script.contains("--task-store $DATA_ROOT/tasks.db"));
        assert!(script.contains("--vault-path $DATA_ROOT/vault.json"));
        assert!(script.contains("GOBLIN_TASK_STORE=$DATA_ROOT/tasks.db"));
        assert!(script.contains("GOBLIN_VAULT_PATH=$DATA_ROOT/vault.json"));
        // One `chown -R`, inside the guard: a second one written by hand would be
        // a second bypass of the check that exists to keep `chown -R` off a
        // system directory.
        assert_eq!(
            script.matches("chown -R").count(),
            1,
            "the install carries a `chown -R` outside `chown_guarded`"
        );
    }

    /// A data root that is a system directory (or nothing at all) must be refused,
    /// not handed to `chown -R`: `workspace_root` is a public field, so a caller
    /// can pass something shallow. The refusal is the generated script's own guard
    /// function, run here exactly as the install runs it.
    #[test]
    fn test_the_install_refuses_a_shallow_data_root() {
        let mut config = test_config(false);
        config.data_root = "/srv".to_string();
        let script = generate_install_script(&config);
        assert!(script.contains("DATA_ROOT=/srv\n"));
        assert!(script.contains("chown_guarded \"$DATA_ROOT\""));

        // The Rust-side refusal, before anything is written or copied.
        for shallow in ["", "/", "/srv", "srv", "/..", "/var/.."] {
            assert!(
                validate_data_root(shallow).is_err(),
                "data_root {shallow:?} was accepted"
            );
        }
        assert!(validate_data_root(DEFAULT_DATA_ROOT).is_ok());
        assert!(validate_data_root("/srv/goblin/").is_ok());

        // And the guard itself, executed: it refuses a system directory, and
        // passes a directory of its own through to the real chown.
        let refused = run_guard(&script, "/srv");
        assert_ne!(
            refused.0,
            Some(0),
            "the guard chowned a top level directory"
        );
        assert!(
            refused.1.contains("refusing to chown \"/srv\""),
            "guard output: {}",
            refused.1
        );
        let refused_root = run_guard(&script, "/");
        assert_ne!(refused.0, Some(0), "the guard chowned /");
        assert!(refused_root.1.contains("refusing to chown \"/\""));

        let allowed = run_guard(&script, "/var/goblin/");
        assert_eq!(
            allowed.0,
            Some(0),
            "the guard refused a directory of its own"
        );
        assert!(
            allowed.1.contains("chown -R goblin:goblin /var/goblin"),
            "guard output: {}",
            allowed.1
        );
    }

    /// The guard compares the *normalised* path, so a value that only looks
    /// different from a bare top level is refused like the top level itself:
    /// `/srv//`, `//srv` and `/srv/../srv` all used to match the guard's `/*/*`
    /// and produce `chown -R goblin:goblin /srv`. Every path the install chowns
    /// is covered: the guard is run out of the generated script, once with the
    /// value as the workspace root and once as the data root, and the Rust side
    /// has to refuse each value first.
    #[test]
    fn test_the_chown_guard_refuses_a_path_that_only_looks_different() {
        for shallow in ["/srv", "/srv//", "//srv", "/srv/../srv", "/", ""] {
            // The Rust-side refusal, before anything is copied to the host.
            for field in ["install_path", "workspace_root", "data_root"] {
                let refused = validate_install_path(field, shallow);
                assert!(
                    refused.is_err(),
                    "{field} {shallow:?} was accepted by the Rust-side check"
                );
                assert!(
                    refused
                        .unwrap_err()
                        .to_string()
                        .contains("refusing to install"),
                    "{field} {shallow:?} was refused without saying so"
                );
            }

            for field in ["workspace_root", "data_root"] {
                let mut config = test_config(false);
                if field == "workspace_root" {
                    config.workspace_root = shallow.to_string();
                } else {
                    config.data_root = shallow.to_string();
                }
                let script = generate_install_script(&config);
                let (status, output) = run_guard(&script, shallow);
                println!("guard on {shallow:?} as {field}: exit {status:?} — {output}");
                assert_eq!(
                    status,
                    Some(1),
                    "the generated script's guard did not refuse {shallow:?} as {field}, \
                     loudly: {output}"
                );
                assert!(
                    output.contains(&format!("refusing to chown \"{shallow}\"")),
                    "the guard refused {shallow:?} as {field} without saying why: {output}"
                );
                assert!(
                    !output.contains("chown -R"),
                    "the guard refused {shallow:?} as {field} after chowning it: {output}"
                );
            }
        }
    }

    /// Normalising must not cost the install a directory that really is its own,
    /// and the chown has to target the normalised path — the same path the
    /// decision was made about, so the check and the `chown -R` cannot disagree.
    #[test]
    fn test_the_guard_chowns_a_path_the_install_owns() {
        let script = generate_install_script(&test_config(false));
        for (value, chowned) in [
            ("/var/goblin/workspaces", "/var/goblin/workspaces"),
            ("/srv/goblin/", "/srv/goblin"),
            ("/srv//goblin/", "/srv/goblin"),
            ("/srv/./goblin", "/srv/goblin"),
            ("//srv/goblin/../goblin", "/srv/goblin"),
        ] {
            assert!(
                validate_install_path("workspace_root", value).is_ok(),
                "the Rust-side check refused the install's own directory {value:?}"
            );
            let (status, output) = run_guard(&script, value);
            assert_eq!(status, Some(0), "the guard refused {value:?}: {output}");
            assert!(
                output.contains(&format!("chown -R goblin:goblin {chowned}")),
                "the guard did not chown {value:?} as {chowned:?}: {output}"
            );
        }
    }

    /// Runs the *generated script's* own `chown_guarded` on `target`: the
    /// function is sliced out of the script text itself, where
    /// `tests/install_systemd_e2e.rs` sources it from the script on the host, so
    /// this tests the guard that ships rather than a copy of it. `chown` is
    /// stubbed and records every call, so the test sees both the decision
    /// (`Some(1)` for a refusal, never a silent skip) and whether the guard
    /// chowned anyway.
    fn run_guard(script: &str, target: &str) -> (Option<i32>, String) {
        let start = script
            .find("chown_guarded() {")
            .expect("the generated script carries the chown guard");
        let end = start
            + script[start..]
                .find("\n}")
                .expect("the guard function is closed")
            + 2;
        let guard = &script[start..end];
        // One argument, whatever it holds: the install passes these values as
        // single quoted expansions, and the empty string has to survive as one.
        let quoted = format!("'{}'", target.replace('\'', r"'\''"));
        let harness =
            format!("chown() {{ echo \"chown $*\"; }}\n{guard}\nchown_guarded {quoted}\n");
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(&harness)
            .output()
            .expect("bash runs the guard");
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status.code(), text.trim().to_string())
    }

    /// `provision_worker` refuses a shallow data root before it touches the host.
    #[test]
    fn test_provisioning_refuses_a_shallow_data_root_before_copying() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(false);
        config.data_root = "/srv".to_string();
        config.goblin_binary = tmp.path().join("goblin");
        let err = provision_worker(&LocalTransport::new(tmp.path()), &config)
            .expect_err("a shallow data root must be refused");
        assert!(
            err.to_string()
                .contains("refusing to install with data_root"),
            "error: {err}"
        );
    }

    /// The same, for the workspace root: the guard above is not the only line.
    /// `workspace_root` is the field a caller passes most freely, and it is
    /// chowned recursively too, so it is checked in Rust before a single byte is
    /// copied to the host.
    #[test]
    fn test_provisioning_refuses_a_shallow_workspace_root_before_copying() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(false);
        config.workspace_root = "/srv//".to_string();
        config.goblin_binary = tmp.path().join("goblin");
        let err = provision_worker(&LocalTransport::new(tmp.path()), &config)
            .expect_err("a shallow workspace root must be refused");
        assert!(
            err.to_string()
                .contains("refusing to install with workspace_root"),
            "error: {err}"
        );
        assert!(
            !tmp.path().join("opt").exists(),
            "the worker was copied to the host before the workspace root was checked"
        );
    }

    #[test]
    fn test_the_remote_desktop_is_not_installed_unless_it_is_asked_for() {
        let script = generate_install_script(&test_config(false));
        assert!(!script.contains("xrdp"));
        assert!(!script.contains("goblin-remote-desktop"));
    }

    /// Computer use needs a desktop on the host, and that desktop is the slow part
    /// of the install: it must be started in the background, and only after the
    /// worker service is already serving.
    #[test]
    fn test_the_remote_desktop_installs_in_the_background_after_the_service() {
        let script = generate_install_script(&test_config(true));
        let service_started = script
            .find("systemctl restart goblin.service")
            .expect("the worker service is started");
        let desktop_launched = script
            .find("nohup \"$INSTALL_PATH/goblin-remote-desktop.sh\"")
            .expect("the remote desktop is launched");
        assert!(
            desktop_launched > service_started,
            "the desktop install must be launched after the worker is serving"
        );
        assert!(script.contains("xrdp"));
        assert!(script.contains("> /var/log/goblin-remote-desktop.log 2>&1 &"));
    }


    fn test_config(install_remote_desktop: bool) -> ProvisionConfig {
        ProvisionConfig {
            worker_id: "worker-123".to_string(),
            name: "vps-1".to_string(),
            install_path: "/opt/goblin".to_string(),
            workspace_root: "/var/goblin/workspaces".to_string(),
            data_root: DEFAULT_DATA_ROOT.to_string(),
            pairing_code_hash: "deadbeef".to_string(),
            install_remote_desktop,
            goblin_binary: PathBuf::from("/tmp/goblin"),
            // A hand-written bundle rather than a generated one: a real
            // certificate is base64, and random base64 would make the
            // "does the script carry X" assertions below flaky.
            worker_bundle: WorkerBundle {
                worker_id: "worker-123".to_string(),
                cert_pem: "-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----".to_string(),
                key_pem: "-----BEGIN PRIVATE KEY-----\ntest\n-----END PRIVATE KEY-----".to_string(),
                ca_cert_pem: "-----BEGIN CERTIFICATE-----\ntest-ca\n-----END CERTIFICATE-----"
                    .to_string(),
                cluster_name: "goble-test".to_string(),
            },
        }
    }

    #[test]
    fn test_provision_bundle_contains_worker_cert() {
        let bundle = WorkerBundle::generate("worker-abc", "test-cluster", "goblin.local").unwrap();
        assert_eq!(bundle.worker_id, "worker-abc");
        assert_eq!(bundle.cluster_name, "test-cluster");
        assert!(bundle.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(bundle.key_pem.contains("BEGIN PRIVATE KEY"));
        assert!(bundle.ca_cert_pem.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn test_worker_bundle_server_config_builds() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let bundle = WorkerBundle::generate("worker-1", "test-cluster", "goblin.local").unwrap();
        let server_config = bundle.server_config().unwrap();
        assert!(server_config.alpn_protocols.is_empty());
    }

    #[test]
    fn test_local_transport_copy_and_run() {
        let tmp = TempDir::new().unwrap();
        let transport = LocalTransport::new(tmp.path());
        let local = tmp.path().join("source.txt");
        std::fs::write(&local, "hello").unwrap();
        transport.copy_file(&local, "/dest/source.txt").unwrap();
        let out = transport.run_command("cat dest/source.txt").unwrap();
        assert_eq!(out.trim(), "hello");
    }
}
