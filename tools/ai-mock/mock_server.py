#!/usr/bin/env python3
"""OpenAI-compatible mock server for Rusty Inbox's AI features (tests and the scripted UI check).

    python3 tools/ai-mock/mock_server.py [port]        # default 18765
    RUSTY_INBOX_AI_URL=http://127.0.0.1:18765/v1 rusty-inbox --autotest=...

Endpoints: GET /v1/models, POST /v1/chat/completions (stream or not). The answer is chosen from
tools/ai-mock/replies/*.json: each file holds {"match": "<substring of the system prompt>",
"answer": "<text>"} or a list of them; the first match wins (files in name order). Each feature
owner adds its own reply file, so nobody edits a shared one. Unmatched requests get a short echo.
Every request is appended to $AI_MOCK_LOG (JSON lines) when set, so tests can assert on prompts.
"""
import glob, json, os, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

HERE = os.path.dirname(os.path.abspath(__file__))


def replies():
    out = []
    for f in sorted(glob.glob(os.path.join(HERE, "replies", "*.json"))):
        data = json.load(open(f, encoding="utf-8"))
        out += data if isinstance(data, list) else [data]
    return out


# ---- C2 (organizing) dynamic answers: they echo the message ids found in the prompt ----------------
import re

C2_MESSAGE = re.compile(r'<message id="(\d+)">\n(.*?)\n</message>', re.S)
C2_AUTOMATED = re.compile(r"(noreply|no-reply|ci@|billing@|security@|notices@|trips@|alerts@)", re.I)
C2_BULK = re.compile(r"(hello@|digest@|notes@|newsletter|weekly)", re.I)


def c2_parse(user):
    out = []
    for mid, block in C2_MESSAGE.findall(user):
        head, _, body = block.partition("\nBody:\n")
        fields = dict(l.split(": ", 1) for l in head.split("\n") if ": " in l)
        out.append({"id": int(mid), "from": fields.get("From", ""), "subject": fields.get("Subject", ""), "body": body})
    return out


def c2_kind(m):
    if m["subject"].lower().startswith("invitation"):
        return "invitations"
    if C2_AUTOMATED.search(m["from"]):
        return "notifications"
    if C2_BULK.search(m["from"]):
        return "newsletters"
    return "personal"


def c2_dynamic(system, user):
    """Answers for the prompts of features 11-13 and 15, or None for any other request."""
    if "You sort incoming email" in system:
        labels = []
        for m in c2_parse(user):
            kind = c2_kind(m)
            ask = kind in ("personal", "invitations") and ("?" in m["body"] or "let me know" in m["body"].lower())
            reason = {"notifications": "automated notice", "newsletters": "bulk mailing", "invitations": "event invitation"}.get(
                kind, "asks a direct question" if ask else "personal message, nothing to answer")
            labels.append({"id": m["id"], "category": kind, "needs_reply": ask, "priority": 2 if ask else 1, "reason": reason})
        return json.dumps({"labels": labels})
    if "that the owner of the mailbox SENT" in system:
        msgs = c2_parse(user)
        asks = bool(msgs) and "?" in msgs[0]["body"]
        return json.dumps({"expects_reply": asks, "awaiting": "an answer to the question" if asks else ""})
    if "daily briefing of the owner" in system:
        msgs = c2_parse(user)
        people = [m for m in msgs if c2_kind(m) == "personal"]
        others = [m for m in msgs if c2_kind(m) != "personal"]
        lines = ["## People"]
        for m in people[:8]:
            name = m["from"].split("<")[0].strip() or m["from"]
            lines.append(f"- **{name}**: {m['subject']} [open](mail:{m['id']})")
        asks = [m for m in people if "?" in m["body"]]
        if asks:
            lines += ["", "## Needs your action"]
            for m in asks[:5]:
                lines.append(f"- Answer {m['from'].split('<')[0].strip()} about [{m['subject']}](mail:{m['id']})")
        if others:
            lines += ["", "## Everything else"]
            for kind in ("notifications", "newsletters", "invitations"):
                n = sum(1 for m in others if c2_kind(m) == kind)
                if n:
                    lines.append(f"- {kind.capitalize()}: {n} unread")
        return "\n".join(lines)
    return None


def answer_for(body):
    msgs = body.get("messages", [])
    system = " ".join(m.get("content", "") for m in msgs if m.get("role") == "system")
    user = msgs[-1].get("content", "") if msgs else ""
    dynamic = c2_dynamic(system, user)
    if dynamic is not None:
        return dynamic
    for r in replies():
        # both given: both must match (e.g. one task on one particular message)
        if "match" in r and "match_user" in r:
            if r["match"] in system and r["match_user"] in user:
                return r["answer"]
        elif r.get("match", "\0") in system or r.get("match_user", "\0") in user:
            return r["answer"]
    return "Mock answer to: " + user[:120]


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def send(self, code, obj):
        b = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        if self.path.rstrip("/").endswith("/models"):
            return self.send(200, {"data": [{"id": "mock"}, {"id": "mock-large"}, {"id": "mock-thinker"}]})
        self.send(404, {"error": {"message": "not found"}})

    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        body = json.loads(self.rfile.read(n) or b"{}")
        if os.environ.get("AI_MOCK_LOG"):
            with open(os.environ["AI_MOCK_LOG"], "a", encoding="utf-8") as f:
                f.write(json.dumps(body, ensure_ascii=False) + "\n")
        if not self.path.rstrip("/").endswith("/chat/completions"):
            return self.send(404, {"error": {"message": "not found"}})
        text = answer_for(body)
        model = body.get("model", "mock")
        if not body.get("stream"):
            return self.send(200, {"model": model, "choices": [{"message": {"role": "assistant", "content": text}}],
                                   "usage": {"prompt_tokens": 10, "completion_tokens": len(text) // 4}})
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        step = 12
        for i in range(0, len(text), step):
            chunk = {"model": model, "choices": [{"index": 0, "delta": {"content": text[i:i + step]}}]}
            self.wfile.write(f"data: {json.dumps(chunk, ensure_ascii=False)}\n\n".encode())
            self.wfile.flush()
            time.sleep(float(os.environ.get("AI_MOCK_DELAY", "0.01")))
        usage = {"model": model, "choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": len(text) // 4}}
        self.wfile.write(f"data: {json.dumps(usage)}\n\ndata: [DONE]\n\n".encode())


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18765
    print(f"mock AI on http://127.0.0.1:{port}/v1", flush=True)
    ThreadingHTTPServer(("127.0.0.1", port), H).serve_forever()
