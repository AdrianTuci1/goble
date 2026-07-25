#!/usr/bin/env python3
import http.server
import socketserver
import json
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 11434

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_POST(self):
        if self.path == "/api/chat":
            length = int(self.headers.get('Content-Length', 0))
            body = self.rfile.read(length)
            data = json.loads(body)
            model = data.get("model", "")
            messages = data.get("messages", [])
            prompt = " ".join(m.get("content", "") for m in messages)
            stream = data.get("stream", True)
            self.send_response(200)
            self.send_header("Content-Type", "application/x-ndjson")
            self.end_headers()
            if stream:
                chunks = [
                    {"message": {"role": "assistant", "content": "Hello "}},
                    {"message": {"role": "assistant", "content": "from "}},
                    {"message": {"role": "assistant", "content": model}},
                    {"message": {"role": "assistant", "content": ". "}},
                    {"message": {"role": "assistant", "content": "Prompt: "}},
                    {"message": {"role": "assistant", "content": prompt}},
                    {"done": True},
                ]
                for chunk in chunks:
                    self.wfile.write((json.dumps(chunk) + "\n").encode())
                    self.wfile.flush()
            else:
                resp = {"response": f"Hello from {model}. Prompt: {prompt}", "done": True}
                self.wfile.write(json.dumps(resp).encode())
        else:
            self.send_response(404)
            self.end_headers()

if __name__ == "__main__":
    with socketserver.TCPServer(("127.0.0.1", PORT), Handler) as httpd:
        print(f"serving on {PORT}", flush=True)
        httpd.serve_forever()
