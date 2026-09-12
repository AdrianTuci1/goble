use super::*;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

fn make_ca() -> ClusterCa {
    ClusterCa::generate_new("test-cluster").unwrap()
}

#[test]
fn test_ca_generation() {
    let ca = make_ca();
    assert!(ca.identity.cert_pem.contains("BEGIN CERTIFICATE"));
    assert!(ca.identity.key_pem.contains("BEGIN PRIVATE KEY"));
    assert_eq!(ca.identity.role, ClusterRole::Owner);
}

#[test]
fn test_sign_device() {
    let ca = make_ca();
    let device = ca
        .sign_device("desktop-1", ClusterRole::Admin, 365)
        .unwrap();
    assert!(device.cert_pem.contains("BEGIN CERTIFICATE"));
    assert!(device.key_pem.contains("BEGIN PRIVATE KEY"));
    assert_eq!(device.role, ClusterRole::Admin);
    assert_eq!(extract_role(&device.cert_pem).unwrap(), ClusterRole::Admin);
}

#[test]
fn test_sign_worker() {
    let ca = make_ca();
    let worker = ca.sign_worker("worker-1", 365).unwrap();
    assert_eq!(worker.role, ClusterRole::Worker);
    ca.verify_worker(&worker.cert_pem).unwrap();
}

#[test]
fn test_verify_role_rejects_wrong_role() {
    let ca = make_ca();
    let viewer = ca
        .sign_device("viewer-1", ClusterRole::Viewer, 365)
        .unwrap();
    assert!(ca.verify_admin(&viewer.cert_pem).is_err());
    assert!(ca.verify_controller(&viewer.cert_pem).is_err());
}

#[test]
fn test_revoke_and_crl() {
    let ca = make_ca();
    let device = ca
        .sign_device("device-1", ClusterRole::Operator, 365)
        .unwrap();
    let serial = device.serial().to_string();
    ca.revoke(&serial).unwrap();
    let crl = ca.crl().unwrap();
    assert_eq!(crl.version, 1);
    assert!(crl.revoked_serials.contains(&serial));
    crl.verify(&ca.identity.cert_pem).unwrap();
    assert!(ca.verify_controller(&device.cert_pem).is_err());
}

#[test]
fn test_crl_apply_updates_store() {
    let ca = make_ca();
    let device = ca.sign_device("device-1", ClusterRole::Admin, 365).unwrap();
    let serial = device.serial().to_string();
    ca.revoke(&serial).unwrap();
    let crl = ca.crl().unwrap();

    let other_ca = ClusterCa::from_ca_cert_pem(&ca.identity.cert_pem).unwrap();
    other_ca
        .store
        .write()
        .unwrap()
        .add_identity(&ca.identity)
        .unwrap();
    other_ca
        .store
        .write()
        .unwrap()
        .add_identity(&device)
        .unwrap();
    assert!(other_ca.apply_crl(crl).unwrap());
    assert!(other_ca.store.read().unwrap().is_revoked(&serial));
    assert!(!other_ca.store.read().unwrap().is_active(&serial));
}

#[test]
fn test_certificate_store_rejects_revoked() {
    let ca = make_ca();
    let device = ca.sign_device("device-1", ClusterRole::Admin, 365).unwrap();
    let serial = device.serial().to_string();
    ca.revoke(&serial).unwrap();
    assert!(ca
        .store
        .write()
        .unwrap()
        .add(device.cert_pem.clone())
        .is_err());
}

#[test]
fn test_role_ordering_hierarchy() {
    assert!(ClusterRole::Owner.can_manage_cluster());
    assert!(ClusterRole::Admin.can_manage_cluster());
    assert!(!ClusterRole::Operator.can_manage_cluster());
    assert!(ClusterRole::Operator.can_operate());
    assert!(!ClusterRole::Viewer.can_operate());
    assert!(!ClusterRole::Worker.can_operate());
}

#[test]
fn test_tls_configs_build() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = make_ca();
    let worker = ca.sign_worker("worker-1", 365).unwrap();
    let desktop = ca
        .sign_device("desktop-1", ClusterRole::Admin, 365)
        .unwrap();

    let server_config = ca
        .server_config(&worker, vec![ClusterRole::Admin, ClusterRole::Owner])
        .unwrap();
    let client_config = ca.client_config(&desktop, ClusterRole::Worker).unwrap();
    assert!(server_config.alpn_protocols.is_empty());
    assert!(client_config.alpn_protocols.is_empty());
}

struct Pipe {
    reader: Arc<Mutex<VecDeque<u8>>>,
    writer: Arc<Mutex<VecDeque<u8>>>,
}

fn pipe_pair() -> (Pipe, Pipe) {
    let a = Arc::new(Mutex::new(VecDeque::new()));
    let b = Arc::new(Mutex::new(VecDeque::new()));
    (
        Pipe {
            reader: a.clone(),
            writer: b.clone(),
        },
        Pipe {
            reader: b,
            writer: a,
        },
    )
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut reader = self.reader.lock().unwrap();
        let mut n = 0;
        while n < buf.len() && !reader.is_empty() {
            buf[n] = reader.pop_front().unwrap();
            n += 1;
        }
        Ok(n)
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut writer = self.writer.lock().unwrap();
        writer.extend(buf.iter());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn run_handshake(
    client: &mut rustls::ClientConnection,
    server: &mut rustls::ServerConnection,
    mut client_pipe: Pipe,
    mut server_pipe: Pipe,
) -> Result<(), rustls::Error> {
    for _ in 0..100 {
        if !client.is_handshaking() && !server.is_handshaking() {
            return Ok(());
        }
        let _ = client.write_tls(&mut client_pipe);
        let _ = server.read_tls(&mut server_pipe);
        server.process_new_packets()?;
        let _ = server.write_tls(&mut server_pipe);
        let _ = client.read_tls(&mut client_pipe);
        client.process_new_packets()?;
    }
    Err(rustls::Error::General("handshake did not complete".into()))
}

#[test]
fn test_mtls_handshake_accepts_controller_client() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = make_ca();
    let worker = ca.sign_worker("worker-1", 365).unwrap();
    let desktop = ca
        .sign_device("desktop-1", ClusterRole::Admin, 365)
        .unwrap();
    let server_config = Arc::new(
        ca.server_config(
            &worker,
            vec![
                ClusterRole::Owner,
                ClusterRole::Admin,
                ClusterRole::Operator,
            ],
        )
        .unwrap(),
    );
    let client_config = Arc::new(ca.client_config(&desktop, ClusterRole::Worker).unwrap());
    let server_name = "worker-1".try_into().unwrap();
    let mut server = rustls::ServerConnection::new(server_config).unwrap();
    let mut client = rustls::ClientConnection::new(client_config, server_name).unwrap();
    let (client_pipe, server_pipe) = pipe_pair();
    run_handshake(&mut client, &mut server, client_pipe, server_pipe).unwrap();
}

#[test]
fn test_mtls_handshake_rejects_worker_client_on_server() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca = make_ca();
    let worker = ca.sign_worker("worker-1", 365).unwrap();
    let malicious_client = ca.sign_worker("bad-1", 365).unwrap();
    let server_config = Arc::new(
        ca.server_config(
            &worker,
            vec![
                ClusterRole::Owner,
                ClusterRole::Admin,
                ClusterRole::Operator,
            ],
        )
        .unwrap(),
    );
    let client_config = Arc::new(
        ca.client_config(&malicious_client, ClusterRole::Worker)
            .unwrap(),
    );
    let server_name = "worker-1".try_into().unwrap();
    let mut server = rustls::ServerConnection::new(server_config).unwrap();
    let mut client = rustls::ClientConnection::new(client_config, server_name).unwrap();
    let (client_pipe, server_pipe) = pipe_pair();
    assert!(run_handshake(&mut client, &mut server, client_pipe, server_pipe).is_err());
}
