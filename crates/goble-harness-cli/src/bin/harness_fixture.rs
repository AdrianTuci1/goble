//! A deterministic proto-speaking subprocess used by the integration tests.
//!
//! It reads `HarnessMessage::Client` frames off stdin and answers each request
//! with `HarnessMessage::Server` frames, so `goble-harness-cli` can drive it
//! like a real external harness. Behaviour is selected by the first argv:
//!   - `happy`   (default): `Run` streams a delta then `Done` and exits.
//!   - `suspend`: `Run` streams a delta then waits (until killed or `Cancel`).
//!   - `resume`:  `Resume` streams a delta echoing the response then `Done`.

use goble_harness_protocol::{HarnessClientRequest, HarnessMessage, HarnessServerEvent, ToolSchemaWire};
use goble_harness_types::HarnessSnapshot;
use std::io::{BufRead, Write};

fn emit(out: &mut impl Write, msg: HarnessMessage) {
    if let Ok(line) = msg.to_line() {
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "happy".to_string());
    let stdin = std::io::stdin();
    let mut lock = stdin.lock();
    let mut out = std::io::stdout().lock();

    loop {
        let mut line = String::new();
        match lock.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let Ok(msg) = HarnessMessage::from_line(line.trim()) else {
            continue;
        };

        match msg {
            HarnessMessage::Client(HarnessClientRequest::ListTools) => {
                emit(
                    &mut out,
                    HarnessMessage::Server(HarnessServerEvent::Ready),
                );
                emit(
                    &mut out,
                    HarnessMessage::Server(HarnessServerEvent::ToolList {
                        tools: vec![ToolSchemaWire {
                            name: "fixture_tool".into(),
                            description: "a deterministic fixture tool".into(),
                            parameters: serde_json::json!({ "type": "object" }),
                        }],
                    }),
                );
            }
            HarnessMessage::Client(HarnessClientRequest::Run { turn }) => {
                let sid = turn.session_id;
                emit(
                    &mut out,
                    HarnessMessage::Server(HarnessServerEvent::AssistantDelta {
                        session_id: sid.clone(),
                        delta: "from-fixture".into(),
                    }),
                );
                if mode == "suspend" {
                    // Stay alive until the host kills us or sends a Cancel.
                } else {
                    emit(&mut out, HarnessMessage::Server(HarnessServerEvent::Done { session_id: sid }));
                    break;
                }
            }
            HarnessMessage::Client(HarnessClientRequest::Resume { session_id, response }) => {
                emit(
                    &mut out,
                    HarnessMessage::Server(HarnessServerEvent::AssistantDelta {
                        session_id: session_id.clone(),
                        delta: response,
                    }),
                );
                emit(&mut out, HarnessMessage::Server(HarnessServerEvent::Done { session_id }));
                break;
            }
            HarnessMessage::Client(HarnessClientRequest::Cancel { session_id }) => {
                emit(
                    &mut out,
                    HarnessMessage::Server(HarnessServerEvent::Error {
                        session_id,
                        message: "cancelled".into(),
                    }),
                );
                break;
            }
            HarnessMessage::Client(HarnessClientRequest::Checkpoint { session_id }) => {
                emit(
                    &mut out,
                    HarnessMessage::Server(HarnessServerEvent::Checkpoint {
                        session_id,
                        snapshot: HarnessSnapshot::new(
                            "fixture",
                            serde_json::json!({ "ok": true }),
                        ),
                    }),
                );
            }
            HarnessMessage::Client(HarnessClientRequest::Restore { session_id, snapshot }) => {
                emit(
                    &mut out,
                    HarnessMessage::Server(HarnessServerEvent::AssistantDelta {
                        session_id: session_id.clone(),
                        delta: format!("restored:{}", snapshot.kind),
                    }),
                );
                emit(&mut out, HarnessMessage::Server(HarnessServerEvent::Done { session_id }));
                break;
            }
            // A harness is a server; it never receives `Server` frames from the
            // host, so ignore them to keep the match exhaustive.
            HarnessMessage::Server(_) => {}
        }
    }
}
