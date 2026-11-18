//! The Modal bridge script, driven against a fake `modal` SDK package.
//!
//! The Rust adapter tests inject a fake [`CommandRunner`], which covers the
//! adapter but not `scripts/modal_sandbox.py` itself. These tests run the real
//! script with the real `python3` and a `modal` package on `PYTHONPATH` that
//! records what the script asked the SDK to do — so the "snapshot, then
//! terminate" order and the create-from-snapshot path are exercised end to end
//! without a Modal account.
//!
//! `python3` is the only requirement; when it is missing the tests report that
//! they were skipped instead of failing, because no Rust code can stand in for
//! the interpreter the adapter ships against.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

/// A minimal stand-in for the Modal SDK: enough surface for the bridge script,
/// with every call appended to `$GOBLE_FAKE_MODAL_LOG` as one JSON line.
const FAKE_MODAL_SDK: &str = r#"
import json
import os

LOG = os.environ.get("GOBLE_FAKE_MODAL_LOG")


def _log(event):
    if LOG:
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(event) + "\n")


class NotFoundError(Exception):
    pass


class _ExceptionModule:
    NotFoundError = NotFoundError


exception = _ExceptionModule()


class App:
    @staticmethod
    def lookup(name, create_if_missing=False):
        _log({"call": "app_lookup", "name": name, "create_if_missing": create_if_missing})
        return App()


class Image:
    @classmethod
    def debian_slim(cls):
        _log({"call": "image", "kind": "debian_slim"})
        return cls()

    @classmethod
    def from_registry(cls, ref):
        _log({"call": "image", "kind": "from_registry", "ref": ref})
        return cls()

    @classmethod
    def from_id(cls, snapshot_id):
        _log({"call": "image", "kind": "from_id", "snapshot_id": snapshot_id})
        return cls()


class _Snapshot:
    def __init__(self, object_id):
        self.object_id = object_id


class Sandbox:
    def __init__(self, object_id, alive=True):
        self.object_id = object_id
        self._alive = alive

    @classmethod
    def create(cls, *command, app=None, image=None, env=None, timeout=None):
        _log({"call": "sandbox_create", "command": list(command), "env": env, "timeout": timeout})
        return cls("sb-created")

    @classmethod
    def from_id(cls, sandbox_id):
        _log({"call": "sandbox_from_id", "sandbox_id": sandbox_id})
        if sandbox_id == "sb-missing":
            raise NotFoundError(sandbox_id)
        return cls(sandbox_id, alive=(sandbox_id != "sb-dead"))

    def poll(self):
        return None if self._alive else 0

    def terminate(self):
        _log({"call": "terminate", "sandbox_id": self.object_id})

    def snapshot_filesystem(self):
        _log({"call": "snapshot_filesystem", "sandbox_id": self.object_id})
        if self.object_id == "sb-snapshot-fails":
            raise RuntimeError("snapshot exploded")
        return _Snapshot("im-fake-1")
"#;

const BRIDGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/modal_sandbox.py");

/// One run of the bridge: what the process did and what the SDK saw.
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    sdk_calls: Vec<Value>,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim()).unwrap_or_else(|error| {
            panic!("bridge stdout is not JSON ({error}): {:?}", self.stdout)
        })
    }

    fn calls(&self) -> Vec<&str> {
        self.sdk_calls
            .iter()
            .map(|call| call["call"].as_str().unwrap_or_default())
            .collect()
    }

    fn first(&self, call: &str) -> &Value {
        self.sdk_calls
            .iter()
            .find(|entry| entry["call"] == call)
            .unwrap_or_else(|| panic!("no {call} in {:?}", self.sdk_calls))
    }
}

struct FakeModal {
    root: tempfile::TempDir,
}

impl FakeModal {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let package = root.path().join("modal");
        std::fs::create_dir_all(&package).expect("fake module dir");
        std::fs::write(package.join("__init__.py"), FAKE_MODAL_SDK).expect("fake module");
        Self { root }
    }

    fn run(&self, action: &str, payload: Value) -> Run {
        let log = self.root.path().join("sdk-calls.jsonl");
        // Each run starts a fresh log, so `sdk_calls` is exactly this run.
        let _ = std::fs::remove_file(&log);
        let mut child = Command::new("python3")
            .arg(BRIDGE)
            .arg(action)
            .env("PYTHONPATH", self.root.path())
            .env("GOBLE_FAKE_MODAL_LOG", &log)
            .env("MODAL_TOKEN_ID", "token-id")
            .env("MODAL_TOKEN_SECRET", "token-secret")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn python3");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(payload.to_string().as_bytes())
            .expect("write payload");
        let output = child.wait_with_output().expect("bridge output");
        let sdk_calls = std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("sdk call json"))
            .collect();
        Run {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            sdk_calls,
        }
    }
}

/// Whether a test can run the bridge at all.
fn python_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

macro_rules! bridge_test {
    ($(async fn $name:ident() $body:block)*) => {
        $(
            #[test]
            fn $name() {
                if !python_available() {
                    eprintln!("skipping {}: python3 is not installed", stringify!($name));
                    return;
                }
                $body
            }
        )*
    };
}

fn create_payload(image: &str, snapshot_id: Option<&str>) -> Value {
    let mut payload = json!({
        "app_name": "goble-goble-2fbfef8fa780",
        "image": image,
        "command": ["goblin"],
        "env": {"GOBLE_WORKER_ID": "worker-1"},
        "timeout_secs": 600,
    });
    if let Some(snapshot_id) = snapshot_id {
        payload["snapshot_id"] = json!(snapshot_id);
    }
    payload
}

fn sandbox_payload(sandbox_id: &str) -> Value {
    json!({ "sandbox_id": sandbox_id })
}

bridge_test! {
    async fn create_uses_a_registry_image_and_prints_the_sandbox_id() {
        let fake = FakeModal::new();
        let run = fake.run(
            "create",
            create_payload("ghcr.io/adrian/goble-worker:latest", None),
        );

        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
        assert_eq!(run.json()["sandbox_id"], "sb-created");
        assert_eq!(run.calls(), vec!["app_lookup", "image", "sandbox_create"]);
        let image = run.first("image");
        assert_eq!(image["kind"], "from_registry");
        assert_eq!(image["ref"], "ghcr.io/adrian/goble-worker:latest");
        let created = run.first("sandbox_create");
        assert_eq!(created["command"], json!(["goblin"]));
        assert_eq!(created["timeout"], 600);
    }

    async fn create_restores_a_snapshot_instead_of_the_request_image() {
        let fake = FakeModal::new();
        let run = fake.run(
            "create",
            create_payload("ghcr.io/adrian/goble-worker:latest", Some("im-previous")),
        );

        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
        assert_eq!(run.json()["sandbox_id"], "sb-created");
        let image = run.first("image");
        assert_eq!(image["kind"], "from_id");
        assert_eq!(image["snapshot_id"], "im-previous");
    }

    async fn snapshot_prints_the_filesystem_snapshot_id() {
        let fake = FakeModal::new();
        let run = fake.run("snapshot", sandbox_payload("sb-live"));

        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
        assert_eq!(run.json(), json!({"snapshot_id": "im-fake-1"}));
        assert_eq!(run.calls(), vec!["sandbox_from_id", "snapshot_filesystem"]);
    }

    async fn snapshot_of_a_sandbox_modal_no_longer_knows_is_none() {
        let fake = FakeModal::new();
        let run = fake.run("snapshot", sandbox_payload("sb-missing"));

        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
        assert_eq!(run.json(), json!({"snapshot_id": null}));
        assert_eq!(run.calls(), vec!["sandbox_from_id"], "nothing to snapshot");
    }

    async fn snapshot_of_a_stopped_sandbox_is_none() {
        let fake = FakeModal::new();
        let run = fake.run("snapshot", sandbox_payload("sb-dead"));

        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
        assert_eq!(run.json(), json!({"snapshot_id": null}));
        assert_eq!(run.calls(), vec!["sandbox_from_id"], "a stopped sandbox cannot be snapshotted");
    }

    async fn snapshot_and_terminate_snapshots_before_it_terminates() {
        let fake = FakeModal::new();
        let run = fake.run("snapshot_and_terminate", sandbox_payload("sb-live"));

        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
        assert_eq!(run.json(), json!({"snapshot_id": "im-fake-1"}));
        assert_eq!(
            run.calls(),
            vec!["sandbox_from_id", "snapshot_filesystem", "terminate"],
            "the snapshot must be taken while the sandbox is still alive"
        );
    }

    async fn snapshot_and_terminate_of_a_missing_sandbox_is_none() {
        let fake = FakeModal::new();
        let run = fake.run("snapshot_and_terminate", sandbox_payload("sb-missing"));

        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
        assert_eq!(run.json(), json!({"snapshot_id": null}));
        assert_eq!(run.calls(), vec!["sandbox_from_id"], "there is nothing to terminate");
    }

    async fn a_failed_snapshot_terminates_anyway_and_exits_nonzero() {
        let fake = FakeModal::new();
        let run = fake.run("snapshot_and_terminate", sandbox_payload("sb-snapshot-fails"));

        // No fake success: the caller must not record a snapshot that was not
        // taken, and the sandbox must not be left running.
        assert_eq!(run.code, Some(1));
        assert!(run.stdout.trim().is_empty(), "no JSON on a failed snapshot");
        assert!(run.stderr.contains("snapshot exploded"), "{:?}", run.stderr);
        assert_eq!(
            run.calls(),
            vec!["sandbox_from_id", "snapshot_filesystem", "terminate"],
            "the terminate happens even though the snapshot failed"
        );
    }

    async fn status_reports_running_and_missing() {
        let fake = FakeModal::new();
        let running = fake.run("status", sandbox_payload("sb-live"));
        assert_eq!(running.json(), json!({"status": "running"}));

        let fake = FakeModal::new();
        let missing = fake.run("status", sandbox_payload("sb-missing"));
        assert_eq!(missing.json(), json!({"status": "missing"}));

        let stopped = fake.run("status", sandbox_payload("sb-dead"));
        assert_eq!(stopped.json(), json!({"status": "missing"}));
    }

    async fn destroy_terminates_and_treats_a_missing_sandbox_as_done() {
        let fake = FakeModal::new();
        let run = fake.run("destroy", sandbox_payload("sb-live"));
        assert_eq!(run.code, Some(0), "{:?}", run.stderr);
        assert_eq!(run.calls(), vec!["sandbox_from_id", "terminate"]);

        let missing = fake.run("destroy", sandbox_payload("sb-missing"));
        assert_eq!(missing.code, Some(0), "{:?}", missing.stderr);
        assert_eq!(missing.json(), json!({}));
        assert_eq!(missing.calls(), vec!["sandbox_from_id"]);
    }

    async fn an_unknown_action_is_refused() {
        let fake = FakeModal::new();
        let run = fake.run("teleport", json!({}));
        assert_eq!(run.code, Some(2));
        assert!(run.stderr.contains("usage:"), "{:?}", run.stderr);
    }
}

/// The adapter's default bridge path must be the script these tests run.
#[test]
fn the_bridge_these_tests_drive_is_the_one_the_adapter_ships() {
    let path = Path::new(BRIDGE);
    assert!(path.is_file(), "missing {}", path.display());
    let source = std::fs::read_to_string(path).expect("read the bridge");
    for action in [
        "snapshot",
        "snapshot_and_terminate",
        "create",
        "destroy",
        "status",
    ] {
        assert!(
            source.contains(&format!("{action}\"")),
            "the bridge does not register {action}"
        );
    }
}
