#!/usr/bin/env python3
"""Cleanplate HTTP service for the web page in web/ (stdlib only).

GET  /api/health                        -> {"ok": true, "cores": N, "version": "..."}
POST /api/feedback, body = JSON {kind, message, email?, context?} -> {"ok": true, "id": N}  (CORS for the static page)
GET  /api/admin/feedback?kind=&status=  -> {"items": [...], "counts": {...}}   (only through nginx's admin location)
POST /api/admin/feedback/<id>, body = {"status": "new" | "seen" | "done"} or {"delete": true} (spam)
POST /api/clean?name=<file name>, body = the file
     200 + the cleaned file (video -> MP4, still -> PNG); headers X-Cleanplate-Log (URL-quoted), X-Cleanplate-Seconds
     422 + {"log": ...}  no mark found with confidence (nothing written)
     4xx/5xx + {"error": ...}

One job at a time (the host is shared); every upload and result is deleted when the response is sent.
"""
import json
import os
import sqlite3
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
DB = os.environ.get("CLEANPLATE_DB", "/var/lib/cleanplate/feedback.db")
# pages on other origins that may post feedback (the GitHub Pages copy of web/)
CORS = set(filter(None, os.environ.get("CLEANPLATE_CORS", "https://dieplai.github.io").split(",")))
KINDS = {"bug", "idea", "refused", "other"}
STATUSES = {"new", "seen", "done"}


def db():
    c = sqlite3.connect(DB, timeout=10)
    c.row_factory = sqlite3.Row
    c.execute(
        "create table if not exists feedback (id integer primary key, created text not null default"
        " (datetime('now')), kind text not null, message text not null, email text, context text,"
        " status text not null default 'new')"
    )
    return c
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

    def _cors(self):
        origin = self.headers.get("Origin")
        if origin in CORS:
            self.send_header("Access-Control-Allow-Origin", origin)
            self.send_header("Vary", "Origin")

    def end_headers(self):
        self._cors()
        super().end_headers()

    def do_OPTIONS(self):  # CORS preflight for /api/feedback
        self.send_response(204)
        self.send_header("Access-Control-Allow-Methods", "POST")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Access-Control-Max-Age", "86400")
        self.end_headers()

    def _admin(self):
        # nginx sets this header only in the password-protected admin location (and clears it everywhere else)
        return self.headers.get("X-Cleanplate-Admin") == "1"

    def _body_json(self, limit):
        size = int(self.headers.get("Content-Length") or 0)
        if not 0 < size <= limit:
            raise ValueError(f"body must be 1..{limit} bytes")
        return json.loads(self.rfile.read(size))

    def do_GET(self):
        url = urllib.parse.urlsplit(self.path)
        if url.path == "/api/health":
            return self._json(200, {"ok": True, "cores": os.cpu_count(), "version": VERSION})
        if url.path == "/api/admin/feedback" and self._admin():
            q = urllib.parse.parse_qs(url.query)
            where, args = [], []
            for col, allowed in (("kind", KINDS), ("status", STATUSES)):
                v = q.get(col, [""])[0]
                if v in allowed:
                    where.append(f"{col} = ?")
                    args.append(v)
            sql = "select * from feedback" + (" where " + " and ".join(where) if where else "")
            with db() as c:
                items = [dict(r) for r in c.execute(sql + " order by id desc limit 1000", args)]
                counts = {f"{r[0]}/{r[1]}": r[2] for r in c.execute("select kind, status, count(*) from feedback group by 1, 2")}
            for it in items:
                it["context"] = json.loads(it["context"]) if it["context"] else None
            return self._json(200, {"items": items, "counts": counts})
        self._json(404, {"error": "not found"})

    def _feedback(self):
        try:
            b = self._body_json(64 << 10)
            kind, msg = b.get("kind"), str(b.get("message") or "").strip()
            email, ctx = str(b.get("email") or "").strip()[:200], b.get("context")
            if kind not in KINDS or not (msg or ctx) or len(msg) > 5000:
                raise ValueError("kind must be bug, idea, refused or other, and the message at most 5000 characters")
            ctx = json.dumps(ctx, ensure_ascii=False)[:20000] if ctx else None
        except (ValueError, json.JSONDecodeError) as e:
            return self._json(400, {"error": str(e)})
        with db() as c:
            fid = c.execute(
                "insert into feedback (kind, message, email, context) values (?, ?, ?, ?)", (kind, msg, email or None, ctx)
            ).lastrowid
        self._json(200, {"ok": True, "id": fid})

    def _set_status(self, fid):
        try:
            b = self._body_json(1 << 10)
        except (ValueError, json.JSONDecodeError) as e:
            return self._json(400, {"error": str(e)})
        if b.get("delete") is True:
            with db() as c:
                n = c.execute("delete from feedback where id = ?", (fid,)).rowcount
            return self._json(200 if n else 404, {"ok": bool(n)})
        status = b.get("status")
        if status not in STATUSES:
            return self._json(400, {"error": "status must be new, seen or done"})
        with db() as c:
            n = c.execute("update feedback set status = ? where id = ?", (status, fid)).rowcount
        self._json(200 if n else 404, {"ok": bool(n)})

    def do_POST(self):
        path = urllib.parse.urlsplit(self.path).path
        if path == "/api/feedback":
            return self._feedback()
        if path.startswith("/api/admin/feedback/") and self._admin():
            fid = path.rsplit("/", 1)[-1]
            return self._set_status(int(fid)) if fid.isdigit() else self._json(404, {"error": "not found"})
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
