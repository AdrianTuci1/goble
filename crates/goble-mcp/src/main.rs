//! `goble-mcp [--socket PATH]`: the MCP stdio server.
//!
//! stdout carries JSON-RPC only; diagnostics go to stderr so an MCP client's
//! framing is never corrupted.

use std::error::Error;
use std::path::PathBuf;

#[cfg(unix)]
fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("goble-mcp: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("goble-mcp: the Goble control protocol is Unix-only");
    std::process::exit(1);
}

#[cfg(unix)]
fn run() -> Result<(), Box<dyn Error>> {
    match parse_args(std::env::args().skip(1))? {
        Invocation::Help => {
            println!("{USAGE}");
            Ok(())
        }
        Invocation::Version => {
            println!("goble-mcp {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Invocation::Run(socket) => {
            let control = goble_mcp::SocketControl::new(socket);
            goble_mcp::serve(
                control,
                std::io::stdin().lock(),
                std::io::stdout().lock(),
            )?;
            Ok(())
        }
    }
}

const USAGE: &str = "\
goble-mcp — Model Context Protocol tools for a running Goble window

Usage: goble-mcp [--socket PATH]

Options:
  --socket PATH   Control socket to call (default: $GOBLE_CONTROL_SOCKET,
                  else $HOME/.goble/control.sock)
  -h, --help      Print this help
  -V, --version   Print the version

The server speaks newline-delimited JSON-RPC 2.0 on stdin/stdout.";

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
enum Invocation {
    Run(PathBuf),
    Help,
    Version,
}

#[cfg(unix)]
fn parse_args(
    args: impl IntoIterator<Item = String>,
) -> Result<Invocation, Box<dyn Error>> {
    let mut socket = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Invocation::Help),
            "-V" | "--version" => return Ok(Invocation::Version),
            "--socket" => {
                let value = args.next().ok_or("--socket needs a path")?;
                socket = Some(PathBuf::from(value));
            }
            other if other.starts_with("--socket=") => {
                let value = &other["--socket=".len()..];
                if value.is_empty() {
                    return Err("--socket needs a path".into());
                }
                socket = Some(PathBuf::from(value));
            }
            other => return Err(format!("unknown argument `{other}`; try --help").into()),
        }
    }
    Ok(Invocation::Run(
        socket.unwrap_or_else(goble_control::resolve_socket_path),
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn a_socket_can_be_named_either_way() {
        assert_eq!(
            parse_args(args(&["--socket", "/tmp/a.sock"])).unwrap(),
            Invocation::Run(PathBuf::from("/tmp/a.sock"))
        );
        assert_eq!(
            parse_args(args(&["--socket=/tmp/b.sock"])).unwrap(),
            Invocation::Run(PathBuf::from("/tmp/b.sock"))
        );
    }

    #[test]
    fn help_and_version_win_over_a_socket() {
        assert_eq!(parse_args(args(&["--help"])).unwrap(), Invocation::Help);
        assert_eq!(parse_args(args(&["--version"])).unwrap(), Invocation::Version);
    }

    #[test]
    fn an_unknown_argument_is_an_error() {
        assert!(parse_args(args(&["--wat"])).is_err());
        assert!(parse_args(args(&["--socket"])).is_err());
    }
}
