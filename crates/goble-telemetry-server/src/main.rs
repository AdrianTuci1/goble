//! Runs the collector.
//!
//! Configuration, all optional:
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `GOBLE_TELEMETRY_ADDR` | `127.0.0.1:8787` | Listen address. Put a TLS terminator in front; this process speaks plain HTTP. |
//! | `GOBLE_TELEMETRY_DATA` | `./telemetry-data` | Where reports and events are stored. |
//! | `GOBLE_TELEMETRY_TOKEN` | unset | When set, every ingest and summary request must present it. Set it on anything reachable from the internet. |
//! | `RUST_LOG` | `info` | Log filter. |

use std::net::SocketAddr;
use std::path::PathBuf;

use goble_telemetry_server::{ServerState, Store, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let addr: SocketAddr = std::env::var("GOBLE_TELEMETRY_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
        .parse()?;
    let data = PathBuf::from(
        std::env::var("GOBLE_TELEMETRY_DATA").unwrap_or_else(|_| "./telemetry-data".to_owned()),
    );
    let token = std::env::var("GOBLE_TELEMETRY_TOKEN").ok();

    let state = ServerState::new(Store::new(&data), token);
    if !state.requires_token() {
        tracing::warn!(
            "no GOBLE_TELEMETRY_TOKEN is set: the collector accepts anything that can reach it"
        );
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("collector listening on {addr}, storing in {}", data.display());

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("stopping the collector");
}
