#!/usr/bin/env python3
"""Run one Modal sandbox operation for goble's Rust adapter.

Modal has no public REST API, so goble drives the official Python SDK through
this script. The caller writes one JSON object to stdin and reads one JSON
object from stdout.

Actions
-------
create                 Create a sandbox from a registry image (or the default
                       Debian one). When the payload carries `snapshot_id`, the
                       sandbox filesystem starts as that snapshot instead
                       (`modal.Image.from_id`).
destroy                Terminate a sandbox; already gone is success.
status                 `running` when the sandbox is alive, `missing` when
                       Modal does not know it or it has stopped.
snapshot               Snapshot the sandbox filesystem and print its id
                       (`{"snapshot_id": ...}`); `null` when there is nothing
                       to snapshot.
snapshot_and_terminate Snapshot then terminate in one process — the durable
                       teardown. The snapshot id is printed even though the
                       sandbox is gone; `null` when the sandbox was already
                       gone. A snapshot failure terminates anyway and exits
                       non-zero, so the caller never records a snapshot that
                       was not taken.

Usage: modal_sandbox.py create|destroy|status|snapshot|snapshot_and_terminate
"""

import json
import sys


def _registry_reference(image):
    """Whether `image` names a registry image instead of the default Debian."""
    return bool(image) and any(marker in image for marker in ("/", ":", "@"))


def _sandbox_from_id(sandbox_id):
    """The SDK handle for `sandbox_id`, or None when Modal no longer knows it."""
    import modal

    try:
        return modal.Sandbox.from_id(sandbox_id)
    except modal.exception.NotFoundError:
        return None


def _is_alive(sandbox):
    """Whether `sandbox` is still running.

    A terminated sandbox may still resolve `from_id`; only `poll()` says
    whether its process is alive. An unreachable sandbox is not usable, so the
    errors count as "not alive".
    """
    try:
        return sandbox.poll() is None
    except Exception:  # noqa: BLE001 - an unreachable sandbox is not usable
        return False


def _snapshot_id(sandbox):
    """The image id of a fresh filesystem snapshot of `sandbox`."""
    return sandbox.snapshot_filesystem().object_id


def create(payload):
    import modal

    app = modal.App.lookup(payload["app_name"], create_if_missing=True)
    snapshot_id = (payload.get("snapshot_id") or "").strip()
    image = payload.get("image", "")
    if snapshot_id:
        # A filesystem snapshot carries the worker image and the project's
        # files; it replaces the request's image entirely.
        image_object = modal.Image.from_id(snapshot_id)
    elif _registry_reference(image):
        image_object = modal.Image.from_registry(image)
    else:
        image_object = modal.Image.debian_slim()

    sandbox = modal.Sandbox.create(
        *payload.get("command", []),
        app=app,
        image=image_object,
        env=payload.get("env", {}),
        timeout=payload.get("timeout_secs"),
    )
    print(json.dumps({"sandbox_id": sandbox.object_id}))


def destroy(payload):
    sandbox = _sandbox_from_id(payload["sandbox_id"])
    if sandbox is not None:
        sandbox.terminate()
    print(json.dumps({}))


def status(payload):
    sandbox = _sandbox_from_id(payload["sandbox_id"])
    if sandbox is None or not _is_alive(sandbox):
        print(json.dumps({"status": "missing"}))
    else:
        print(json.dumps({"status": "running"}))


def snapshot(payload):
    sandbox = _sandbox_from_id(payload["sandbox_id"])
    if sandbox is None or not _is_alive(sandbox):
        print(json.dumps({"snapshot_id": None}))
        return
    print(json.dumps({"snapshot_id": _snapshot_id(sandbox)}))


def snapshot_and_terminate(payload):
    """Snapshot, then terminate, whatever the snapshot outcome.

    The terminate is attempted even when the snapshot raises: a tear-down that
    fails to snapshot must not leave the sandbox running. The error propagates
    (non-zero exit, message on stderr) so the caller does not record a snapshot
    id that was never minted.
    """
    sandbox = _sandbox_from_id(payload["sandbox_id"])
    if sandbox is None:
        print(json.dumps({"snapshot_id": None}))
        return
    snapshot_id = None
    snapshot_error = None
    if _is_alive(sandbox):
        try:
            snapshot_id = _snapshot_id(sandbox)
        except Exception as exc:  # noqa: BLE001 - terminated below regardless
            snapshot_error = exc
    try:
        sandbox.terminate()
    except Exception:  # noqa: BLE001 - an already-gone sandbox is fine
        pass
    if snapshot_error is not None:
        raise snapshot_error
    print(json.dumps({"snapshot_id": snapshot_id}))


ACTIONS = {
    "create": create,
    "destroy": destroy,
    "status": status,
    "snapshot": snapshot,
    "snapshot_and_terminate": snapshot_and_terminate,
}


def main(argv):
    if len(argv) != 2 or argv[1] not in ACTIONS:
        print(
            "usage: modal_sandbox.py "
            "create|destroy|status|snapshot|snapshot_and_terminate",
            file=sys.stderr,
        )
        return 2
    try:
        payload = json.load(sys.stdin)
        ACTIONS[argv[1]](payload)
    except Exception as exc:  # noqa: BLE001 - one failure report for the caller
        print(f"modal {argv[1]} failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
