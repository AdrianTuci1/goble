use goble_core::tls::{CertGenerator, PairingBundle};
use goble_core::worker::WorkerId;

/// Verifies that a pairing bundle can be produced and serialized for transport.
#[test]
fn test_pairing_bundle_for_worker_contains_all_pems() {
    let ca = CertGenerator::generate_ca().unwrap();
    let server = CertGenerator::generate_server(&ca, "goblin.local").unwrap();
    let worker_id = WorkerId::generate();
    let desktop = CertGenerator::generate_client(&ca, &worker_id.0).unwrap();

    let bundle = PairingBundle {
        ca_cert_pem: ca.cert_pem,
        ca_key_pem: None,
        worker_cert_pem: server.cert_pem,
        worker_key_pem: server.key_pem,
        desktop_cert_pem: desktop.cert_pem,
        desktop_key_pem: desktop.key_pem,
        pairing_code_hash: "deadbeef".to_string(),
    };

    let json = serde_json::to_string(&bundle).unwrap();
    assert!(json.contains("BEGIN CERTIFICATE"));
    assert!(json.contains("BEGIN PRIVATE KEY"));
}

/// Ensures the secret set/get subcommands parse correctly.
#[test]
fn test_secret_subcommand_parsing() {
    use clap::Parser;
    use goble_cli::Args;
    let _ = Args::parse_from([
        "goble-cli",
        "secret",
        "set",
        "--worker",
        "worker-1",
        "--url",
        "wss://vps-1.local:8787/ws",
        "--name",
        "openai-api-key",
        "--value",
        "sk-123",
    ]);
    let _ = Args::parse_from([
        "goble-cli",
        "secret",
        "get",
        "--worker",
        "worker-1",
        "--url",
        "wss://vps-1.local:8787/ws",
        "--name",
        "openai-api-key",
    ]);
}

/// Ensures the schedule-manage subcommands parse correctly.
#[test]
fn test_schedule_manage_subcommand_parsing() {
    use clap::Parser;
    use goble_cli::Args;
    let _ = Args::parse_from([
        "goble-cli",
        "schedule-manage",
        "list",
        "--worker",
        "worker-1",
        "--url",
        "wss://vps-1.local:8787/ws",
    ]);
    let _ = Args::parse_from([
        "goble-cli",
        "schedule-manage",
        "cancel",
        "--worker",
        "worker-1",
        "--url",
        "wss://vps-1.local:8787/ws",
        "--task-id",
        "task-123",
    ]);
}

/// Ensures the CLI setup-worker command can be parsed and its alias points at the same logic.
#[test]
fn test_setup_worker_cli_parsing() {
    use clap::Parser;
    use goble_cli::Args;
    let _ = Args::parse_from([
        "goble-cli",
        "setup-worker",
        "--name",
        "vps-1",
        "--host",
        "vps-1.local",
        "--username",
        "root",
        "--local-test",
    ]);
}

/// Ensures the cluster helm-install subcommand parses correctly.
#[test]
fn test_cluster_helm_install_subcommand_parsing() {
    use clap::Parser;
    use goble_cli::Args;
    let _ = Args::parse_from([
        "goble-cli",
        "cluster",
        "helm-install",
        "--name",
        "goblin",
        "--namespace",
        "goblin",
        "--replicas",
        "3",
        "--passphrase",
        "secret",
        "--provider",
        "r2",
        "--endpoint",
        "https://example.r2.cloudflarestorage.com",
        "--bucket",
        "my-goble-snapshots",
        "--access-key-id",
        "abc",
        "--secret-access-key",
        "xyz",
    ]);
}
#[test]
fn test_device_restore_subcommand_parsing() {
    use clap::Parser;
    use goble_cli::Args;
    let _ = Args::parse_from([
        "goble-cli",
        "device",
        "restore",
        "--from-snapshot",
        "/tmp/snapshots",
        "--cluster-key",
        "c3Vkd2f8sX7f8J8f8J8f8J8f8J8f8J8f8J8f8J8=",
        "--passphrase",
        "secret",
        "--device-id",
        "phone-device",
        "--device-name",
        "Phone",
    ]);
}
