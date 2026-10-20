#!/usr/bin/env python3
"""Run one Modal sandbox operation for goble's Rust adapter.

Modal has no public REST API, so goble drives the official Python SDK through
this script. The caller writes one JSON object to stdin and reads one JSON
object from stdout.

Usage: modal_sandbox.py create|destroy|status
"""

import json
import sys


def _registry_reference(image):
    """Whether `image` names a registry image instead of the default Debian."""
    return bool(image) and any(marker in image for marker in ("/", ":", "@"))


def create(payload):
    import modal

    app = modal.App.lookup(payload["app_name"], create_if_missing=True)
    image = payload.get("image", "")
    if _registry_reference(image):
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
    import modal

    modal.Sandbox.from_id(payload["sandbox_id"]).terminate()
    print(json.dumps({}))


def status(payload):
    import modal

    try:
        modal.Sandbox.from_id(payload["sandbox_id"])
    except modal.exception.NotFoundError:
        print(json.dumps({"status": "missing"}))
    else:
        print(json.dumps({"status": "running"}))


ACTIONS = {"create": create, "destroy": destroy, "status": status}


def main(argv):
    if len(argv) != 2 or argv[1] not in ACTIONS:
        print("usage: modal_sandbox.py create|destroy|status", file=sys.stderr)
        return 2
    payload = json.load(sys.stdin)
    ACTIONS[argv[1]](payload)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
