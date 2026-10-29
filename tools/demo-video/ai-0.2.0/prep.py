#!/usr/bin/env python3
"""prep.py: fresh demo data for the 0.2.0 AI video (throwaway dir, never the owner's data).

    python3 prep.py            # seed mailbox, settings, ai_state, borrowed ChatGPT sign-in (no refresh)
    python3 prep.py cleanup    # delete secrets.sqlite (+wal/shm) of the demo dir

The sign-in is copied from the owner's secret store READ-ONLY; refresh_token is blanked and the
email replaced, so the demo app can neither rotate the real token nor show a real address.
Nothing secret is ever printed.
"""
import json, os, shutil, sqlite3, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
MEDIA = os.path.expanduser("~/side projects/rusty-inbox-video-media")
D = os.environ.get("DEMO_DIR", f"{MEDIA}/demo-data")
CORE = f"{D}/core-data"
DBS = f"{CORE}/databases"


def cleanup():
    for f in os.listdir(DBS) if os.path.isdir(DBS) else []:
        if f.startswith("secrets.sqlite"):
            os.remove(f"{DBS}/{f}")
    print("demo secrets removed")


if len(sys.argv) > 1 and sys.argv[1] == "cleanup":
    cleanup(); sys.exit()

env = dict(os.environ, DEMO_DIR=D, XVFB=os.path.expanduser("~/.local/opt/xvfb/usr/bin/Xvfb"),
           LD_LIBRARY_PATH=os.path.expanduser("~/.local/opt/xvfb/usr/lib"))
subprocess.run([f"{ROOT}/tools/demo-video/demo-env.sh", "stop"], env=env, stdout=subprocess.DEVNULL, check=False)
for p in ("core-data", "theme", "shots"):
    shutil.rmtree(f"{D}/{p}", ignore_errors=True)
subprocess.run(["cargo", "run", "-q", "--release", "-p", "rusty-inbox-core", "--example", "seed_demo", "--", CORE],
               cwd=ROOT, stdin=subprocess.DEVNULL, check=True, timeout=900)

db = sqlite3.connect(f"{DBS}/rusty-inbox.sqlite")
S = {"aiEnabled": True, "aiProvider": "codex", "aiModel": os.environ.get("AI_MODEL", "gpt-6-luna"),
     "aiThinking": os.environ.get("AI_THINKING", "low"), "aiLanguage": "en", "aiComposeTools": True,
     "aiReadTools": True, "aiNeedsReply": True, "aiFollowUps": True, "aiSearch": True, "aiBriefing": True,
     "aiFollowUpDays": 3, "themeMode": "dark", "notifications": False}
for k, v in S.items():
    db.execute("INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
               (f"rustyInbox.{k}", json.dumps(v)))
for k in ("filter", "needs_reply", "follow_ups", "briefing"):
    db.execute("INSERT INTO ai_state(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
               (f"since:{k}", "0"))
# the contract attachment is a real (fictional) PDF so "Summarize" works offline
os.makedirs(f"{D}/attachments", exist_ok=True)
shutil.copy(f"{MEDIA}/Q4-contract-draft.pdf", f"{D}/attachments/Q4-contract-draft.pdf")
db.execute("UPDATE attachments SET path=? WHERE filename='Q4-contract-draft.pdf'", (f"{D}/attachments/Q4-contract-draft.pdf",))
db.commit(); db.close()

src = sqlite3.connect("file:" + os.path.expanduser("~/.local/share/rusty-inbox/core-data/databases/secrets.sqlite") + "?mode=ro", uri=True)
row = src.execute("SELECT value FROM secrets WHERE key='ai:codex'").fetchone()
src.close()
tok = json.loads(row[0])
tok["refresh_token"] = ""
tok["email"] = "alex@northwind.dev"
dst = sqlite3.connect(f"{DBS}/secrets.sqlite")
dst.execute("CREATE TABLE IF NOT EXISTS secrets (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
dst.execute("INSERT INTO secrets(key,value) VALUES('ai:codex',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (json.dumps(tok),))
dst.commit(); dst.close()
os.chmod(f"{DBS}/secrets.sqlite", 0o600)
print("token fields:", sorted(tok.keys()), "| expires_in_h:", round((tok.get("expires_at", 0) - time.time()) / 3600, 1))
print("prepared", D)
