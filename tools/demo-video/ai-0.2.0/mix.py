"""Mix the 0.2.0 soundtrack: VO sentences + whoosh/tick/riser/hit SFX (no music) -> assets/audio/mix.wav (-16 LUFS)."""
import os, subprocess, sys

from timeline import S, TOTAL, W

MEDIA = os.path.expanduser("~/side projects/rusty-inbox-video-media")
A = f"{MEDIA}/audio"
OUT = f"{MEDIA}/video/assets/audio/mix.wav"
os.makedirs(os.path.dirname(OUT), exist_ok=True)

inputs, chains, labels = [], [], []


def add(path, t, vol, pre=""):
    i = len(inputs)
    inputs.append(path)
    ms = int(max(t, 0) * 1000)
    chains.append(f"[{i}:a]aformat=sample_rates=48000:channel_layouts=mono,{pre}volume={vol},adelay={ms}|{ms}[a{i}]")
    labels.append(f"[a{i}]")


for k in sorted(S):
    for s in S[k]["sents"]:
        add(s["file"], s["t0"], 1.0)

for k in sorted(S):
    if k > 1:
        add(f"{A}/sfx_whoosh1.mp3" if k % 2 == 0 else f"{A}/sfx_whoosh2.mp3", S[k]["start"] - 0.3, 0.55 if k % 2 == 0 else 0.7)
# riser into the end card, then the hit
t8 = S[8]["start"]
add(f"{A}/sfx_riser.mp3", t8 - 1.7, 0.30, pre="atrim=start=0.8,asetpts=PTS-STARTPTS,afade=t=in:d=0.5,")
add(f"{A}/sfx_hit.mp3", t8 - 0.05, 0.42, pre="atrim=end=1.2,afade=t=out:st=0.7:d=0.5,")
# a few soft ticks on UI emphasis
for t in (W(2, "chat", 0, 0), W(3, "replace", 0, 3), W(4, "summar", 0, 0), W(5, "risk", 0, 0), W(6, "needs", 0, 0), W(7, "off", 0, 0)):
    add(f"{A}/sfx_tick.mp3", t + 0.25, 0.22)

graph = ";".join(chains) + ";" + "".join(labels) + f"amix=inputs={len(labels)}:normalize=0:dropout_transition=0,apad=whole_dur={TOTAL:.2f},atrim=end={TOTAL:.2f},pan=stereo|c0=c0|c1=c0,loudnorm=I=-16:TP=-1.5:LRA=7[out]"
open(f"{A}/mix.graph", "w").write(graph)
base = ["ffmpeg", "-v", "error", "-y"]
for p_ in inputs:
    base += ["-i", p_]
tail = ["-map", "[out]", "-ar", "48000", "-ac", "2", OUT]
r = subprocess.run(base + ["-/filter_complex", f"{A}/mix.graph"] + tail, capture_output=True, text=True)
if r.returncode:
    r = subprocess.run(base + ["-filter_complex", graph] + tail, capture_output=True, text=True)
    if r.returncode:
        print(r.stderr[-1500:]); sys.exit(1)
loud = subprocess.run(["ffmpeg", "-nostats", "-i", OUT, "-af", "ebur128", "-f", "null", "-"], capture_output=True, text=True).stderr
print([l for l in loud.splitlines() if "I:" in l][-1:], "total", round(TOTAL, 2))
