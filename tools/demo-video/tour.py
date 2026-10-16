"""Scripted feature tour of Rusty Inbox on the demo mailbox (private display :78).

    python3 tour.py dry     # run with checkpoint screenshots (shots/), no video
    python3 tour.py record  # record the whole tour to raw.mkv (app launched by the script)
"""
import os
import shutil
import subprocess
import sys
import time

from xd import X

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
D = os.environ.get("DEMO_DIR", os.path.join(ROOT, "target", "demo"))
HERE = os.path.dirname(os.path.abspath(__file__))
THEMES = os.environ.get("THEMES", "/usr/share/omarchy/themes")
MODE = sys.argv[1] if len(sys.argv) > 1 else "dry"
os.makedirs(f"{D}/shots", exist_ok=True)
x = X(":78")
n = 0


def shot(name):
    global n
    if MODE == "dry":
        n += 1
        x.shot(f"{D}/shots/{n:02d}-{name}.png")


def wait(s):
    time.sleep(s)


T_REC = [None]
MARKS = []


def mark(label):
    """Caption boundary: seconds since the recording started."""
    if T_REC[0] is not None:
        MARKS.append((round(time.time() - T_REC[0], 2), label))


def theme(name):
    shutil.copy(f"{THEMES}/{name}/colors.toml", f"{D}/theme/colors.toml")


# ---------------------------------------------------------------- launch
x.warp(1180, 640)
rec = None
if MODE == "record":
    rec = subprocess.Popen(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "1", "-framerate", "60",
         "-video_size", "1920x1080", "-i", ":78", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "12",
         "-pix_fmt", "yuv420p", f"{D}/raw.mkv"],
        stdin=subprocess.PIPE)
    T_REC[0] = time.time()
    wait(1.2)
t0 = time.time()
mark("Opens instantly")
subprocess.run([f"{HERE}/demo-env.sh", "app"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
print("launched", flush=True)
wait(2.6)
shot("launched")

mark("Smart Inbox: people first")
# ---------------------------------------------------------------- smart inbox: hover rows
for y in (282, 376, 471, 566):
    x.move(1150, y, 0.45)
    wait(0.35)
wait(0.4)
shot("hover")

mark("One-click categories")
# ---------------------------------------------------------------- categories in the sidebar
for y in (120, 160, 201):
    x.click(110, y)
    wait(1.3)
shot("newsletters")
x.click(90, 78)            # Smart Inbox: all accounts, unfolds them
wait(1.4)
x.click(90, 78)            # fold
wait(1.0)

mark("Automated mail folds away")
# ---------------------------------------------------------------- smart groups fold and unfold
x.click(560, 213)          # fold People
wait(1.1)
shot("people-folded")
x.click(560, 487)          # unfold Notifications
wait(1.6)
shot("notifications-open")
x.click(560, 487)          # fold Notifications
wait(0.9)
x.click(560, 213)          # unfold People
wait(1.2)

mark("Full HTML email, threaded")
# ---------------------------------------------------------------- reader + keyboard
x.click(820, 282)          # Maya: 3-message thread
wait(2.4)
shot("reader")
x.scroll(7, 1400, 700)
wait(1.4)
x.scroll(-7)
wait(0.8)
mark("Keyboard first: J / K")
x.move(1500, 960, 0.5)
for k in ("j", "j", "k"):
    x.key(k)
    wait(1.5)
shot("keyboard-jonas")
mark("Pin with P")
x.key("p")                 # pin Jonas
wait(1.8)
shot("pinned")

mark("Done, with undo")
# ---------------------------------------------------------------- done + undo
x.click(983, 30)           # Done from the reader toolbar
wait(1.4)
shot("done-toast")
x.key("z")                 # undo
wait(1.6)
shot("undone")

mark("Snooze it for later")
# ---------------------------------------------------------------- snooze
x.click(1068, 30)
wait(0.9)
shot("snooze-menu")
x.move(1160, 128, 0.35)
wait(0.3)
x.click(1160, 180)         # Tomorrow
wait(1.8)
shot("snoozed")
x.key("Escape")            # close the reader
wait(1.2)

mark("Select, select all, act on many")
# ---------------------------------------------------------------- multi-select
x.click(1856, 28)          # Select
wait(0.9)
for y in (282, 376, 471):
    x.click(900, y)
    wait(0.55)
shot("three-selected")
wait(0.6)
x.key("ctrl+a")            # Select all
wait(1.2)
shot("all-selected")
x.key("e")                 # Done on everything...
wait(1.6)
shot("all-done")
x.key("z")                 # ...and undo
wait(1.8)
x.key("Escape")
wait(1.0)

mark("Every list, all accounts or one")
# ---------------------------------------------------------------- unified lists, per account
x.click(80, 408)           # Sent: all accounts, unfolds
wait(1.5)
shot("sent-unfolded")
x.click(120, 490)          # Sent > Personal
wait(1.4)
x.click(120, 449)          # Sent > Northwind
wait(1.4)
shot("sent-northwind")
x.click(80, 408)           # Sent, all accounts again
wait(1.0)
x.click(80, 408)           # fold Sent
wait(1.0)
x.click(80, 449)           # Drafts
wait(1.4)
shot("drafts")
x.click(80, 449)           # fold Drafts
wait(0.8)
x.click(80, 78)            # back to the Smart Inbox
wait(1.0)
x.click(80, 78)            # fold
wait(1.0)

mark("Settings for everything")
# ---------------------------------------------------------------- settings
x.click(80, 1048)
wait(1.3)
shot("settings-general")
x.click(1370, 443)         # undo time 10 s
wait(0.8)
x.click(1308, 443)         # back to 5 s
wait(0.8)
x.click(110, 227)          # Appearance
wait(1.2)
mark("Light, dark, or follow the system")
x.click(1428, 136)         # Light
wait(1.8)
shot("settings-light")
x.click(1504, 136)         # Dark
wait(1.8)
x.click(1352, 136)         # System
wait(1.6)
mark("Compact density")
x.click(1484, 232)         # Compact
wait(1.0)
x.key("Escape")            # back to mail: compact list
wait(1.8)
shot("compact-list")
x.key("ctrl+comma")
wait(1.0)
x.click(110, 227)
wait(0.8)
x.click(1372, 232)         # Comfortable
wait(0.9)
mark("Add any IMAP account")
x.click(110, 137)          # Accounts
wait(1.4)
shot("settings-accounts")
x.click(1484, 56)          # Add account
wait(1.1)
x.click(1100, 200)
x.type("Alex Rivera")
x.click(1100, 275)
x.type("alex@studio.example")
x.click(1100, 350)
x.type("demo-password")
wait(0.5)
x.click(740, 393)          # Server settings
wait(1.6)
shot("add-account")
x.key("Escape")            # back to Accounts
wait(0.9)
mark("Keyboard shortcuts")
x.click(110, 272)          # Shortcuts
wait(2.0)
shot("shortcuts")
x.key("Escape")
wait(1.0)

mark("Matches your desktop theme, live")
# ---------------------------------------------------------------- follows the system theme, live
x.click(820, 282)
wait(2.2)
x.move(1700, 1000, 0.6)
for name in ("tokyo-night", "catppuccin-latte", "everforest", "rose-pine", "nord", "gruvbox"):
    theme(name)
    wait(2.6)
    shot(f"theme-{name}")
wait(1.0)
x.key("Escape")
wait(1.5)

mark("END")
print(f"tour {time.time() - t0:.1f}s", flush=True)
import json
json.dump(MARKS, open(f"{D}/marks.json", "w"), indent=1)
if rec:
    rec.communicate(b"q", timeout=30)
