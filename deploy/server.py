#!/usr/bin/env python3
"""Cleanplate HTTP service for the web page in web/ (stdlib only).

GET  /api/health                        -> {"ok": true, "cores": N, "version": "..."}
POST /api/clean?name=<file name>, body = the file
     200 + the cleaned file (video -> MP4, still -> PNG); headers X-Cleanplate-Log (URL-quoted), X-Cleanplate-Seconds
     422 + {"log": ...}  no mark found with confidence (nothing written)
     4xx/5xx + {"error": ...}

One job at a time (the host is shared); every upload and result is deleted when the response is sent.
"""
import json
import os
import subprocess
import tempfile
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

BIN = os.environ.get("CLEANPLATE_BIN", "/opt/cleanplate/cleanplate")
PORT = int(os.environ.get("CLEANPLATE_PORT", "8095"))
MAX_BYTES = int(os.environ.get("CLEANPLATE_MAX_MB", "95")) << 20  # Cloudflare free plan caps uploads at 100 MB
TIMEOUT = int(os.environ.get("CLEANPLATE_TIMEOUT", "95"))  # Cloudflare drops a response after 100 s
QUEUE_WAIT = 60
VIDEO = {"mp4", "mov", "m4v", "mkv", "webm"}
STILL = {"png", "jpg", "jpeg", "webp"}
JOB = threading.Semaphore(1)
VERSION = subprocess.run([BIN, "--version"], capture_output=True, text=True).stdout.strip()


class Handler(BaseHTTPRequestHandler):
    server_version = "cleanplate"

    def _json(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if urllib.parse.urlsplit(self.path).path == "/api/health":
            return self._json(200, {"ok": True, "cores": os.cpu_count(), "version": VERSION})
        self._json(404, {"error": "not found"})

    def do_POST(self):
        try:
            self._clean()
        except (BrokenPipeError, ConnectionResetError):
            pass  # the client went away (closed tab, cancelled upload); the temp dir is already gone

    def _clean(self):
        url = urllib.parse.urlsplit(self.path)
        if url.path != "/api/clean":
            return self._json(404, {"error": "not found"})
        name = urllib.parse.parse_qs(url.query).get("name", ["file"])[0]
        ext = name.rsplit(".", 1)[-1].lower() if "." in name else ""
        if ext not in VIDEO | STILL:
            return self._json(400, {"error": f"unsupported file type .{ext}"})
        size = int(self.headers.get("Content-Length") or 0)
        if size <= 0:
            return self._json(411, {"error": "empty upload"})
        if size > MAX_BYTES:
            return self._json(413, {"error": f"file larger than {MAX_BYTES >> 20} MB"})
        if not JOB.acquire(timeout=QUEUE_WAIT):
            return self._json(503, {"error": "the server is busy with another file, try again in a minute"})
        try:
            with tempfile.TemporaryDirectory(prefix="cleanplate-") as d:
                inp = os.path.join(d, "in." + ext)
                out = os.path.join(d, "out." + ("mp4" if ext in VIDEO else "png"))
                left = size
                with open(inp, "wb") as f:
                    while left:
                        chunk = self.rfile.read(min(left, 1 << 20))
                        if not chunk:
                            return self._json(400, {"error": "upload interrupted"})
                        f.write(chunk)
                        left -= len(chunk)
                t0 = time.monotonic()
                try:
                    p = subprocess.run([BIN, inp, out], capture_output=True, text=True, timeout=TIMEOUT)
                except subprocess.TimeoutExpired:
                    return self._json(504, {"error": f"took longer than {TIMEOUT} s, use the CLI for long clips"})
                secs = time.monotonic() - t0
                log = (p.stdout + p.stderr).strip()
                if p.returncode == 2:
                    return self._json(422, {"log": log})
                if p.returncode != 0 or not os.path.exists(out):
                    return self._json(500, {"error": log.splitlines()[-1] if log else "processing failed"})
                self.send_response(200)
                self.send_header("Content-Type", "video/mp4" if out.endswith(".mp4") else "image/png")
                self.send_header("Content-Length", str(os.path.getsize(out)))
                self.send_header("X-Cleanplate-Log", urllib.parse.quote(log[-3000:]))
                self.send_header("X-Cleanplate-Seconds", f"{secs:.1f}")
                self.end_headers()
                with open(out, "rb") as f:
                    while chunk := f.read(1 << 20):
                        self.wfile.write(chunk)
        finally:
            JOB.release()

    def log_message(self, fmt, *args):  # one line per request, no file names
        print(f"{self.address_string()} {self.command} {urllib.parse.urlsplit(self.path).path} {args[1] if len(args) > 1 else ''}", flush=True)


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
