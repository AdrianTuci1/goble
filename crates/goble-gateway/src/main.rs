use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use clap::Parser;
use goble_gateway::{generate_pin, generate_token, hash_pin, Gateway, GatewayConfig, DEFAULT_BIND};
use tracing_subscriber::EnvFilter;

/// Always-on session gateway for Goble workers.
///
/// The listener speaks plain HTTP/WebSocket. When it is reachable beyond the
/// loopback interface, put it behind Tailscale or a TLS reverse proxy.
#[derive(Parser, Debug)]
#[command(name = "goble-gateway", version, about)]
struct Args {
    /// Address to listen on.
    #[arg(long, default_value = DEFAULT_BIND)]
    bind: SocketAddr,

    /// File containing the client PIN. Preferred over --pin.
    #[arg(long, value_name = "FILE", conflicts_with = "pin")]
    pin_file: Option<PathBuf>,

    /// Client PIN. Discouraged: it is visible in shell history and process lists.
    #[arg(long, value_name = "PIN")]
    pin: Option<String>,

    /// Public URL browsers use to reach the gateway (also allowed in Host).
    #[arg(long, value_name = "URL")]
    public_url: Option<String>,

    /// Directory holding the built browser client (index.html and friends).
    /// Defaults to `web` next to the executable, then the crate's `web/`.
    #[arg(long, value_name = "DIR", env = "GOBLE_WEB_DIR")]
    web_dir: Option<PathBuf>,

    /// Extra Host header value accepted by the DNS-rebinding guard. Repeatable.
    #[arg(long = "allow-host", value_name = "HOST")]
    allow_hosts: Vec<String>,

    /// File containing the shared worker token. Generated and printed once if absent.
    #[arg(long, value_name = "FILE")]
    worker_token_file: Option<PathBuf>,

    /// Print the PIN at startup even when it came from --pin or --pin-file.
    #[arg(long)]
    print_pin: bool,

    /// Disable client authentication. Loopback binds only; for local development.
    #[arg(long, conflicts_with_all = ["pin", "pin_file"])]
    no_auth: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    if args.no_auth && !args.bind.ip().is_loopback() {
        bail!("--no-auth is only allowed with a loopback --bind");
    }

    let supplied_pin = match (&args.pin, &args.pin_file) {
        (Some(pin), _) => Some(pin.clone()),
        (None, Some(path)) => Some(read_secret_file(path, "PIN")?),
        (None, None) => None,
    };

    let pin_hash = if args.no_auth {
        None
    } else if let Some(pin) = supplied_pin {
        if pin.trim().is_empty() {
            bail!("the PIN must not be empty");
        }
        if args.print_pin {
            eprintln!("goble-gateway: PIN: {pin}");
        }
        Some(hash_pin(&pin))
    } else if args.bind.ip().is_loopback() {
        let pin = generate_pin();
        eprintln!("goble-gateway: generated PIN: {pin}");
        eprintln!("goble-gateway: only the PIN hash is kept; use --pin-file to choose your own");
        Some(hash_pin(&pin))
    } else {
        bail!(
            "refusing to bind {} without a PIN; pass --pin-file or --pin",
            args.bind
        );
    };

    let (worker_token, generated_token) = match &args.worker_token_file {
        Some(path) => (read_secret_file(path, "worker token")?, false),
        None => (generate_token(), true),
    };
    if generated_token {
        eprintln!("goble-gateway: generated worker token: {worker_token}");
        eprintln!(
            "goble-gateway: workers authenticate with 'Authorization: Bearer <token>'; it is not shown again"
        );
    }

    let mut config = GatewayConfig::new(args.bind, pin_hash, worker_token);
    config.public_url = args.public_url;
    config.allowed_hosts = args.allow_hosts;
    config.web_dir = args.web_dir;
    let gateway = Gateway::new(config)?;
    let bind = gateway.bind_addr();
    tracing::info!(
        "browser client directory: {}",
        gateway.state().web_root().display()
    );

    if !bind.ip().is_loopback() {
        tracing::warn!(
            "listening on non-loopback {bind}: this listener speaks plain HTTP; \
             terminate TLS with Tailscale or a reverse proxy"
        );
    }
    if !gateway.state().requires_auth() {
        tracing::warn!("client authentication is disabled (--no-auth); loopback only");
    }

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("could not bind {bind}"))?;
    let local_addr = listener.local_addr().unwrap_or(bind);
    tracing::info!("goble-gateway listening on http://{local_addr}");

    axum::serve(listener, gateway.router())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn read_secret_file(path: &Path, what: &str) -> anyhow::Result<String> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {what} file {}", path.display()))?;
    let contents = contents.trim();
    if contents.is_empty() {
        bail!("{what} file {} is empty", path.display());
    }
    Ok(contents.to_string())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("stopping goble-gateway");
}
