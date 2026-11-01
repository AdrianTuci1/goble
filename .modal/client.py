#!/usr/bin/env python3
"""Call the deployed `sandboxes` control plane from this machine.

The app is deployed once with `modal deploy .modal/app.py`; it creates a
`modal.Sandbox` per job. This client only invokes its functions.

    ./.modal/sandboxes list
    ./.modal/sandboxes start --label release3 --prompt-file .modal/tasks/release.md
    ./.modal/sandboxes status --label release3
    ./.modal/sandboxes logs --label release3
    ./.modal/sandboxes summary --label release3
    ./.modal/sandboxes stop --label release3
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

import modal

APP_NAME = "sandboxes"
ROOT = pathlib.Path(__file__).resolve().parent
TAIL = ROOT / "grok_tail.py"


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(prog="sandboxes")
    sub = root.add_subparsers(dest="action", required=True)

    start = sub.add_parser("start", help="create the sandbox for one job")
    start.add_argument("--label", required=True)
    start.add_argument("--prompt-file", required=True)
    start.add_argument("--model", default="deepseek")
    start.add_argument("--base-branch", default="terminal-first")
    start.add_argument("--max-turns", type=int, default=240)

    for name in ("status", "summary", "stop"):
        command = sub.add_parser(name)
        command.add_argument("--label", required=True)

    logs = sub.add_parser("logs", help="last transcript lines, rendered")
    logs.add_argument("--label", required=True)
    logs.add_argument("--tail", type=int, default=120)
    logs.add_argument("--raw", action="store_true")

    sub.add_parser("list", help="every job recorded by the app")
    sub.add_parser("profiles", help="what start accepts")
    return root


def call(function: str, **kwargs):
    try:
        return modal.Function.from_name(APP_NAME, function).remote(**kwargs)
    except Exception as exc:
        print(
            f"could not call {APP_NAME}.{function}: {type(exc).__name__}: {exc}\n"
            "deploy the control plane first: modal deploy .modal/app.py",
            file=sys.stderr,
        )
        raise SystemExit(1)


def main() -> None:
    args = parser().parse_args()
    action = args.action

    if action == "start":
        prompt = pathlib.Path(args.prompt_file).read_text()
        if not prompt.strip():
            raise SystemExit(f"{args.prompt_file} is empty")
        result = call(
            "sandbox_start",
            label=args.label,
            prompt=prompt,
            model=args.model,
            base_branch=args.base_branch,
            max_turns=args.max_turns,
        )
    elif action == "status":
        result = call("sandbox_status", label=args.label)
    elif action == "logs":
        result = call("sandbox_logs", label=args.label, tail=args.tail)
        if result.get("ok") and not args.raw:
            text = "\n".join(result.pop("tail")) + "\n"
            rendered = subprocess.run(
                [sys.executable, str(TAIL), "-"],
                input=text,
                capture_output=True,
                text=True,
            ).stdout
            result["rendered"] = rendered
    elif action == "summary":
        result = call("sandbox_summary", label=args.label)
    elif action == "stop":
        result = call("sandbox_stop", label=args.label)
    elif action == "list":
        result = call("sandbox_list")
    elif action == "profiles":
        result = call("sandbox_profiles")
    else:
        raise SystemExit(f"unknown action {action}")

    print(json.dumps(result, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
