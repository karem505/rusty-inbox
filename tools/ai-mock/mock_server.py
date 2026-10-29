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


def answer_for(body):
    msgs = body.get("messages", [])
    system = " ".join(m.get("content", "") for m in msgs if m.get("role") == "system")
    user = msgs[-1].get("content", "") if msgs else ""
    for r in replies():
        if r.get("match", "\0") in system or r.get("match_user", "\0") in user:
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
