//! `goble-acp` — stdio Agent Client Protocol server backed by the Goble gateway.
//!
//! An ACP client (T3 Code web/desktop/mobile, or any other ACP agent client)
//! launches this binary as its "Local ACP command". stdin/stdout carry
//! newline-delimited JSON-RPC 2.0; stderr carries logs. Prompts are dispatched
//! to the gateway's remote-execution API, so code runs in the Goble sandbox and
//! never on this machine.
//!
//! Configuration is environment-only, so an ACP client can launch the binary
//! with no arguments. See `docs/gateway-acp.md` for the full table.

use std::process::ExitCode;

use goble_acp::AcpConfig;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    // stdout is reserved for ACP protocol frames: every diagnostic goes to
    // stderr, including tracing.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("goble-acp {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if let Some(unknown) = args.first() {
        eprintln!(
            "goble-acp: unknown argument {unknown:?}; configuration is environment-only \
             (see --help)"
        );
        return ExitCode::FAILURE;
    }

    let config = match AcpConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("goble-acp: configuration error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("goble-acp: could not start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(goble_acp::run(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("goble-acp: fatal: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "goble-acp {} — Agent Client Protocol (ACP) bridge for Goble gateway sessions

USAGE
  goble-acp            run the ACP server on stdin/stdout
  goble-acp --version  print the version
  goble-acp --help     print this help

Configuration is environment-only (the ACP client sets it):
  GOBLE_GATEWAY_URL           gateway base URL           (default http://127.0.0.1:8788)
  GOBLE_GATEWAY_TOKEN         bearer token for /api/v1
  GOBLE_GATEWAY_PIN           PIN paired into a token via POST /api/v1/session
  GOBLE_ACP_SESSION_NAME      fixed gateway session name for every ACP session
  GOBLE_ACP_WORKER            worker id to bind gateway sessions to
  GOBLE_ACP_RUN_TIMEOUT_S     run budget sent to the gateway (default 1800)
  GOBLE_ACP_STATE_DIR         where sessions.json is persisted (default ~/.goble/acp)
  GOBLE_ACP_STRICT_SETUP=1    refuse mcpServers/additionalDirectories instead of warning
  GOBLE_ACP_RECONNECT_ATTEMPTS  SSE reconnect attempts after a drop (default 3)
  RUST_LOG                    stderr log filter (default info)

The sandbox behind the gateway is the only place code runs: fs/* and terminal/*
ACP methods are refused, never executed locally. Full documentation:
docs/gateway-acp.md",
        env!("CARGO_PKG_VERSION")
    );
}
