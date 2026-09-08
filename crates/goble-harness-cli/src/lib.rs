//! BYOH seam adaptor: drives an external harness subprocess.
//!
//! This is the "bring your own harness" piece of the `types <- protocol <-
//! runtime` split. [`CliHarness`] wraps an external binary (a CLI, an agent, a
//! tool you already use) that speaks [`goble_harness_protocol`] over stdio, and
//! fronts it as a [`HarnessRuntime`] so the daemon can drive it through the same
//! contract it uses for the internal harness.
//!
//! The subprocess is spawned per turn (one `Run`/`Resume` per process). The
//! harness binary owns its own session state (keyed by [`SessionId`]), so a
//! `Resume` request can re-attach to a prior session the same way a fresh CLI
//! invocation would. Cancellation kills the child process, which ends the event
//! stream; the daemon's own cancel flag is also honored.
//!
//! Dependency direction: daemon → this crate → `goble-harness-{protocol,types,
//! runtime}`. It never depends on `goble-core` or `app/`.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use goble_harness_protocol::{HarnessClientRequest, HarnessMessage, HarnessServerEvent};
use goble_harness_runtime::{HarnessRun, HarnessRuntime};
use goble_harness_types::{HarnessCapabilities, HarnessId, HarnessTurn, SessionId};
use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

/// Configuration for an external harness subprocess adapter.
#[derive(Debug, Clone)]
pub struct CliHarnessConfig {
    /// Identifier the daemon uses to look this harness up.
    pub id: HarnessId,
    /// The command to launch (argv; `command[0]` is the program).
    pub command: Vec<String>,
    /// Capabilities declared for this harness. `tools` may be pre-seeded here,
    /// or left empty if the harness reports tools via `ListTools` on the wire.
    pub capabilities: HarnessCapabilities,
}

impl CliHarnessConfig {
    /// A default capability set for a generic CLI harness (no voice / screen,
    /// sandboxed allow-list, tool list supplied later).
    pub fn new(id: HarnessId, command: Vec<String>) -> Self {
        Self {
            id,
            command,
            capabilities: HarnessCapabilities::internal(),
        }
    }
}

/// A [`HarnessRuntime`] that drives an external binary over goble-harness-protocol.
pub struct CliHarness {
    config: CliHarnessConfig,
    /// The child of the most recent `Run`/`Resume`, so `cancel()` can kill it.
    /// `None` when no turn is currently streaming.
    active: Arc<Mutex<Option<Child>>>,
}

impl CliHarness {
    /// Build the adapter. `config.command` must be non-empty.
    pub fn new(config: CliHarnessConfig) -> anyhow::Result<Self> {
        if config.command.is_empty() {
            anyhow::bail!("goble-harness-cli requires a non-empty command");
        }
        Ok(Self {
            config,
            active: Arc::new(Mutex::new(None)),
        })
    }

    /// Spawn `command`, send `request`, and return a stream of the subprocess's
    /// events until a terminal `Done`/`Error` for `session_id` (or until the
    /// child is cancelled/killed).
    fn drive(
        &self,
        request: HarnessClientRequest,
        session_id: &SessionId,
        cancel: Arc<AtomicBool>,
    ) -> HarnessRun {
        let session_id = session_id.clone();
        if self.config.command.is_empty() {
            return error_stream(session_id, "harness command is empty".into());
        }

        let mut command = Command::new(&self.config.command[0]);
        command
            .args(&self.config.command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(ch) => ch,
            Err(e) => return error_stream(session_id, format!("spawn harness subprocess: {e}")),
        };

        let mut stdin = match child.stdin.take() {
            Some(s) => s,
            None => {
                drop(child);
                return error_stream(session_id, "harness stdin is not piped".into());
            }
        };
        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                drop(child);
                return error_stream(session_id, "harness stdout is not piped".into());
            }
        };
        let stderr = match child.stderr.take() {
            Some(s) => s,
            None => {
                drop(child);
                return error_stream(session_id, "harness stderr is not piped".into());
            }
        };

        // Publish the child so `cancel()` can kill it, then drain stderr.
        *self.active.lock() = Some(child);
        {
            let id = self.config.id.clone();
            tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    log::debug!("[harness {}] {}", id.0, line);
                }
            });
        }

        let request_line = match HarnessMessage::Client(request).to_line() {
            Ok(line) => line,
            Err(e) => {
                self.active.lock().take();
                return error_stream(session_id, format!("encode harness request: {e}"));
            }
        };

        let active = Arc::clone(&self.active);
        let events = async_stream::stream! {
            // Send the request on first poll (pipes are async, so the write
            // happens inside the stream rather than in the sync `run` call).
            if stdin.write_all(request_line.as_bytes()).await.is_err() {
                yield HarnessServerEvent::Error {
                    session_id: session_id.clone(),
                    message: "failed to write request to harness stdin".into(),
                };
                active.lock().take();
                return;
            }
            if stdin.flush().await.is_err() {
                yield HarnessServerEvent::Error {
                    session_id: session_id.clone(),
                    message: "failed to flush harness stdin".into(),
                };
                active.lock().take();
                return;
            }

            let mut lines = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(msg) = HarnessMessage::from_line(&line) else {
                    continue;
                };
                if let HarnessMessage::Server(event) = msg {
                    let is_terminal = matches!(
                        &event,
                        HarnessServerEvent::Done { session_id: sid }
                            | HarnessServerEvent::Error { session_id: sid, .. }
                            if sid == &session_id
                    );
                    yield event;
                    if is_terminal {
                        break;
                    }
                }
            }

            // The stream is over (terminal event, cancel, or EOF): drop the child
            // (killing it via `kill_on_drop`) so no process outlives the turn.
            active.lock().take();
        };

        HarnessRun {
            events: Box::pin(events),
        }
    }
}

impl HarnessRuntime for CliHarness {
    fn id(&self) -> HarnessId {
        self.config.id.clone()
    }

    fn capabilities(&self) -> HarnessCapabilities {
        self.config.capabilities.clone()
    }

    fn run(&self, turn: HarnessTurn, cancel: Arc<AtomicBool>) -> HarnessRun {
        let session_id = turn.session_id.clone();
        self.drive(HarnessClientRequest::Run { turn }, &session_id, cancel)
    }

    fn cancel(&self) {
        if let Some(mut child) = self.active.lock().take() {
            let _ = child.start_kill();
        }
    }

    fn resume(
        &self,
        session_id: &SessionId,
        response: &str,
        credential: Option<(String, String)>,
    ) -> Option<HarnessRun> {
        // The wire `Resume` request does not carry a credential; it is a
        // harness-side concern (e.g. read from its own config/store).
        let _ = credential;
        Some(self.drive(
            HarnessClientRequest::Resume {
                session_id: session_id.clone(),
                response: response.to_string(),
            },
            session_id,
            Arc::new(AtomicBool::new(false)),
        ))
    }
}

/// Build a [`HarnessRun`] that immediately yields one [`HarnessServerEvent::Error`].
fn error_stream(session_id: SessionId, message: String) -> HarnessRun {
    HarnessRun {
        events: Box::pin(futures::stream::once(async move {
            HarnessServerEvent::Error { session_id, message }
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_command_is_rejected() {
        let err = CliHarness::new(CliHarnessConfig::new(HarnessId::new("cli"), vec![]));
        assert!(err.is_err());
    }

    #[test]
    fn capabilities_and_id_come_from_config() {
        let config = CliHarnessConfig::new(HarnessId::new("cli"), vec!["harness-bin".into()]);
        let harness = CliHarness::new(config).unwrap();
        assert_eq!(harness.id(), HarnessId::new("cli"));
        assert_eq!(harness.capabilities(), HarnessCapabilities::internal());
    }

    #[tokio::test]
    async fn missing_binary_emits_an_error_event() {
        // A non-existent command must not panic; it surfaces as an Error event.
        use futures::StreamExt;
        let harness = CliHarness::new(CliHarnessConfig::new(
            HarnessId::new("cli"),
            vec!["/no/such/harness/bin-xyz".into()],
        ))
        .unwrap();
        let turn = HarnessTurn::new(HarnessId::new("cli"), SessionId::new("s1"), "do it");
        // `run` itself is sync; the child spawn failure is reported in the stream.
        let mut run = harness.run(turn, Arc::new(AtomicBool::new(false)));
        let events = run.events.next().await;
        assert!(matches!(events, Some(HarnessServerEvent::Error { .. })));
    }
}
