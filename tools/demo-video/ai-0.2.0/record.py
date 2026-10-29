"""Scripted recording of the Rusty Inbox 0.2.0 AI features on the demo mailbox (display :78).

    python3 record.py dry  <beat|all>     # run with a screenshot at every mark (no video)
    python3 record.py rec  <beat|all>     # record rec/<beat>.mkv + rec/<beat>.json (marks)

Beats are independent (each starts from the mail list) so one can be re-recorded alone.
The app must already be running (`. env.sh; ../demo-env.sh x; ../demo-env.sh app`) with prep.py data.
"""
import json, os, subprocess, sys, time

from h import x

MEDIA = os.path.expanduser("~/side projects/rusty-inbox-video-media")
REC = f"{MEDIA}/rec"
SHOTS = f"{MEDIA}/shots"
os.makedirs(REC, exist_ok=True); os.makedirs(SHOTS, exist_ok=True)
MODE = sys.argv[1]
WHICH = sys.argv[2:] or ["all"]

# rows of the list (Smart Inbox folded): 94.5 px apart
ROW = {"maya": 282, "cloudhost": 376, "jonas": 471, "nadia": 566, "priya": 660, "youssef": 755, "diego": 850, "hannah": 944}
T0 = [None]; MARKS = []; REC_P = [None]; CUR = [""]; N = [0]


def mark(label, shot=False):
    t = round(time.time() - T0[0], 2) if T0[0] else 0
    MARKS.append((t, label))
    if MODE == "dry" or shot:
        N[0] += 1
        x.shot(f"{SHOTS}/{CUR[0]}-{N[0]:02d}-{label}.png")


def start(name):
    CUR[0] = name; N[0] = 0; MARKS.clear()
    if MODE == "rec":
        REC_P[0] = subprocess.Popen(
            ["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse", "1", "-framerate", "60",
             "-video_size", "1920x1080", "-i", ":78", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "14",
             "-pix_fmt", "yuv420p", f"{REC}/{name}.mkv"], stdin=subprocess.PIPE)
        time.sleep(0.8)
    T0[0] = time.time()


def stop():
    time.sleep(0.8)
    if MODE == "rec" and REC_P[0]:
        REC_P[0].communicate(b"q", timeout=60)
        json.dump(MARKS, open(f"{REC}/{CUR[0]}.json", "w"), indent=1)
    print(CUR[0], "marks:", MARKS, flush=True)


def esc(n=3):
    for _ in range(n):
        x.key("Escape"); time.sleep(0.4)
    time.sleep(0.6)


def folded():
    from PIL import Image, ImageChops
    x.shot("/tmp/_home.png")
    a = Image.open("/tmp/_home.png").convert("L").crop((20, 332, 230, 362))
    b = Image.open(f"{MEDIA}/ref_folded.png")
    d = ImageChops.difference(a, b)
    return sum(d.getdata()) / (210 * 30) < 2.0


def home():
    """Mail list of the Smart Inbox, accounts folded, nothing open."""
    esc()
    x.click(90, 140); time.sleep(1.4)
    if not folded():
        x.click(90, 140); time.sleep(1.4)
    assert folded(), "home(): sidebar state not recognised"
    x.warp(1500, 700)


def open_row(who):
    x.click(820, ROW[who]); time.sleep(2.4)


def sidebar(y):
    x.click(80, y); time.sleep(1.6)


# ------------------------------------------------------------------ beats
def beat_settings():
    home(); x.warp(1200, 300)
    start("settings")
    mark("start")
    x.click(80, 1048); time.sleep(1.2)
    x.click(80, 272); time.sleep(1.8)
    mark("providers")
    for y in (380, 530, 622, 714, 806, 898):
        x.move(1000, y, 0.5); time.sleep(0.45)
    time.sleep(0.8)
    x.scroll(6, 1300, 700); time.sleep(1.2)
    mark("model")
    x.click(1080, 594); time.sleep(1.6)
    mark("picker")
    x.type("luna", cps=4); time.sleep(1.6)
    mark("search")
    x.click(800, 716); time.sleep(1.0)       # GPT-6-Luna
    mark("thinking")
    x.click(868, 768); time.sleep(1.0)       # Medium
    x.click(798, 768); time.sleep(1.0)       # Low
    x.click(1444, 866)                        # Test connection
    time.sleep(1.0)
    mark("testing")
    time.sleep(6.0)
    mark("tested")
    stop()


def beat_write():
    home()
    open_row("maya")
    x.click(1000, 1040); time.sleep(2.0)
    x.click(1100, 300); time.sleep(0.5)
    start("write")
    mark("typing")
    x.type("hi maya, tuesday is fine for the anouncement. i will test the packges on a clean machine by thursday and send you the benchmark numbres befor the sync", cps=26)
    time.sleep(0.9)
    x.click(1270, 1040)
    mark("proofread-click")
    x.move(1500, 800, 0.4)
    time.sleep(9.0)
    mark("proofread-diff")
    time.sleep(1.5)
    x.click(1658, 956); time.sleep(1.4)
    mark("replaced")
    x.click(1412, 1040); time.sleep(1.2)
    mark("rewrite-menu")
    x.click(1410, 846)
    mark("rewrite-click")
    x.move(1500, 800, 0.4)
    time.sleep(11.0)
    mark("rewrite-preview")
    time.sleep(1.5)
    x.click(1842, 956); time.sleep(1.0)       # Cancel: nothing changes
    mark("cancelled")
    x.click(1888, 28); time.sleep(1.4)         # close the composer
    open_row("hannah")
    x.click(1000, 1040); time.sleep(2.0)
    x.click(1100, 300); time.sleep(0.4)
    x.click(1530, 1040); time.sleep(1.1)
    x.click(1590, 964); time.sleep(1.0)
    mark("write-for-me")
    x.click(1100, 860); time.sleep(0.4)
    x.type("Tell Hannah we keep one IDLE connection per account and only poll as a fallback.", cps=22)
    time.sleep(0.6)
    x.click(1760, 956)
    mark("write-click")
    x.move(1500, 800, 0.4)
    time.sleep(11.0)
    mark("write-preview")
    time.sleep(1.5)
    x.click(1658, 956); time.sleep(1.4)
    mark("write-replaced")
    time.sleep(1.5)
    stop()
    x.click(1888, 28); time.sleep(1.0)


def beat_read():
    home()
    open_row("maya")
    x.warp(1500, 600)
    start("read")
    mark("start")
    x.click(1026, 86)
    mark("summary-click")
    x.move(1500, 980, 0.5)
    time.sleep(12.0)
    mark("summary")
    time.sleep(1.5)
    x.click(1848, 142); time.sleep(0.8)
    x.click(1260, 86)
    mark("replies-click")
    x.move(1500, 980, 0.5)
    time.sleep(12.0)
    mark("replies")
    time.sleep(1.5)
    x.click(1100, 196); time.sleep(2.2)
    mark("reply-opened")
    time.sleep(1.5)
    x.click(1888, 28); time.sleep(1.2)
    open_row("youssef")
    mark("arabic")
    time.sleep(1.2)
    x.click(1148, 86)
    mark("translate-click")
    x.move(1500, 980, 0.5)
    time.sleep(12.0)
    mark("translated")
    time.sleep(1.5)
    esc(1)
    stop()


def beat_attach():
    home()
    x.click(820, 140); time.sleep(2.4)       # Lea Martin (pinned)
    start("attach")
    mark("start")
    time.sleep(1.2)
    x.click(1328, 648)                        # attachment summarize sparkle
    mark("attach-click")
    x.move(1500, 980, 0.5)
    time.sleep(12.0)
    mark("attach-summary")
    time.sleep(1.5)
    stop()


def beat_safe():
    home()
    open_row("cloudhost")
    x.warp(1500, 900)
    start("safe")
    mark("start")
    time.sleep(1.8)
    x.click(1400, 86)
    mark("phish-click")
    x.move(1500, 1000, 0.5)
    time.sleep(10.0)
    mark("phish")
    time.sleep(1.8)
    stop()


def beat_ontop():
    home()
    start("ontop")
    mark("start")
    sidebar(346)
    mark("needs-reply")
    time.sleep(2.2)
    sidebar(388)
    mark("follow-up")
    time.sleep(2.4)
    sidebar(140)                  # Smart Inbox (unfolds the accounts)
    sidebar(140)                  # fold again
    x.click(1810, 26); time.sleep(0.9)
    mark("search-open")
    x.type("? invoices from Cloudhost last month", cps=14); time.sleep(0.8)
    mark("search-typed")
    x.key("Return")
    time.sleep(12.0)
    mark("search-result")
    time.sleep(2.0)
    esc(2)
    sidebar(428)
    mark("briefing")
    time.sleep(3.0)
    x.scroll(3, 1300, 700); time.sleep(2.0)
    mark("briefing-scrolled")
    time.sleep(1.5)
    stop()


def beat_privacy():
    home(); x.warp(1200, 300)
    x.click(80, 1048); time.sleep(1.2)
    x.click(80, 272); time.sleep(1.8)
    start("privacy")
    mark("top")
    time.sleep(1.6)
    x.click(1508, 212); time.sleep(1.8)        # Use AI off
    mark("off")
    x.click(1508, 212); time.sleep(1.6)        # and on again
    mark("on")
    x.scroll(40, 1300, 700); time.sleep(1.4)
    mark("bottom")
    time.sleep(1.2)
    x.click(684, 704); time.sleep(1.8)         # exclude Personal
    mark("excluded")
    x.click(684, 704); time.sleep(1.0)         # and back
    time.sleep(1.2)
    stop()
    esc(2)


BEATS = {"settings": beat_settings, "write": beat_write, "read": beat_read, "attach": beat_attach,
         "safe": beat_safe, "ontop": beat_ontop, "privacy": beat_privacy}
for b in (BEATS if WHICH == ["all"] else WHICH):
    BEATS[b]()
