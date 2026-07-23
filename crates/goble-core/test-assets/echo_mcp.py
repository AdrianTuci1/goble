#!/usr/bin/env python3
"""Minimal line-oriented JSON-RPC MCP echo server for tests.

Reads newline-delimited JSON-RPC requests from stdin and writes
newline-delimited responses to stdout. Supports:

- initialize -> returns protocol info
- tools/list -> returns one mock tool
- error      -> returns a JSON-RPC error
- any other method -> echoes the params back
"""

import sys
import json


def send(msg: dict) -> None:
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            send({
                "jsonrpc": "2.0",
                "id": None,
                "error": {"code": -32700, "message": "parse error"},
            })
            continue

        req_id = req.get("id")
        method = req.get("method")

        if method == "initialize":
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "serverInfo": {"name": "echo", "version": "1.0"},
                    "capabilities": {},
                },
            })
        elif method == "tools/list":
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "tools": [
                        {
                            "name": "echo",
                            "description": "echoes input",
                            "inputSchema": {"type": "object"},
                        }
                    ]
                },
            })
        elif method == "error":
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32600,
                    "message": "intentional error",
                },
            })
        else:
            send({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": req.get("params"),
            })


if __name__ == "__main__":
    main()
