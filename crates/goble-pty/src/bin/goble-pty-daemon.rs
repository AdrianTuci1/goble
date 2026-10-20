//! `goble-pty-daemon`: run the attachable PTY daemon.
//!
//! `--daemon` re-execs detached (used by [`goble_pty::Client`]); `--foreground`
//! runs in this process, which is what tests and service managers use.

#[cfg(unix)]
fn main() {
    if let Err(error) = run() {
        eprintln!("goble-pty-daemon: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("goble-pty-daemon: Unix domain sockets and ptys are required");
    std::process::exit(1);
}

#[cfg(unix)]
fn run() -> anyhow::Result<()> {
    use std::path::PathBuf;

    use anyhow::bail;

    let mut socket: Option<PathBuf> = None;
    let mut daemon = false;
    let mut new_session = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--daemon") => daemon = true,
            Some("--foreground") => daemon = false,
            Some("--new-session") => new_session = true,
            Some("--socket") => {
                let path = args.next().ok_or_else(|| {
                    anyhow::anyhow!("--socket needs a path")
                })?;
                socket = Some(PathBuf::from(path));
            }
            Some("--help" | "-h") => {
                println!(
                    "usage: goble-pty-daemon [--foreground | --daemon] [--socket PATH]\n\
                     \n\
                     --foreground  run in this process (default)\n\
                     --daemon      detach, with stdio on the log beside the socket\n\
                     --socket      $GOBLE_PTY_SOCKET, else $XDG_RUNTIME_DIR/goble/pty.sock,\n\
                     else $HOME/.goble/pty.sock"
                );
                return Ok(());
            }
            Some(other) => bail!("unknown argument {other}"),
            None => bail!("arguments must be valid UTF-8"),
        }
    }

    let socket = socket.unwrap_or_else(goble_pty::resolve_socket_path);
    if new_session {
        goble_pty::server::detach_session(&socket)
    } else if daemon {
        goble_pty::server::daemonize(&socket)
    } else {
        goble_pty::server::run_foreground(&socket)
    }
}
