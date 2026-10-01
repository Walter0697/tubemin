#!/usr/bin/env python3
import http.client
import json
import os
import signal
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import urlopen

TOKEN = os.environ.get("METUBE_MAINTENANCE_TOKEN", "")
PORT = int(os.environ.get("METUBE_MAINTENANCE_PORT", "8082"))
REQUEST_FILE = "/tmp/metube-update-request"
PID_FILE = "/tmp/metube-app.pid"


def active_downloads():
    try:
        with urlopen("http://127.0.0.1:8081/history", timeout=3) as response:
            data = json.load(response)
        return bool(data.get("queue") or data.get("pending"))
    except Exception:
        return True


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        return

    def send_json(self, status, body):
        payload = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self):
        if self.path == "/maintenance/health":
            self.send_json(200, {"status": "ok"})
            return
        self.send_json(404, {"error": "not found"})

    def do_POST(self):
        if self.path != "/maintenance/update":
            self.send_json(404, {"error": "not found"})
            return
        if not TOKEN or self.headers.get("Authorization") != f"Bearer {TOKEN}":
            self.send_json(401, {"error": "unauthorized"})
            return
        if active_downloads():
            self.send_json(409, {"error": "downloads are active"})
            return
        try:
            with open(PID_FILE) as handle:
                pid = int(handle.read().strip())
            open(REQUEST_FILE, "w").close()
            os.kill(pid, signal.SIGTERM)
        except (FileNotFoundError, ValueError, ProcessLookupError):
            self.send_json(503, {"error": "metube is not ready"})
            return
        self.send_json(202, {"status": "updating"})


ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
