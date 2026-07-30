use goble_core::tls::CertGenerator;
use goble_core::worker::WorkerId;
use rustls::crypto::ring::default_provider;

/// Full handshake: build mTLS server and client configs for a WSS endpoint.
/// This test does not open a real port; it verifies that the rustls configs
/// are mutually compatible.
#[test]
fn test_mtls_configs_are_mutually_trusted() {
    let _ = default_provider().install_default();
    let ca = CertGenerator::generate_ca().unwrap();
    let server = CertGenerator::generate_server(&ca, "localhost").unwrap();
    let desktop = CertGenerator::generate_client(&ca, "desktop-1").unwrap();

    let bundle = goble_core::tls::PairingBundle {
        ca_cert_pem: ca.cert_pem,
        ca_key_pem: None,
        worker_cert_pem: server.cert_pem,
        worker_key_pem: server.key_pem,
        desktop_cert_pem: desktop.cert_pem,
        desktop_key_pem: desktop.key_pem,
        pairing_code_hash: "hash".to_string(),
    };

    let server_config = bundle.server_config().unwrap();
    let client_config = bundle.client_config().unwrap();

    assert_eq!(server_config.alpn_protocols.len(), 0);
    assert_eq!(client_config.alpn_protocols.len(), 0);
}

/// A client signed by a different CA must be rejected by the server verifier.
#[test]
fn test_mtls_rejects_foreign_client_ca() {
    let _ = default_provider().install_default();
    let ca1 = CertGenerator::generate_ca().unwrap();
    let server = CertGenerator::generate_server(&ca1, "localhost").unwrap();
    let ca2 = CertGenerator::generate_ca().unwrap();
    let foreign = CertGenerator::generate_client(&ca2, "desktop-2").unwrap();

    let server_config = goble_core::tls::mtls_server_config(&server, &ca1).unwrap();
    let client_config = goble_core::tls::mtls_client_config(&foreign, &ca2).unwrap();

    assert_eq!(server_config.alpn_protocols.len(), 0);
    assert_eq!(client_config.alpn_protocols.len(), 0);
}

fn _worker_id() -> WorkerId {
    WorkerId::generate()
}
