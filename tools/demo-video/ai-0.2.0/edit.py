"""Cut the per-scene footage clips from the raw beat recordings, retimed to the VO timeline.

    python3 edit.py [scene ...]       # writes $MEDIA/video/assets/clips/sNN.mp4 (no audio, 30 fps, native crop size)

Each shot: (beat, src_a, src_b, until, [crop]) -> source seconds [a, b] are stretched/squeezed to end at
`until` (seconds from the scene start; the previous shot's `until` is its start). A shot that would need to
play slower than 1x is played at 1x and its last frame is held. Source times use M(beat, mark, delta):
video time = mark time + OFF (the recorder starts ffmpeg ~OFF s before the beat's clock starts).
"""
import json, os, subprocess, sys

from timeline import S, TOTAL, W

MEDIA = os.path.expanduser("~/side projects/rusty-inbox-video-media")
REC = f"{MEDIA}/rec"
OUT = f"{MEDIA}/video/assets/clips"
TMP = f"{MEDIA}/tmp"
os.makedirs(OUT, exist_ok=True); os.makedirs(TMP, exist_ok=True)
OFF = 0.8
MK = {b: {l: t for t, l in json.load(open(f"{REC}/{b}.json"))} for b in
      ["settings", "write", "read", "attach", "safe", "ontop", "privacy"]}


def M(beat, mark, d=0.0):
    return round(MK[beat][mark] + OFF + d, 2)


def local(sc, sentence, d=0.0):
    """Seconds from the scene start to the start of a sentence (+d)."""
    return round(S[sc]["sents"][sentence]["t0"] - S[sc]["start"] + d, 2)


def dur(sc):
    return round(S[sc]["end"] - S[sc]["start"], 2)


def settle(beat, t0, t1, region, thr=0.03):
    """Video time of the last big change in `region` between t0 and t1: when the answer has finished
    streaming (the 'working' dots change far less than text does)."""
    import numpy as np
    x, y, w, h = region
    t1 = min(t1, float(subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f"{REC}/{beat}.mkv"])) - 0.1)
    p = subprocess.run(["ffmpeg", "-v", "error", "-ss", f"{t0}", "-t", f"{t1 - t0}", "-i", f"{REC}/{beat}.mkv", "-vf",
                        f"fps=10,crop={w}:{h}:{x}:{y},format=gray", "-f", "rawvideo", "-"], capture_output=True)
    a = np.frombuffer(p.stdout, dtype=np.uint8).reshape(-1, h, w).astype(float)
    d = np.abs(np.diff(a, axis=0)).mean(axis=(1, 2))
    idx = np.nonzero(d > thr)[0]
    t = t0 + (idx[-1] + 1) / 10 if len(idx) else t0 + 1.0
    print(f"    settle {beat} [{t0:.1f},{t1:.1f}] -> {t:.1f}")
    return round(t, 2)


# crops (x, y, w, h) in the 1920x1080 recording; all shots of one scene share the aspect ratio
PANE_R = (900, 40, 1020, 900)          # reader pane (AI strip, cards, attachments)
PANE_W = (900, 180, 1020, 900)         # composer pane (body, previews, bottom bar)
SET = (640, 120, 1000, 880)            # settings column
LIST = (0, 0, 1000, 880)              # sidebar + list
BRIEF = (560, 0, 1000, 880)
RG_BOT = (900, 650, 1020, 380)         # preview panels at the bottom of the composer
RG_TOP = (900, 100, 1020, 500)         # result cards above the message
RG_BODY = (900, 250, 1020, 700)
RG_LIST = (360, 60, 1000, 600)

SCENES = {}


def define():
    d_su = settle("read", M("read", "summary-click"), M("read", "summary"), RG_TOP)
    d_re = settle("read", M("read", "replies-click"), M("read", "replies"), RG_TOP)
    d_tr = settle("read", M("read", "translate-click"), M("read", "translated"), RG_BODY)
    d_at = settle("attach", M("attach", "attach-click"), M("attach", "attach-summary"), RG_TOP)
    d_ph = settle("safe", M("safe", "phish-click"), M("safe", "phish"), RG_TOP)
    d_pf = settle("write", M("write", "proofread-click"), M("write", "proofread-diff"), RG_BOT)
    d_rw = settle("write", M("write", "rewrite-click"), M("write", "rewrite-preview"), RG_BOT)
    d_wr = settle("write", M("write", "write-click"), M("write", "write-preview"), RG_BOT)
    d_se = settle("ontop", M("ontop", "search-typed", 0.8), M("ontop", "search-result"), RG_LIST)

    SCENES[1] = [                       # cover + hook: three results in a row
        ("read", d_su - 0.5, d_su + 0.5, 3.9, PANE_R),
        ("write", d_pf - 0.8, d_pf + 0.4, 6.0, PANE_W),
        ("safe", d_ph - 0.6, d_ph + 0.6, dur(1) + 0.6, PANE_R),
    ]
    SCENES[2] = [                       # choose your AI (settings)
        ("settings", M("settings", "providers", -1.3), M("settings", "providers", 4.1), local(2, 1, -0.05), SET),
        ("settings", M("settings", "providers", 0.9), M("settings", "providers", 7.0), local(2, 2, -0.05), SET),
        ("settings", M("settings", "model", -1.2), M("settings", "thinking", 3.9), dur(2) + 0.6, SET),
    ]
    SCENES[3] = [                       # write
        ("write", 8.0, 9.0, 0.9, PANE_W),
        ("write", M("write", "proofread-click", -0.5), d_pf + 0.6, local(3, 1, -0.05), PANE_W),
        ("write", M("write", "rewrite-menu", -0.3), d_rw + 0.6, local(3, 2, -0.05), PANE_W),
        ("write", M("write", "write-click", -1.0), M("write", "write-click", 0.0), local(3, 2, 1.0), PANE_W),
        ("write", M("write", "write-click", 0.0), d_wr + 0.6, local(3, 3, -0.05), PANE_W),
        ("write", M("write", "write-preview", -1.0), M("write", "write-replaced", 3.4), dur(3) + 0.6, PANE_W),
    ]
    SCENES[4] = [                       # read
        ("read", M("read", "summary-click", -0.3), d_su + 0.5, local(4, 1, -0.05), PANE_R),
        ("read", M("read", "replies-click", -0.2), d_re + 0.4, local(4, 2, -1.9), PANE_R),
        ("read", M("read", "reply-opened", -3.0), M("read", "reply-opened", 0.4), local(4, 2, -0.05), PANE_R),
        ("read", M("read", "arabic", -0.5), d_tr + 0.4, local(4, 2, 1.85), PANE_R),
        ("attach", M("attach", "start", 0.3), d_at + 0.4, dur(4) + 0.6, PANE_R),
    ]
    SCENES[5] = [
        ("safe", M("safe", "phish-click", -1.2), d_ph + 0.8, dur(5) + 0.6, PANE_R),
    ]
    SCENES[6] = [
        ("ontop", M("ontop", "start", 0.5), M("ontop", "follow-up", -1.0), local(6, 1, -0.05), LIST),
        ("ontop", M("ontop", "follow-up", -1.0), M("ontop", "follow-up", 2.2), local(6, 2, -0.05), LIST),
        ("ontop", M("ontop", "search-typed", -2.6), M("ontop", "search-typed", 0.8), local(6, 2, 1.65), LIST),
        ("ontop", M("ontop", "search-typed", 0.8), d_se + 0.6, local(6, 3, -0.05), LIST),
        ("ontop", M("ontop", "briefing", -0.3), M("ontop", "briefing-scrolled", 1.5), dur(6) + 0.6, BRIEF),
    ]
    SCENES[7] = [
        ("privacy", M("privacy", "top", 1.0), M("privacy", "on", 0.8), local(7, 1, -0.05), SET),
        ("privacy", M("privacy", "on", 1.0), M("privacy", "excluded", 2.0), dur(7) + 0.6, SET),
    ]


def run(args):
    r = subprocess.run(["ffmpeg", "-v", "error", "-y"] + args, capture_output=True, text=True)
    if r.returncode:
        print(r.stderr[-800:]); sys.exit(1)


def build(sc):
    segs = SCENES[sc]
    files, t_prev = [], 0.0
    for i, (beat, a, b, until, crop) in enumerate(segs):
        d = round(until - t_prev, 3)
        t_prev = until
        x, y, w, h = crop
        vd = float(subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f"{REC}/{beat}.mkv"]))
        b = min(b, vd - 0.1)
        src = b - a
        speed = max(1.0, src / d)                 # never slower than real time
        play = src / speed                         # seconds of output the source fills
        vf = f"crop={w}:{h}:{x}:{y},setpts=(PTS-STARTPTS)/{speed:.4f},fps=30"
        if play < d - 0.01:
            vf += f",tpad=stop_mode=clone:stop_duration={d - play + 0.1:.3f}"
        f = f"{TMP}/s{sc}_{i}.mp4"
        run(["-ss", f"{a}", "-t", f"{src}", "-i", f"{REC}/{beat}.mkv", "-vf", vf, "-t", f"{d}", "-an",
             "-c:v", "libx264", "-crf", "15", "-preset", "fast", "-pix_fmt", "yuv420p", f])
        files.append(f)
        print(f"  scene {sc} shot {i}: {beat} src {a}-{b} ({src:.1f}s) -> {d:.2f}s speed {speed:.2f}x")
    lst = f"{TMP}/s{sc}.txt"
    open(lst, "w").write("".join(f"file '{f}'\n" for f in files))
    run(["-f", "concat", "-safe", "0", "-i", lst, "-c:v", "libx264", "-crf", "16", "-preset", "medium",
         "-pix_fmt", "yuv420p", "-r", "30", "-movflags", "+faststart", f"{OUT}/s{sc}.mp4"])


if __name__ == "__main__":
    define()
    for sc in ([int(a) for a in sys.argv[1:]] or sorted(SCENES)):
        build(sc)
