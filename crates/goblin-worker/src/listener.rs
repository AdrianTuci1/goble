use std::io;
use std::sync::Arc;

use axum::serve::Listener;
use rustls::ServerConfig;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

/// A [`Listener`] that performs a TLS handshake on each accepted TCP
/// connection before handing the stream to axum. This lets the worker use
/// axum's `ConnectInfo` mechanism to extract the mTLS peer certificate in the
/// WebSocket handler.
pub struct TlsListener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
}

impl TlsListener {
    pub fn new(tcp: TcpListener, config: Arc<ServerConfig>) -> Self {
        Self {
            tcp,
            acceptor: TlsAcceptor::from(config),
        }
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (tcp, addr) = match self.tcp.accept().await {
                Ok(tup) => tup,
                Err(e) => {
                    tracing::error!("tcp accept failed: {e}");
                    continue;
                }
            };
            match self.acceptor.accept(tcp).await {
                Ok(tls) => return (tls, addr),
                Err(e) => {
                    tracing::error!("tls handshake failed from {addr}: {e}");
                    continue;
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}
