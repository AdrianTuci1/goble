//! T3 — the generated install script, run on a real systemd host.
//!
//! [`generate_install_script`] is only ever asserted on as text in
//! `provision.rs`'s unit tests, so nothing has ever executed it. This is the one
//! thing that does: it builds the systemd-as-PID-1 fixture in
//! `deploy/systemd-host/`, starts it privileged the way a systemd host has to
//! be, builds a Linux `goblin` from this checkout in a Rust container (this is
//! the worker, not the desktop app), generates the install script through the
//! Rust API, copies the script and the worker binary into the container, runs
//! the script as root, and then asks the host itself whether the unit is active
//! and whether the worker answers `/health` on 8787.
//!
//! It runs that install **twice**, each against a fresh container: once with the
//! default workspace root, and once with a workspace root that is *not* under the
//! data root. The second is the clause a `dirname`-derived data root cannot
//! serve — the derivation named `/srv/goblin` while the worker opened its store at
//! the fixed absolute `/var/goblin/tasks.db`, so the unit flapped in `activating`
//! (`D1`: `crates/goble-core/src/provision.rs` now installs the data root
//! explicitly and passes it to the unit as `--task-store`/`--vault-path`).
//!
//! It is `#[ignore]`d because it needs the Docker daemon and a `--privileged`
//! container, which the default suite may not assume. Run it with:
//!
//! ```text
//! cargo test -p goble-core --test install_systemd_e2e -- --ignored --nocapture
//! ```
//!
//! The generated unit starts the worker with `--tls-bundle`, so a provisioned
//! worker serves mTLS only: `/health` is read through the container's published
//! 8787 with [`WorkerBundle::client_config`], the client configuration the
//! desktop itself pairs with. `curl` cannot be that client — see the note at the
//! request below.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use goble_core::identity::{ClusterCa, ClusterRole};
use goble_core::provision::{generate_install_script, ProvisionConfig, DEFAULT_DATA_ROOT};

/// The fixture image `deploy/systemd-host/Dockerfile` builds.
const FIXTURE_IMAGE: &str = "goblin-systemd-host:t3";
/// The Rust image the worker is built with, from the mounted checkout.
const RUST_IMAGE: &str = "rust:1.97-bookworm";
/// Kept between runs so the release build is paid for once.
const BUILD_VOLUME: &str = "goblin-t3-target";
/// Where the generated unit installs the worker (`ProvisionConfig::install_path`).
const INSTALL_PATH: &str = "/opt/goblin";
/// The default phase: the container command T3 documented, with the roots the
/// CLI and the desktop actually install.
const CONTAINER: &str = "goblin-t3-systemd-host";
const WORKSPACE_ROOT: &str = "/var/goblin/workspaces";
/// The second phase's container, with a workspace root a `dirname`-derived data
/// root would have called `/srv/goblin` — not where the worker writes.
const CONTAINER_OFF_ROOT: &str = "goblin-t3-systemd-host-off-root";
const OFF_WORKSPACE_ROOT: &str = "/srv/goblin/workspaces";
/// Where the script and the health client are copied inside the container.
const STAGING: &str = "/tmp/goblin-t3";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn docker_status(args: &[&str]) -> (bool, String) {
    let output = Command::new("docker")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("could not run `docker {}`: {e}", args.join(" ")));
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), text.trim().to_string())
}

/// Run docker with the full output printed: it is the evidence this test exists
/// to produce.
fn docker(args: &[&str]) -> String {
    let (ok, text) = docker_status(args);
    println!("$ docker {}\n{text}", args.join(" "));
    assert!(ok, "`docker {}` failed", args.join(" "));
    text
}

/// Run docker for a step whose output is noise (a cargo build), printing the
/// tail only.
fn docker_quiet(args: &[&str]) -> String {
    let (ok, text) = docker_status(args);
    let tail: Vec<&str> = text.lines().rev().take(12).collect();
    println!("$ docker {} …", args.join(" "));
    for line in tail.iter().rev() {
        println!("  {line}");
    }
    assert!(ok, "`docker {}` failed", args.join(" "));
    text
}

fn wait_for_systemd(container: &str) {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let (_, text) = docker_status(&["exec", container, "systemctl", "is-system-running"]);
        if text.contains("running") || text.contains("degraded") {
            println!("systemd in the fixture: {text}");
            return;
        }
        if Instant::now() > deadline {
            panic!("systemd never came up in the fixture (last: {text:?})");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Print why the unit is not up, so a failure is diagnosable from the run alone.
fn report_failure(container: &str) {
    println!("$ docker exec {container} systemctl status goblin.service --no-pager -l");
    println!(
        "{}",
        docker_status(&[
            "exec",
            container,
            "systemctl",
            "status",
            "goblin.service",
            "--no-pager",
            "-l",
        ])
        .1
    );
    println!("$ docker exec {container} journalctl -u goblin.service -n 40 --no-pager");
    println!(
        "{}",
        docker_status(&[
            "exec",
            container,
            "journalctl",
            "-u",
            "goblin.service",
            "-n",
            "40",
            "--no-pager",
        ])
        .1
    );
}

/// One mTLS `GET /health` against the worker the unit started. The client
/// configuration is the product's own — the same one a paired desktop uses — so
/// a response here is also the pairing chain working end to end.
fn get_health(
    host: &str,
    port: u16,
    server_name: &str,
    config: rustls::ClientConfig,
) -> std::io::Result<String> {
    let name = rustls::pki_types::ServerName::try_from(server_name.to_string())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let sock = TcpStream::connect((host, port))?;
    sock.set_read_timeout(Some(Duration::from_secs(10)))?;
    sock.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut tls = rustls::StreamOwned::new(conn, sock);
    tls.write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    tls.read_to_string(&mut response)?;
    Ok(response)
}

/// The JSON object in a raw HTTP/1.1 response, whatever the framing around it.
fn json_body(response: &str) -> &str {
    let start = response.find('{').unwrap_or(0);
    let end = response.rfind('}').map(|i| i + 1).unwrap_or(response.len());
    &response[start..end]
}

/// The shipped script's own `chown_guarded`, sourced out of the script on the
/// host and asked for a top level directory. `chown` is *not* stubbed here: the
/// guard must refuse before it runs, and `/srv` must still be root's.
fn guard_refusal_script() -> String {
    let mut script = String::from("eval \"$(sed -n '/^chown_guarded() {/,/^}/p' ");
    script.push_str(STAGING);
    script.push_str("/goblin-install.sh)\"; chown_guarded /srv");
    script
}

/// One full install, in its own fixture container, with the workspace root the
/// caller names: generate the script through the Rust API, copy it and the
/// worker binary into the container the way `provision_worker` does over SSH,
/// run it as root, then ask the host itself whether the unit is active and
/// whether the worker answers `/health` on 8787. Returns the health body.
fn run_install_phase(
    staging: &Path,
    binary: &Path,
    container: &str,
    workspace_root: &str,
) -> String {
    println!("\n=== install phase: {container}, workspace_root = {workspace_root} ===");

    let _ = docker_status(&["rm", "-f", container]);
    docker(&[
        "run",
        "-d",
        "--name",
        container,
        "--privileged",
        "--cgroupns=host",
        "--tmpfs",
        "/run",
        "--tmpfs",
        "/run/lock",
        "-v",
        "/sys/fs/cgroup:/sys/fs/cgroup:rw",
        // The health request comes from this process, so 8787 has to be the
        // host's too.
        "-p",
        "127.0.0.1:8787:8787",
        FIXTURE_IMAGE,
    ]);
    wait_for_systemd(container);

    // The install script and its TLS material, through the real API: one CA
    // signs the worker's bundle (what the script writes to the host) and the
    // operator identity the health client authenticates with.
    let ca = ClusterCa::generate_new("goblin-t3").expect("cluster CA");
    let worker_bundle = ca
        .sign_worker_bundle("t3-worker", "goblin-t3", 1)
        .expect("worker bundle");
    let health_client = ca
        .sign_device("t3-client", ClusterRole::Admin, 1)
        .expect("operator certificate");

    let config = ProvisionConfig {
        worker_id: "t3-worker".to_string(),
        name: container.to_string(),
        install_path: INSTALL_PATH.to_string(),
        workspace_root: workspace_root.to_string(),
        // The data root is an installed value, not a `dirname` of the workspace
        // root: the store and the vault inside it are named in the unit below.
        data_root: DEFAULT_DATA_ROOT.to_string(),
        pairing_code_hash: "t3-pairing-hash".to_string(),
        install_remote_desktop: false,
        goblin_binary: binary.to_path_buf(),
        worker_bundle: worker_bundle.clone(),
    };
    let script = generate_install_script(&config);
    for line in script.lines().filter(|l| {
        l.starts_with("INSTALL_PATH=")
            || l.starts_with("WORKSPACE_ROOT=")
            || l.starts_with("DATA_ROOT=")
    }) {
        println!("generated script: {line}");
    }
    std::fs::write(staging.join("goblin-install.sh"), &script).expect("write script");
    std::fs::write(staging.join("client.pem"), &health_client.cert_pem).unwrap();
    std::fs::write(staging.join("client-key.pem"), &health_client.key_pem).unwrap();
    std::fs::write(staging.join("ca.pem"), &ca.identity.cert_pem).unwrap();

    // What `provision_worker` does over SSH, by hand: the binary lands at
    // `$INSTALL_PATH/goblin.new`, the script is copied to /tmp and run as root.
    docker(&["exec", container, "mkdir", "-p", INSTALL_PATH]);
    docker(&[
        "cp",
        &binary.display().to_string(),
        &format!("{container}:{INSTALL_PATH}/goblin.new"),
    ]);
    docker(&["exec", container, "mkdir", "-p", STAGING]);
    for name in [
        "goblin-install.sh",
        "client.pem",
        "client-key.pem",
        "ca.pem",
    ] {
        docker(&[
            "cp",
            &staging.join(name).display().to_string(),
            &format!("{container}:{STAGING}/{name}"),
        ]);
    }
    docker(&[
        "exec",
        container,
        "chmod",
        "+x",
        &format!("{STAGING}/goblin-install.sh"),
    ]);

    // The script has to parse on the host before it is allowed to run there.
    docker(&[
        "exec",
        container,
        "bash",
        "-n",
        &format!("{STAGING}/goblin-install.sh"),
    ]);

    let installed = docker(&[
        "exec",
        container,
        "bash",
        "-c",
        &format!("timeout 900 {STAGING}/goblin-install.sh"),
    ]);
    assert!(
        installed.contains("provisioned at"),
        "the install script did not report a provisioned worker: {installed}"
    );

    // The host's own answer, from the host's own systemd.
    let (active, state) = docker_status(&[
        "exec",
        container,
        "systemctl",
        "is-active",
        "goblin.service",
    ]);
    println!("$ docker exec {container} systemctl is-active goblin.service\n{state}");
    if !active {
        report_failure(container);
    }
    assert_eq!(
        state, "active",
        "the installed unit is not active on the systemd host (workspace_root {workspace_root})"
    );

    // What the unit was actually started with, and who owns the directories the
    // install chowned.
    let unit = docker(&[
        "exec",
        container,
        "grep",
        "ExecStart",
        "/etc/systemd/system/goblin.service",
    ]);
    assert!(
        unit.contains(&format!(
            "--task-store {DEFAULT_DATA_ROOT}/tasks.db --vault-path {DEFAULT_DATA_ROOT}/vault.json"
        )),
        "the unit does not name the worker's store and vault in the data root: {unit}"
    );
    for dir in [INSTALL_PATH, workspace_root, DEFAULT_DATA_ROOT] {
        let owner = docker(&["exec", container, "stat", "-c", "%U", dir]);
        println!("owner of {dir}: {owner}");
        assert_eq!(
            owner, "goblin",
            "{dir} is not owned by the worker user after the install"
        );
    }

    // The shipped script's own guard, on the host: asked to chown a top level
    // directory it has to refuse it loudly and leave it alone.
    let guard = guard_refusal_script();
    let (guard_ok, guard_out) = docker_status(&["exec", container, "bash", "-c", &guard]);
    println!("$ docker exec {container} bash -c {guard:?}\n{guard_out}");
    assert!(
        !guard_ok && guard_out.contains("refusing to chown \"/srv\""),
        "the install's chown guard did not refuse /srv: ok={guard_ok} {guard_out}"
    );
    let srv_owner = docker(&["exec", container, "stat", "-c", "%U", "/srv"]);
    assert_eq!(srv_owner, "root", "the guard chowned /srv after all");

    // The unit starts the worker with `--tls-bundle`, so this is what plain HTTP
    // on 8787 does — recorded here so the mTLS check below is not mistaken for a
    // plaintext endpoint.
    let (plain_ok, plain) = docker_status(&[
        "exec",
        container,
        "curl",
        "-sS",
        "-o",
        "/dev/null",
        "-w",
        "%{http_code}",
        "http://127.0.0.1:8787/health",
    ]);
    println!("plain http on 8787: ok={plain_ok} {plain}");

    // curl is not the client for this endpoint, and the reason is worth keeping:
    // ring writes an identity's PKCS#8 with the public-key attribute (version 1),
    // and the container's OpenSSL refuses to load it, so `--cert`/`--key` cannot
    // present a goble certificate. The health response below is therefore read
    // with the cluster's own client configuration over the published port.
    let (curl_ok, curl_out) = docker_status(&[
        "exec",
        container,
        "curl",
        "-sS",
        "--cacert",
        &format!("{STAGING}/ca.pem"),
        "--cert",
        &format!("{STAGING}/client.pem"),
        "--key",
        &format!("{STAGING}/client-key.pem"),
        "https://127.0.0.1:8787/health",
    ]);
    println!("mTLS curl inside the container: ok={curl_ok} {curl_out}");

    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let client_config = worker_bundle
        .client_config(&health_client)
        .expect("client config from the worker bundle");
    let deadline = Instant::now() + Duration::from_secs(30);
    let body = loop {
        match get_health("127.0.0.1", 8787, "t3-worker", client_config.clone()) {
            Ok(response) => break response,
            Err(e) => {
                if Instant::now() > deadline {
                    panic!(
                        "the worker never answered /health on 8787 (workspace_root {workspace_root}): {e}"
                    );
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    };
    println!("GET https://127.0.0.1:8787/health over the published port:\n{body}");

    let report: serde_json::Value = serde_json::from_str(json_body(&body)).expect("health is JSON");
    assert_eq!(report["status"], "Online", "health body: {body}");
    assert_eq!(report["worker_id"], "t3-worker", "health body: {body}");

    docker(&["rm", "-f", container]);
    body
}

#[test]
#[ignore = "needs the Docker daemon: runs the generated install script in a privileged systemd container"]
fn the_generated_install_script_starts_the_worker_on_a_systemd_host() {
    let repo = repo_root();
    let staging = tempfile::TempDir::new().expect("staging dir");

    // The fixture: a plain Ubuntu host with systemd as PID 1, no goble code in
    // it at all.
    let fixture_dir = repo.join("deploy/systemd-host");
    docker(&[
        "build",
        "-t",
        FIXTURE_IMAGE,
        "-f",
        &fixture_dir.join("Dockerfile").display().to_string(),
        &fixture_dir.display().to_string(),
    ]);

    // The worker, built for Linux inside the container from the mounted
    // checkout — a macOS host cannot run the script's `goblin` otherwise.
    docker_quiet(&[
        "run",
        "--rm",
        "-v",
        &format!("{}:/src", repo.display()),
        "-w",
        "/src",
        "-v",
        &format!("{BUILD_VOLUME}:/target"),
        "-e",
        "CARGO_TARGET_DIR=/target",
        RUST_IMAGE,
        "cargo",
        "build",
        "-p",
        "goblin-worker",
        "--release",
    ]);
    docker_quiet(&[
        "run",
        "--rm",
        "-v",
        &format!("{BUILD_VOLUME}:/target"),
        "-v",
        &format!("{}:/out", staging.path().display()),
        RUST_IMAGE,
        "cp",
        "/target/release/goblin",
        "/out/goblin",
    ]);
    let binary = staging.path().join("goblin");
    assert!(
        binary.metadata().map(|m| m.len()).unwrap_or(0) > 1_000_000,
        "the worker built in the container is not at {}",
        binary.display()
    );

    // The install the CLI and the desktop actually make: the default roots.
    run_install_phase(staging.path(), &binary, CONTAINER, WORKSPACE_ROOT);

    // And the clause a `dirname`-derived data root cannot serve: a workspace root
    // that is not under the data root. The worker's store and vault defaults are
    // the fixed absolutes `/var/goblin/tasks.db` and `/var/goblin/vault.json`, so
    // the old derivation chowned `/srv/goblin` and left `/var/goblin` root-owned,
    // and the unit flapped in `activating` with `Error code 14: Unable to open
    // the database file`. The unit now names both, so the service comes up.
    run_install_phase(
        staging.path(),
        &binary,
        CONTAINER_OFF_ROOT,
        OFF_WORKSPACE_ROOT,
    );
}
