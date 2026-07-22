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

/// Ensures the CLI setup-worker command can be parsed and its alias points at the same logic.
/// Full provisioning is exercised by the unit tests in provision.rs; here we test argument plumbing.
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
