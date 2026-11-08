"""Local stand-in for the Slack Web API with invented data, used by docs/demo.tape."""

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs

CHANNELS = [
    {"id": "C01GENERAL", "name": "general", "is_private": False},
    {"id": "C02DEPLOYS", "name": "deploys", "is_private": False},
    {"id": "C03ONCALL1", "name": "oncall", "is_private": False},
    {"id": "G04INFRA01", "name": "infra-private", "is_private": True},
]

USERS = [
    {"id": "U01ALICE01", "name": "alice", "profile": {"real_name": "Alice Martin", "display_name": "alice"}},
    {"id": "U02BOB0001", "name": "bob", "profile": {"real_name": "Bob Chen", "display_name": "bob"}},
    {"id": "U03CAROL01", "name": "carol", "profile": {"real_name": "Carol Diaz", "display_name": "carol"}},
]

HISTORY = {
    "C02DEPLOYS": [
        {"ts": "1762591200.000100", "user": "U01ALICE01", "text": "api v2.14.0 rolling out to staging"},
        {"ts": "1762591500.000200", "user": "U02BOB0001", "text": "staging smoke tests green", "reactions": [{"name": "white_check_mark", "count": 2}]},
        {"ts": "1762592100.000300", "user": "U01ALICE01", "text": "prod rollout started", "reply_count": 3},
        {"ts": "1762593000.000400", "user": "U03CAROL01", "text": "p99 latency up 40ms on checkout, watching"},
    ],
}

SEARCH = [
    {"channel": {"id": "C03ONCALL1", "name": "oncall"}, "ts": "1762505400.000100", "username": "bob", "text": "login page times out on slow networks"},
    {"channel": {"id": "C02DEPLOYS", "name": "deploys"}, "ts": "1762593000.000400", "username": "carol", "text": "p99 latency up 40ms on checkout, watching"},
]


def respond(method, params):
    if method in ("conversations.list", "users.list"):
        key = "channels" if method == "conversations.list" else "members"
        rows = CHANNELS if key == "channels" else USERS
        if key == "channels":
            types = params.get("types", "public_channel").split(",")
            rows = [c for c in rows if ("private_channel" if c["is_private"] else "public_channel") in types]
        return {"ok": True, key: rows, "response_metadata": {"next_cursor": ""}}
    if method == "conversations.history":
        return {"ok": True, "messages": list(reversed(HISTORY.get(params.get("channel"), []))), "has_more": False}
    if method == "search.messages":
        words = params.get("query", "").lower().split()
        hits = [m for m in SEARCH if all(w in m["text"].lower() for w in words)]
        return {"ok": True, "messages": {"matches": hits, "total": len(hits)}}
    if method == "chat.postMessage":
        return {"ok": True, "channel": params.get("channel"), "ts": "1762593600.000500"}
    return {"ok": False, "error": "unknown_method"}


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
        params = {k: v[0] for k, v in parse_qs(body).items()}
        data = json.dumps(respond(self.path.rsplit("/", 1)[-1], params)).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18432
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
