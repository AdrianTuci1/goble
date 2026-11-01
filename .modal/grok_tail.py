#!/usr/bin/env python3
"""Render a `grok --output-format streaming-json` log as readable progress.

Text is printed as it arrives; tool calls and file edits become one line each;
thought deltas and token accounting are dropped. Non-JSON lines pass through.
Pass `-` to read stdin.
"""

import json
import sys

NOISE = {
    "available_commands",
    "usage",
    "thought",
    "thinking",
    "reasoning",
    "content",
}


def _text(event: dict) -> str:
    content = event.get("content") or []
    for item in content:
        inner = item.get("content") if isinstance(item, dict) else None
        if isinstance(inner, dict) and inner.get("type") == "text":
            return inner.get("text") or ""
    return ""


def render(stream) -> None:
    for line in stream:
        line = line.rstrip("\n")
        if not line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            print(line, flush=True)
            continue

        kind = event.get("type")
        if kind == "text":
            print(event.get("data", ""), end="", flush=True)
        elif kind == "tool_call":
            name = event.get("toolName") or "tool"
            title = event.get("title") or ""
            print(f"\n[{name}] {title}".rstrip(), flush=True)
        elif kind == "tool_call_update":
            for item in event.get("content") or []:
                if not isinstance(item, dict):
                    continue
                if item.get("type") == "diff":
                    path = str(item.get("path", ""))
                    path = path.split("/repo/")[-1] if "/repo/" in path else path
                    print(f"  [edit] {path}", flush=True)
                elif item.get("type") == "content":
                    text = _text(item)
                    if text and len(text) < 200:
                        print(f"  {text}", flush=True)
        elif kind == "error":
            print(f"\n[error] {event.get('message', '')}", flush=True)
        elif kind == "end":
            usage = event.get("usage") or {}
            print(
                "\n[turn ended: stop={reason}, turns={turns}, tokens={tokens}]".format(
                    reason=event.get("stopReason"),
                    turns=event.get("num_turns"),
                    tokens=usage.get("total_tokens"),
                ),
                flush=True,
            )
        elif kind in NOISE:
            continue
        else:
            summary = {
                k: v
                for k, v in event.items()
                if k != "type" and not isinstance(v, (dict, list))
            }
            if summary:
                print(f"[{kind}] {json.dumps(summary)[:300]}", flush=True)


def main(argv: list[str]) -> None:
    if len(argv) < 2:
        sys.exit(f"usage: {argv[0]} <log.jsonl|->")
    if argv[1] == "-":
        render(sys.stdin)
        return
    with open(argv[1], errors="replace") as handle:
        render(handle)


if __name__ == "__main__":
    try:
        main(sys.argv)
    except (KeyboardInterrupt, BrokenPipeError):
        pass
